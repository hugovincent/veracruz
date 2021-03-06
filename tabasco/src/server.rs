//! The Tabasco server
//!
//! ## Authors
//!
//! The Veracruz Development Team.
//!
//! ## Licensing and copyright notice
//!
//! See the `LICENSE.markdown` file in the Veracruz root directory for
//! information on licensing and copyright.

use crate::attestation;
#[cfg(feature = "psa")]
use crate::attestation::psa;
#[cfg(feature = "sgx")]
use crate::attestation::sgx;
use crate::error::*;
use actix_web::{dev::Server, middleware, web, App, HttpServer};
use psa_attestation::{
    q_useful_buf_c, t_cose_crypto_lib_t_T_COSE_CRYPTO_LIB_PSA, t_cose_key,
    t_cose_key__bindgen_ty_1, t_cose_parameters, t_cose_sign1_set_verification_key,
    t_cose_sign1_verify, t_cose_sign1_verify_ctx, t_cose_sign1_verify_delete_public_key,
    t_cose_sign1_verify_init, t_cose_sign1_verify_load_public_key,
};
use std::{ffi::c_void, ptr::null};

async fn verify_iat(input_data: String) -> TabascoResponder {
    if input_data.is_empty() {
        return Err(TabascoError::MissingFieldError("tabasco::verify_iat data"));
    }

    let proto_bytes = base64::decode(&input_data)?;

    let proto = colima::parse_tabasco_request(&proto_bytes)?;
    if !proto.has_proxy_psa_attestation_token() {
        return Err(TabascoError::NoProxyPSAAttestationTokenError);
    }

    let (token, pubkey, device_id) =
        colima::parse_proxy_psa_attestation_token(proto.get_proxy_psa_attestation_token());

    let pubkey_hash = {
        let conn = crate::orm::establish_connection()?;
        crate::orm::query_device(&conn, device_id)?
    };

    // verify that the pubkey we received matches the hash we received
    // during native attestation
    let calculated_pubkey_hash = ring::digest::digest(&ring::digest::SHA256, pubkey.as_ref());
    if calculated_pubkey_hash.as_ref().to_vec() != pubkey_hash {
        return Err(TabascoError::MismatchError {
            variable: "Tabasco::server public key",
            received: calculated_pubkey_hash.as_ref().to_vec(),
            expected: pubkey_hash,
        });
    }

    let mut t_cose_ctx: t_cose_sign1_verify_ctx = unsafe { ::std::mem::zeroed() };
    unsafe { t_cose_sign1_verify_init(&mut t_cose_ctx, 0) };

    let mut key_handle: u16 = 0;
    let lpk_ret = unsafe {
        t_cose_sign1_verify_load_public_key(
            pubkey.as_ptr() as *const u8,
            pubkey.len() as u64,
            &mut key_handle,
        )
    };
    if lpk_ret != 0 {
        return Err(TabascoError::UnsafeCallError(
            "tabasco::server::verify_iat t_cose_sign1_verify_load_public_key",
            lpk_ret,
        ));
    }

    let cose_key = t_cose_key {
        crypto_lib: t_cose_crypto_lib_t_T_COSE_CRYPTO_LIB_PSA,
        k: t_cose_key__bindgen_ty_1 {
            key_handle: key_handle as u64,
        },
    };
    unsafe { t_cose_sign1_set_verification_key(&mut t_cose_ctx, cose_key) };
    let sign1 = q_useful_buf_c {
        ptr: token.as_ptr() as *mut c_void,
        len: token.len() as u64,
    };
    let mut payload_vec = Vec::with_capacity(token.len());
    let mut payload = q_useful_buf_c {
        ptr: payload_vec.as_mut_ptr() as *mut c_void,
        len: payload_vec.capacity() as u64,
    };

    let mut decoded_parameters: t_cose_parameters = unsafe { ::std::mem::zeroed() };

    let sv_ret = unsafe {
        t_cose_sign1_verify(
            &mut t_cose_ctx,
            sign1,
            &mut payload,
            &mut decoded_parameters,
        )
    };
    if sv_ret != 0 {
        return Err(TabascoError::UnsafeCallError(
            "tabasco::server::verify_iat t_cose_sign1_verify",
            sv_ret,
        ));
    }

    // remove the key from storage
    let dpk_ret = unsafe { t_cose_sign1_verify_delete_public_key(&mut key_handle) };
    if dpk_ret != 0 {
        println!("tabasco::attestation::psa_attestation_token Was unable to delete public key, and received the error code:{:?}.
                   I can't do anything about it, and it may not cause a problem right now, but this will probably end badly for you.", dpk_ret);
    }

    if payload.ptr == null() {
        return Err(TabascoError::MissingFieldError("payload.ptr"));
    }

    let payload_vec =
        unsafe { std::slice::from_raw_parts(payload.ptr as *const u8, payload.len as usize) };

    Ok(base64::encode(payload_vec))
}

#[allow(unused)]
async fn sgx_router(psa_request: web::Path<String>, input_data: String) -> TabascoResponder {
    #[cfg(feature = "sgx")]
    match psa_request.into_inner().as_str() {
        "Msg1" => sgx::msg1(input_data),
        "Msg3" => sgx::msg3(input_data),
        _ => Err(TabascoError::UnsupportedRequestError),
    }
    #[cfg(not(feature = "sgx"))]
    Err(TabascoError::UnimplementedRequestError)
}

#[allow(unused)]
async fn psa_router(psa_request: web::Path<String>, input_data: String) -> TabascoResponder {
    #[cfg(feature = "psa")]
    if psa_request.into_inner().as_str() == "AttestationToken" {
        psa::attestation_token(input_data)
    } else {
        Err(TabascoError::UnsupportedRequestError)
    }
    #[cfg(not(feature = "psa"))]
    Err(TabascoError::UnimplementedRequestError)
}

pub fn server(url: String) -> Result<Server, String> {
    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/VerifyPAT", web::post().to(verify_iat))
            .route("/Start", web::post().to(attestation::start))
            .route("/SGX/{sgx_request}", web::post().to(sgx_router))
            .route("/PSA/{psa_request}", web::post().to(psa_router))
    })
    .bind(&url)
    .map_err(|err| format!("binding error: {:?}", err))?
    .run();
    Ok(server)
}

hash_value=$(dd skip=960 count=32 if=../sonora/css.bin bs=1 status=none| xxd -ps -cols 32)
echo $hash_value
rm -f tabasco.db
diesel --config-file ../tabasco/diesel.toml setup
echo "INSERT INTO firmware_versions VALUES(1, 'sgx', '0.3.0', '${hash_value}');" > tmp.sql
echo "INSERT INTO firmware_versions VALUES(2, 'psa', '0.3.0', 'deadbeefdeadbeefdeadbeefdeadbeeff00dcafef00dcafef00dcafef00dcafe');" >> tmp.sql
sqlite3 tabasco.db < tmp.sql

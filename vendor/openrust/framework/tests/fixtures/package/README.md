# Package acceptance fixture

The independent Cargo workspace has two members. The provider lives in
`package` and the host installs it as a dependency in `app`.
The HTTP and Artisan probe runs with resources copied outside both crates.

From the repository root:

```sh
fixture=vendor/openrust/framework/tests/fixtures/package
cp Cargo.lock "$fixture/Cargo.lock"
export CARGO_TARGET_DIR="$PWD/target"
cargo +1.94.0 build --offline --manifest-path "$fixture/Cargo.toml"
cargo +1.94.0 test --offline --locked --manifest-path "$fixture/Cargo.toml"
python3 "$fixture/verify.py" target/debug/package-probe-app
```

Set `PROBE_MYSQL_URL` to the URL of a dedicated empty MySQL test database to
run the same migration and rollback on MySQL. The probe creates the migration
ledger in that database.

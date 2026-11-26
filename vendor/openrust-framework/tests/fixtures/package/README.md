# Package acceptance fixture

The independent Cargo workspace has a package and a host. The host declares
the package as a renamed optional dependency. Its default feature discovers the
provider and a Rust alias. The HTTP and Artisan probe deploys the assets outside
both crates and runs the binaries with an empty `PATH`.
The host's `packages` module encapsulates the generated manifest and keeps the
compiled `package_aliases` export available, matching the application bootstrap.

From the repository root:

```sh
fixture=vendor/openrust-framework/tests/fixtures/package
mkdir -p target/tmp/package-acceptance
export TMPDIR="$PWD/target/tmp/package-acceptance"
binaries=$(mktemp -d)
cp Cargo.lock "$fixture/Cargo.lock"
export CARGO_TARGET_DIR="$PWD/target/tmp/package-acceptance/target"
cargo +1.95.0 build --offline --manifest-path "$fixture/app/Cargo.toml"
cp "$CARGO_TARGET_DIR/debug/package-probe-app" "$binaries/with-package"
cargo +1.95.0 build --offline --locked --no-default-features --manifest-path "$fixture/app/Cargo.toml"
cp "$CARGO_TARGET_DIR/debug/package-probe-app" "$binaries/without-package"
cargo +1.95.0 test --offline --locked --manifest-path "$fixture/Cargo.toml"
python3 "$fixture/verify.py" "$binaries/with-package" "$binaries/without-package"
```

The first binary exercises discovery, the package cache, Artisan commands, JSON
and namespaced translations, and typed and anonymous Blade components.
Additional binaries passed to `verify.py` should exclude the package so the
script can check the absence of its route, command and manifest entry.

Set `PROBE_MYSQL_URL` to a dedicated empty MySQL test database to run the same
migration and rollback on MySQL. The probe creates the migration ledger there.

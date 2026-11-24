# Getting Started With Testing

## Running tests {#running-tests}

Place integration tests under the application's `tests/` directory or in the relevant crate. Run the workspace tests with the committed lockfile.

Run the application’s tests with Artisan or run every workspace member with Cargo. From Artisan, use Cargo options before the test filter and use `--` after the filter to pass options to Rust’s test harness. An initial `--` immediately after `test` forwards options that overlap with Artisan, such as Cargo’s `--help`.

```sh
./artisan test --locked --test feature artisan_inspire_prints_an_attributed_quote -- --exact
cargo +1.94.0 test --workspace --locked
```

Artisan runs Cargo from the application root, streams its output and returns its exit status. It uses the `CARGO` executable from the environment when set, otherwise `cargo` from the executable search path.

## Parallel Tests {#parallel-tests}

```sh
./artisan test --parallel --processes=4 --locked --test feature
./artisan test --parallel --processes=2 --test feature health -- --exact
./artisan test --parallel --test feature -- --list
```

Cargo builds the test artifacts once. The native runner lists matching libtest cases and runs each case in its own child process, with `--processes` limiting concurrent children. Each child receives a distinct string `TEST_TOKEN`. A failed case makes the command return 1; Ctrl+C returns 130. Listing does not run the cases. Filters, `--exact`, `--skip name`, `--ignored`, `--include-ignored`, `--nocapture`, and `--show-output` are supported; unsupported harness options return an error. Ordinary `artisan test` keeps forwarding to Cargo.

Register hooks in a testing Provider's boot method, where the application facade context is active:

```rust
use openrust::{
    contracts::foundation::Application as _,
    support::facades::ParallelTesting,
    testing::ParallelTestError,
};

pub fn register_parallel_hooks() -> Result<(), ParallelTestError> {
    ParallelTesting::set_up_process(|app, token| Box::pin(async move {
        app.config()?.set("testing.token", token)?;
        Ok(())
    }))
}
```

Use `set_up_test_case` / `tear_down_test_case` for callbacks receiving the actual mutable `TestApp`; process and database hooks receive the actual application and token. `set_up_test_database_before_migrating` and `set_up_test_database` run around the first migration preparation of a managed MySQL fixture. Repeated refreshes do not repeat them. A callback or migration failure does not turn the existing database into a newly created one on retry.

Generated feature tests use `TestApp::run_parallel_test`. Wrap existing feature-test bodies with it to run the test-case hooks and, in a native parallel child, the process hooks. The wrapper runs teardown after a returned error or panic. `ParallelTesting::run_process` is available for a custom runner; the six `call_*_callbacks` methods can also be invoked explicitly. Do not wrap the native child in a second process lifecycle.

`option` and `token` preserve false versus string values. `resolve_options_using(Some(Arc::new(...)))` and `resolve_token_using` install application-local resolvers; None restores the captured process environment. Resolver errors propagate. Native callbacks receive typed application/fixture arguments and may resolve other services through the application. They run outside registration locks and may reenter registration.

Default application services prepare a global cache prefix of `original_prefix + test_<token>_` before test-case callbacks. `--without-cache` preserves the configured prefix. Existing store rules remain: store-level prefixes take precedence, file-cache paths are unchanged, and a repository resolved before setup keeps its prefix until explicitly purged. String prefixes retain their original base across repeated setup; explicit null follows the fixed recapture behavior, so prefer a string prefix.

When `view.compiled` is enabled, the process creates `original_path/test_<token>`. Setup updates both the configuration and any already resolved View or Blade compiler; retained view instances also use the new directory. Normal process teardown removes only that token's directory, preserving its parent and other tokens. A missing or disabled compiled path is left alone. An explicitly null path can be recaptured after configuration changes. Custom tokens used in paths reject separators and reserved characters before filesystem changes.

The application binding enables these defaults. A standalone `testing::ParallelTesting::new` remains a bare lifecycle service; use `with_default_isolation()` when creating a custom replacement that should retain the standard isolation behavior.

The current native adapter uses an isolated process per test, rather than keeping a worker alive across multiple cases. Existing owned MySQL fixtures still create and clean their own databases. Automatic database-testing strategy integration, asynchronous hook/database cleanup on cancellation, and native Linux/Windows process acceptance remain separate open items; they are not provided by the lifecycle API alone.

Test-case teardown invokes registered cleanup callbacks without preparing cache prefixes or compiled-view paths again; configuration changes made by the test remain visible to those callbacks.

### Reusable parallel databases

Declare database use with `TestApp::with_parallel_database()` before `run_parallel_test`. In parallel children, the default connection switches to `<original_database>_test_<token>`; an existing database is reused and is kept by default. A missing database is created, then the two database hooks run once. SQLite `:memory:` is left unchanged. Existing owned MySQL fixtures already have their own databases and cannot combine the two ownership modes.

```sh
./artisan test --parallel --recreate-databases --drop-databases --test feature
./artisan test --parallel --without-databases --test feature
```

`--recreate-databases` drops the token database during process setup; `--drop-databases` drops it during normal process teardown. `--without-databases` disables switching, creation and both drop options. The original database is preserved by these operations. Connection URLs, read/write candidates and PostgreSQL direct endpoints all follow the selected token database; credentials and other connection options are retained.

The marker declares database use; it does not automatically select a migration or reset strategy. Run the required migrations through `test.artisan(["migrate", "--force"]).await.assert_successful()` inside `run_parallel_test`. Explicit migration and repeated migration are supported on SQLite, MySQL and PostgreSQL. Automatic migration/transaction/reset wiring for every database-testing strategy remains open; do not assume the owned-MySQL reset helpers accept reusable databases.

## Test environment {#test-environment}

Artisan sets `APP_ENV=testing` for the Cargo child process. When the application boots with that setting, it selects `.env.testing` if present and otherwise reads `.env`. The bootstrap supplies `array` for `CACHE_STORE` and `SESSION_DRIVER` when neither the process environment nor the selected file defines them. Keep test database settings in the test environment and clear any application configuration cache with `./artisan config:clear` before changing its settings.

## Creating a test application

Use `TestApp` to create an application with its own temporary root and service container. Create local fixtures under `test.root()` before boot. The callback receives the normal application builder and can register the providers, routes and configuration required by the test.

```rust
use openrust::{contracts::foundation::Application as _, testing::TestApp};

#[tokio::test]
async fn boots_a_test_application() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| builder)?;
    std::fs::write(test.root().join("fixture.txt"), "test data")?;
    test.boot().await?;
    assert_eq!(test.app().config()?.get("app.env"), Some("testing".into()));
    Ok(())
}
```

`boot` runs the application bootstrap without binding a port. The default configuration uses the `testing` environment. A custom configuration loader supplies its own settings. Each fixture removes its temporary directory when dropped. The helper leaves the process environment and working directory alone.

Write `.env.testing` under `test.root()` before the first `boot`, HTTP request or Artisan command. A configuration loader can read the fixture’s values from its environment argument. The fixture gives the loader `APP_ENV=testing` and ignores inherited `APP_CONFIG_CACHE`, `APP_PACKAGES_CACHE`, `APP_ROUTES_CACHE` and `APP_EVENTS_CACHE` paths. To use a custom route cache in a fixture, set `APP_ROUTES_CACHE` in its `.env.testing`.

## Creating tests {#creating-tests}

Artisan generates feature tests in `tests/feature/` and unit tests in `tests/unit/`. A generated feature test boots a temporary application and checks its health route. A generated unit test uses Rust’s standard test harness.

```sh
./artisan make:test HealthTest
./artisan make:test GreetingTest --unit
```

Declare `mod health_test;` in `tests/feature/main.rs` and `mod greeting_test;` in `tests/unit/main.rs` so Cargo can discover the files. For nested names, declare each parent module too. Then run the selected tests.

```sh
cargo +1.94.0 test --locked --test feature test_health_test
cargo +1.94.0 test --locked --test unit test_greeting_test
```

Use `-f` or `--force` to replace a generated file. To customize the templates, add `stubs/test.rs.stub` or `stubs/test.unit.rs.stub` at the application root. The generator substitutes `{{test}}` with the Rust function name.

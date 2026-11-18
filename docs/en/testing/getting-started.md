# Getting Started With Testing

## Running tests {#running-tests}

Place integration tests under the application's `tests/` directory or in the relevant crate. Run the workspace tests with the committed lockfile.

Run the application’s tests with Artisan or run every workspace member with Cargo. From Artisan, use Cargo options before the test filter and use `--` after the filter to pass options to Rust’s test harness. An initial `--` immediately after `test` forwards options that overlap with Artisan, such as Cargo’s `--help`.

```sh
./artisan test --locked --test feature artisan_inspire_prints_an_attributed_quote -- --exact
cargo +1.94.0 test --workspace --locked
```

Artisan runs Cargo from the application root, streams its output and returns its exit status. It uses the `CARGO` executable from the environment when set, otherwise `cargo` from the executable search path.

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

Write `.env.testing` under `test.root()` before the first `boot`, HTTP request or Artisan command. A configuration loader can read the fixture’s values from its environment argument. The fixture gives the loader `APP_ENV=testing` and ignores inherited `APP_CONFIG_CACHE`, `APP_PACKAGES_CACHE` and `APP_ROUTES_CACHE` paths. To use a custom route cache in a fixture, set `APP_ROUTES_CACHE` in its `.env.testing`.

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

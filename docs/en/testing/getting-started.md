# Getting Started With Testing

## Running tests

Place integration tests under the application's `tests/` directory or in the relevant crate. Run the workspace tests with the committed lockfile.

```sh
cargo +1.94.0 test --workspace --locked
```

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

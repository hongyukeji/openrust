# Console Tests

## Running commands

Run an Artisan command with `TestApp::artisan`, passing arguments after the `artisan` program name. It runs the normal console kernel and returns its exit code, stdout, and stderr.

```rust
use openrust::testing::TestApp;

#[tokio::test]
async fn reports_command_success_and_failure() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| builder)?;
    let help = test.artisan(["help", "env"]).await;
    help.assertSuccessful();
    assert!(help.stdout.contains("env"));
    assert!(help.stderr.is_empty());

    let missing = test.artisan(["example:missing"]).await;
    missing.assertExitCode(1);
    assert!(missing.stdout.is_empty());
    assert!(missing.stderr.contains("example:missing"));
    Ok(())
}
```

## Inspecting command output

Use `assertSuccessful` for exit code zero, `assertExitCode` for a specific code, and `expectsOutput` for an exact stdout line. Use a subprocess to inspect output written directly to the process streams by a command.

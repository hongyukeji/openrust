# Mocking

## Replacing a container instance {#mocking-objects}

Implement the application service trait in your test, then bind the substitute with `TestApp::instance`. This boots the fixture and replaces the binding for later resolutions in that application. Install the substitute before the code under test resolves the service. Other fixtures retain their own bindings.

```rust
use std::sync::Arc;
use openrust::testing::TestApp;

#[tokio::test]
async fn replaces_a_service() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    app.instance(Arc::new(String::from("test value"))).await?;

    assert_eq!(app.app().make::<String>().await?.as_str(), "test value");
    Ok(())
}
```

For event-specific assertions, see [Testing events](../digging-deeper/events.md#testing).

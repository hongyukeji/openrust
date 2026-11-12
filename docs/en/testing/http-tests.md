# HTTP Tests

## Dispatching requests

Use [TestApp](getting-started.md) to configure the application and dispatch requests through the HTTP kernel in the test process. Use `get` for an empty GET or `call` with an HTTP request containing the method, headers, and body.

```rust
use openrust::{foundation::configuration::Routing, testing::TestApp};

#[tokio::test]
async fn health_route_responds() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| {
        builder.withRouting(Routing::default().health("/up"))
    })?;
    test.get("/up").await?.assertOk();
    Ok(())
}
```

## Inspecting responses

The returned `TestResponse` exposes status, headers, and body. Chain `assertStatus`, `assertHeader`, `assertContent`, or `assertExactJson` to check them. Body capture has a 1 MiB limit and a five-second timeout. Use a running listener and an HTTP client to test transport behavior such as connection metadata.

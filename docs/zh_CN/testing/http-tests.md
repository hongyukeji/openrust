# HTTP 测试

## 分发请求

使用 [TestApp](getting-started.md) 配置应用，并在测试进程中经 HTTP 内核分发请求。空 GET 用 `get`，需要指定方法、请求头和正文时，将 HTTP 请求传给 `call`。

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

## 检查响应

`TestResponse` 提供状态、响应头和正文。用 `assertStatus`、`assertHeader`、`assertContent` 或 `assertExactJson` 断言。正文捕获限制为 1 MiB，超时为五秒。验证连接元数据等传输行为时，启动监听器并使用 HTTP 客户端。

# HTTP 客户端

## 发送请求 {#making-requests}

应用处理器可以使用 `openrust::support::facades` 中的 `Http`。`Http::get(url).await?` 返回带状态码的响应。用 `status`、`successful` 或 `failed` 检查状态，`throw()` 将 4xx 或 5xx 转为携带响应的错误。`json::<T>()` 用于类型化解码，`body()` 返回缓冲的字节。

用 `Http::acceptJson()?` 或 `Http::pendingRequest()?` 创建可配置请求。POST、PUT、PATCH 和 DELETE 数据按引用传入，默认编码为 JSON。用 `asForm`、`withBody` 或 `attach` 更换编码。`withQueryParameters` 设置编码后的查询参数，省略时保留 URL 内的查询串。超时使用 `Duration`，响应缓冲可用 `maxResponseBytes` 限制。默认连接设置下，同一个应用共享连接池。

## 重试与测试 {#retries-and-testing}

`retry` 接收总尝试次数。HTTP 失败默认将最后一份响应作为错误返回。`throwOnRetryFailure(false)` 直接返回该响应，传输失败仍返回错误。一次性请求正文需要为每次独立发送重新提供。`Factory::new` 可独立使用，并隔离 fake 和请求记录。用 `preventStrayRequests(true)` 让未匹配的测试请求在建立连接前失败。

```rust
use http::{HeaderMap, StatusCode};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::time::Duration;

let factory = Factory::new()?;
factory.fakeSequence("example.test/catalog")
    .push(Ok(Response::new(StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new(), Vec::new())))
    .push(Response::from_json(StatusCode::OK, &json!({"items": ["rust"]})));
factory.preventStrayRequests(true);

let response = factory.pendingRequest()
    .acceptJson()
    .retry(2, Duration::ZERO)
    .get("https://example.test/catalog")
    .await?
    .throw()?;
assert_eq!(response.json::<Value>()?["items"][0], "rust");
factory.assertSentCount(2);
```

用 `pool` 设置非零并发上限，通过 `Pool::r#as` 注册命名请求。结果保留注册顺序及每个请求自己的成功或错误。

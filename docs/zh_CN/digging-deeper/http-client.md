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

用 `pool` 设置非零并发上限，或用 `NonZeroUsize::MAX` 同时启动全部请求。结果按完成顺序排列。`poolOrdered` 同时启动全部请求，并按注册顺序返回每项结果。用 `r#as` 为请求命名，或用 `newRequest` 分配字符串形式的下一个数字键。

## 批次和事件 {#batches-and-events}

用 `Http::batch` 注册请求，再调用 `send().await`。用 `concurrency` 设置同时运行的请求上限。`before` 在首个请求前运行，`progress` 接收 2xx 响应，`catch` 接收 4xx、5xx 响应或单项错误。3xx 响应只增加已完成计数。没有失败时运行 `then`，`finally` 始终按注册顺序接收结果。

在待发送请求上调用 `throw()` 或 `throwIf`，可将匹配的最终 4xx、5xx 响应转为单项错误。`defer()?` 消费持有自有数据的批次，将它安排在当前 HTTP 请求、Artisan 命令或队列任务结束时执行。默认在任务成功后执行。要在任务失败后也执行，可对延后回调调用 `always()`。

运行下例时，将 `HTTP_CLIENT_EXAMPLE_ORIGIN` 设为本地服务地址，使 `GET /ok` 返回 200、`GET /missing` 返回 404。

```rust
use std::num::NonZeroUsize;
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let ok = format!("{origin}/ok");
let missing = format!("{origin}/missing");
let pool = Http::pool(NonZeroUsize::MIN, |pool| {
    pool.newRequest(|request| request.get(&ok))
        .r#as("missing", |request| request.get(&missing));
}).await?;
assert!(pool["0"].as_ref().is_ok_and(|response| response.ok()));
assert!(pool["missing"].as_ref().is_ok_and(|response| response.notFound()));

let batch = Http::batch(|batch| {
    batch
        .r#as("ok", |request| request.get(&ok))
        .r#as("missing", |request| request.throw().get(&missing))
        .finally(|counts, results| {
            assert_eq!(counts.failed_requests, 1);
            assert_eq!(results.keys().map(String::as_str).collect::<Vec<_>>(), ["ok", "missing"]);
        });
})?;
let results = batch.send().await;
assert!(results["missing"].is_err());
```

从 `openrust::http::client::events` 导入 `RequestSending`、`ResponseReceived` 和 `ConnectionFailed` 注册监听。事件名使用 `OpenRust\Http\Client\Events\` 前缀。应用 HTTP 客户端在每次请求尝试时派发事件，包括重试。

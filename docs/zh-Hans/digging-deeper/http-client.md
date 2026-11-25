# HTTP 客户端

## 发送请求 {#making-requests}

应用处理器可以使用 `openrust::support::facades` 中的 `Http`。`Http::get(url).await?` 返回带状态码的响应。用 `status`、`successful` 或 `failed` 检查状态，`throw()` 将 4xx 或 5xx 转为携带响应的错误。`json::<T>()` 用于类型化解码，`body()?` 按需返回正文 `Bytes`；文件读取失败返回 `Error::ResponseBody`。

用 `Http::accept_json()?` 或 `Http::pending_request()?` 创建可配置请求。POST、PUT、PATCH 和 DELETE 数据按引用传入，默认编码为 JSON。用 `as_form`、`with_body` 或 `attach` 更换编码。`with_query_parameters` 设置编码后的查询参数，省略时保留 URL 内的查询串。超时使用 `Duration`，响应缓冲可用 `max_response_bytes` 限制。默认连接设置下，同一个应用共享连接池。

使用 `pin_destination(url, addresses)?`，可将请求固定到已验证地址及指定协议、主机和端口。解析只使用提供的地址，Host 和 TLS 仍使用 URL 中的主机名。HTTP 传输层拒绝中间件改换来源或覆盖 Host。固定请求禁用重定向和 Unix socket。调用前由应用按网络策略校验地址。

请求和响应回调使用 Rust 类型。通过请求构建器明确选择传输设置，可用 Rust 函数或扩展 trait 复用具名配置。

### Digest 认证 {#digest-authentication}

使用 `with_basic_auth` 配置 Basic 凭据，使用 `with_token` 配置 Bearer 令牌。使用 `with_digest_auth` 响应原始源站的 Digest 挑战。Digest 会重放已缓冲的请求，服务端报告 nonce 过期时可再尝试一次。只有最终响应进入 `after_response`。下例使用本地 `GET /digest`，该端点使用示例凭据并返回正文 `authenticated`。

```rust
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let response = Http::with_digest_auth("example-user", "example-password")?
    .get(&format!("{origin}/digest"))
    .await?
    .throw()?;
assert_eq!(response.body()?.as_ref(), b"authenticated");
```

## 重试与测试 {#retries-and-testing}

`retry` 接收总尝试次数。HTTP 失败默认将最后一份响应作为错误返回。`throw_on_retry_failure(false)` 直接返回该响应，传输失败仍返回错误。一次性请求正文需要为每次独立发送重新提供。`Factory::new` 可独立使用，并隔离 fake 和请求记录。用 `prevent_stray_requests(true)` 让未匹配的测试请求在建立连接前失败。

```rust
use http::{HeaderMap, StatusCode};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::time::Duration;

let factory = Factory::new()?;
factory.fake_sequence("example.test/catalog")
    .push(Ok(Response::new(StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new(), Vec::new())))
    .push(Response::from_json(StatusCode::OK, &json!({"items": ["rust"]})));
factory.prevent_stray_requests(true);

let response = factory.pending_request()
    .accept_json()
    .retry(2, Duration::ZERO)
    .get("https://example.test/catalog")
    .await?
    .throw()?;
assert_eq!(response.json::<Value>()?["items"][0], "rust");
factory.assert_sent_count(2);
```

用 `pool` 设置非零并发上限，或用 `NonZeroUsize::MAX` 同时启动全部请求。结果按完成顺序排列。`pool_ordered` 同时启动全部请求，并按注册顺序返回每项结果。用 `r#as` 为请求命名，或用 `new_request` 分配字符串形式的下一个数字键。

### URI 模板、重试回调和 URL 放行 {#uri-templates-retry-and-allowance}

用 `with_url_parameters` 在请求发送前展开 RFC 6570 表达式。JSON 映射可传入字符串、数字、布尔值、列表或关联对象。空值会省略相应变量。查询对象使用展开修饰符时，嵌套字段采用方括号键。前缀长度按 Unicode 字符计数。重复调用时，同名参数以后一次为准。发送包含字面花括号的 URL 时，可以省略模板参数。

调用 `retry_when_request` 检查失败并修改待重放的请求，例如在 401 后替换授权标头。返回 `Ok(true)` 继续重试、`Ok(false)` 停止，或返回错误以中止。修改会保留到后续尝试。

启用 `prevent_stray_requests(true)` 后，可用 `allow_stray_requests` 替换允许运行真实请求的 URL 模式。匹配的 fake 优先，重定向的每一跳都会检查。

将 `HTTP_CLIENT_EXAMPLE_ORIGIN` 设为本地服务。该服务对示例 URL 返回 401，收到 `x-replay: ready` 标头后返回正文 `retried`。

```rust
use openrust::{http::client::Error, support::facades::Http};
use serde_json::json;
use std::time::Duration;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
Http::prevent_stray_requests(true)?;
Http::allow_stray_requests([format!("{origin}/retry/*")])?;

let parameters = [
    ("origin".to_owned(), json!(origin)),
    ("item".to_owned(), json!("item/id")),
    ("tag".to_owned(), json!("x y")),
].into_iter().collect();

let response = Http::with_url_parameters(parameters)?
    .retry(2, Duration::ZERO)
    .retry_when_request(|error, next| {
        if matches!(error, Error::RequestFailed(response) if response.unauthorized()) {
            next.headers_mut().insert("x-replay", "ready".parse().expect("valid header"));
            return Ok(true);
        }
        Ok(false)
    })
    .get("{+origin}/retry/{item}{?tag}")
    .await?;
assert_eq!(response.body()?.as_ref(), b"retried");
assert!(matches!(Http::get(&format!("{origin}/blocked")).await, Err(Error::StrayRequest)));
```

## 批次和事件 {#batches-and-events}

用 `Http::batch` 注册请求，再调用 `send().await`。用 `concurrency` 设置同时运行的请求上限。`before` 在首个请求前运行，`progress` 接收 2xx 响应，`catch` 接收 4xx、5xx 响应或单项错误。3xx 响应只增加已完成计数。没有失败时运行 `then`，`finally` 始终按注册顺序接收结果。

在待发送请求上调用 `throw()` 或 `throw_if`，可将匹配的最终 4xx、5xx 响应转为单项错误。`defer()?` 消费持有自有数据的批次，将它安排在当前 HTTP 请求、Artisan 命令或队列任务结束时执行。默认在任务成功后执行。要在任务失败后也执行，可对延后回调调用 `always()`。

运行下例时，将 `HTTP_CLIENT_EXAMPLE_ORIGIN` 设为本地服务地址，使 `GET /ok` 返回 200、`GET /missing` 返回 404。

```rust
use std::num::NonZeroUsize;
use openrust::support::facades::Http;

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let ok = format!("{origin}/ok");
let missing = format!("{origin}/missing");
let pool = Http::pool(NonZeroUsize::MIN, |pool| {
    pool.new_request(|request| request.get(&ok))
        .r#as("missing", |request| request.get(&missing));
}).await?;
assert!(pool["0"].as_ref().is_ok_and(|response| response.ok()));
assert!(pool["missing"].as_ref().is_ok_and(|response| response.not_found()));

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

### 出站请求事件 {#outgoing-request-events}

从 `openrust::http::client::events` 导入 `RequestSending`、`ResponseReceived` 和 `ConnectionFailed` 注册监听。应用工厂使用所属应用的派发器。独立工厂可通过 `with_dispatcher` 指定派发器。用 `with_attributes` 附加本地元数据，监听器可从请求快照读取，这些数据留在客户端。

每次向上游发送请求前，`RequestSending` 先于单项请求的 `before_sending` 回调派发。收到最终响应时，`ResponseReceived` 在 `after_response` 之前派发，4xx 和 5xx 也会触发。`ConnectionFailed` 携带实际尝试发送的请求以及类型化的连接或超时错误。重试会重复这一事件序列。被测试拦截的意外请求仅派发 `RequestSending`。

运行下例时，在本地服务加入 `GET /redirect`，使它以 302 重定向到 `/missing`。将 `HTTP_CLIENT_EXAMPLE_FAILURE` 设为拒绝连接的本地端点 URL。

```rust
use openrust::http::client::{Error, events::{ConnectionFailed, RequestSending, ResponseReceived}};
use openrust::support::facades::{Event, Http};

let origin = std::env::var("HTTP_CLIENT_EXAMPLE_ORIGIN")?;
let unavailable = std::env::var("HTTP_CLIENT_EXAMPLE_FAILURE")?;
let sending = r"OpenRust\Http\Client\Events\RequestSending";
let received = r"OpenRust\Http\Client\Events\ResponseReceived";
let failed = r"OpenRust\Http\Client\Events\ConnectionFailed";
let events = Event::fake(&[sending, received, failed])?;

assert!(Http::get(&format!("{origin}/redirect")).await?.not_found());
assert!(matches!(Http::get(&unavailable).await, Err(Error::Connection)));
let outgoing = events.dispatched(sending);
assert_eq!(outgoing.len(), 3);
assert_eq!(outgoing[1].downcast_ref::<RequestSending>().unwrap().request.url.path(), "/missing");
let replies = events.dispatched(received);
assert_eq!(replies.len(), 1);
assert!(replies[0].downcast_ref::<ResponseReceived>().unwrap().response.not_found());
let failures = events.dispatched(failed);
assert_eq!(failures.len(), 1);
assert_eq!(failures[0].downcast_ref::<ConnectionFailed>().unwrap().request.url.as_str(), unavailable);
```

## 全局中间件与传输耗时 {#global-middleware-and-timing}

在应用 Provider 中使用 `Http::global_middleware` 包裹每一跳出站请求；独立客户端可用 `Factory::global_middleware`。回调接收拥有所有权的请求和续体 `next`，返回 `Box::pin(async move { ... })`；`next.run(request).await` 返回响应或包括超时在内的传输错误。中间件按注册顺序进入、反序返回，重定向和重试都会再次经过。注册作用于随后创建的 PendingRequest，同一 Factory 的克隆共享注册，应用之间隔离。

`global_request_middleware` 通过 `&mut Request` 修改请求并返回 `Result<()>`；`global_response_middleware` 将 `Response` 映射为 `Result<Response>`。单个 PendingRequest 使用 `with_middleware`、`with_request_middleware`、`with_response_middleware`；现有 `before_sending` 与 `after_response` 仍可使用。

```rust
use openrust::http::client::Http;

Http::global_middleware(|request, next| Box::pin(async move {
    let response = next.run(request).await?;
    let seconds = response.handler_stats().get("total_time").copied();
    // Use `seconds` with the application's metrics collector.
    Ok(response)
}))?;
```

`Response::handler_stats()` 的 `total_time` 以秒记录最后一次网络传输及缓冲正文读取的耗时，不包含重试间隔，也不是全部重定向或尝试的总时长。`response.transfer_stats.as_ref().map(|stats| stats.get_transfer_time())` 提供相同耗时。fake 响应的传输时间为0且没有网络 handler stats；未调用传输的中间件自建响应不具有传输统计。reqwest 不提供 cURL 分阶段统计，因此不伪造 DNS、连接和 TLS 耗时。脱敏响应事件保留计时，不额外暴露正文或凭据。

## 请求选项

`replace_headers` 仅替换传入的同名请求头，保留其他头和多值。`with_header`、`content_type`、`as_multipart`、`max_redirects` 沿用原请求构建链。

客户端跟随带有效 HTTP(S) `Location` 的 3xx 响应。300–303 会清除正文，保留 GET、HEAD 与 OPTIONS，其余方法改为 GET。304–399 保留请求方法，并要求正文可重放。每次跳转都会清除上一次请求的 `Referer` 头。

`sink(path)` 在网络传输中逐块写入文件，响应保留文件句柄，不常驻完整正文；也可将 `std::fs::File` 移交给 `sink(file)`。PendingRequest、Factory 和 Http Facade 均提供入口。路径首次写入时创建或截断；文件资源从当前游标写入，不截断，传输完成后回到开头。成功的空响应也会创建或清空路径文件。每次重试和重定向重新处理目标路径；未收到正文便连接失败时保留旧文件。

`response.resource()?` 返回实现 `Read`、`Write` 和 `Seek` 的资源，与响应克隆共享正文及游标；Rust 资源直接操作共享游标，不模拟 PHP 包装资源的预读缓冲。`body()?` 每次从开头读取完整正文，`json::<T>()?` 同样需要完整正文；调用这两个方法或启用会读取正文的观察器（例如 Telescope）仍会分配正文内存。`close()?` 关闭共享正文，后续读取、写入和定位返回错误。

下载仍受 `max_response_bytes` 及超时限制。写入失败返回 `Error::Sink`；断连、超时、取消或超限保留已经写入的部分文件，不保证原子替换，也不自动删除。fake 请求把原有缓冲正文写入目标，响应继续保留 fake 正文。

`with_user_agent("OpenRust/1.0")` 设置 `User-Agent`，覆盖已有的同名头，并移除首尾 ASCII 空格、制表、换行、回车、NUL 和垂直制表符；Unicode 空格保留。参数为 Rust 字符串，非法头值会在发出请求前返回 `InvalidRequest`。PendingRequest、Factory 和 Http Facade 都提供这个入口。

```rust
let response = Http::with_user_agent("OpenRust/1.0")?.get(url).await?;
let response = Http::without_verifying()?.get(test_https_url).await?;
```

TLS 校验默认开启。`without_verifying()` 仅为当前构建的请求关闭证书链与主机名校验，不更改共享 Factory 或之后的默认请求；它也可与 `connect_timeout` 组合。该入口用于显式接受未受信任证书的受控端点。当前传输未集成 NTLM 认证握手，尚未提供 `withNtlmAuth`；它不能由 Basic、Digest 或直接拼接 Authorization 头替代。

## Multipart 字段值

`as_multipart().post(data)` 接受由标量字段组成的对象。布尔值按 PHP 方式编码为 `1` 或空字符串，null 编码为空字符串。嵌套对象或数组返回 `InvalidRequest`；文件使用 `attach`。该入口不接收 Guzzle 的全部 multipart 选项。

所有部件均已缓冲的 multipart 请求，在 307、308 跳转和显式配置的重试中沿用原正文与 boundary。配置重试后，含一次性流部件的请求返回 `UnreplayableRequest`。

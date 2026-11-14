# 错误处理

## 报告异常

在 `bootstrap/app.rs` 的 `withExceptions` 闭包中配置报告。使用 `report::<E>` 匹配异常类型或其来源。回调按注册顺序执行，`ReportDecision::Stop` 停止后续回调和默认报告。

通过同一 `withExceptions` 回调配置[队列任务按异常停止重试](../digging-deeper/queues.md#stopping-retries-by-exception)。

默认以 `Level::Error` 将可报告的异常写入应用配置的日志渠道。HTTP 报告记录状态码，默认消息省略来源详情。Artisan 保留 stderr 输出和退出状态。用 `level::<E>` 设定级别，用 `context` 向记录附加字段。公开的 [Context](../digging-deeper/context.md) 字段作为独立元数据写入。

### 忽略异常

默认忽略 HTTP 400、403、404 和 405。用 `dontReport::<E>` 忽略匹配类型，`dontReportWhen` 按条件忽略，`stopIgnoring::<HttpError>` 恢复这些 HTTP 错误的报告。忽略规则只影响报告，响应仍按原流程渲染。

### 异常实例去重

启用 `dontReportDuplicates` 后，用 `SharedException` 包装需要共享身份的异常。克隆保留同一身份。可以显式调用 `Application::report`，也可以将共享异常作为 `HttpError::Internal` 的来源返回。消息相同的独立异常仍可分别报告。

```rust
use openrust::{
    contracts::debug::SharedException,
    foundation::Application,
    log::Level,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withExceptions(|exceptions| {
            exceptions
                .level::<std::io::Error>(Level::Warning)
                .dontReportDuplicates();
        })
        .create()?;
    app.boot().await?;
    let error = SharedException::new(std::io::Error::other("Job failed"));
    app.report(&error).await?;
    app.report(&error.clone()).await?;
    Ok(())
}
```

## 限流异常报告

在 `withExceptions` 中通过 `throttle::<E>` 注册策略。第一个匹配且返回策略的回调生效。`ReportThrottle::Lottery(Lottery::odds(1, 100)?)` 用于采样，`ReportThrottle::Limit` 用于固定窗口配额，`ReportThrottle::Unlimited` 允许直接报告。

把下面的 `configure` 传给 `withExceptions`，每分钟最多报告两次 I/O 异常。

```rust
use openrust::{
    cache::rate_limiting::Limit,
    foundation::configuration::{Exceptions, ReportThrottle},
};

fn configure(exceptions: &mut Exceptions) {
    exceptions.throttle::<std::io::Error>(|_| {
        Some(ReportThrottle::Limit(Limit::perMinute(2).by("transport")))
    });
}
```

省略 `by` 时按注册的 Rust 异常类型计数。使用同一显式键的策略在配置的限流缓存中共享配额。缓存操作失败时继续报告。限流在报告回调和写日志前执行，HTTP 状态与正文、Artisan 的标准错误和退出码仍按原有流程处理。

`Application::report` 是异步方法，显式报告与 HTTP、Artisan 共用基于缓存的限流策略。

## 渲染异常

用 `render::<E>` 根据错误和请求返回 HTTP 响应。返回 `None` 时继续尝试后续回调，最终回退到默认 JSON 或 HTML 渲染。用 `respond` 调整最终异常响应，自定义和默认响应都会经过它。这两个回调只用于 HTTP。

### 自定义异常响应

可为默认 HTML 响应添加 UTF-8 页面，例如 `resources/views/errors/404.html` 或 `resources/views/errors/500.html`。框架按已配置的视图路径查找。其他错误状态可先回退到 `4xx.html` 或 `5xx.html`，再使用内置页面。JSON 响应有自己的渲染流程。

```rust
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, http::Kernel},
    http::{Body, HttpError, Request, Response, StatusCode},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Application::configure(std::env::current_dir()?)
        .withExceptions(|exceptions| {
            exceptions
                .render::<HttpError>(|error, request| {
                    if matches!(error, HttpError::NotFound) && request.uri().path() == "/example" {
                        Some(Response::new(StatusCode::NOT_FOUND, Body::from("Example not found")))
                    } else {
                        None
                    }
                })
                .respond(|mut response, _, _| {
                    response.parts_mut().headers.insert("x-error", "handled".parse().unwrap());
                    response
                });
        })
        .create()?;

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await?;
    let (parts, ()) = http::Request::get("/example").body(())?.into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["x-error"], "handled");
    Ok(())
}
```

## 将异常渲染为 JSON

框架默认根据请求对 JSON 的偏好选择响应。应用骨架在 `withExceptions` 中为 最终生效的 API 路径 和期望 JSON 的请求启用 JSON 异常响应。可用 `shouldRenderJsonWhen` 自定义策略。

生产环境设为 `APP_DEBUG=false`。通用 500 的默认 JSON 消息为 `Server Error`，默认 HTML 页面省略异常来源。调试模式下，内置服务端错误响应包含有数量及长度上限的来源消息，输出到 HTML 时会转义。自定义回调负责控制所返回的响应内容。

例如，访问不存在的 API 路由会返回 `404` 和包含 `message` 字段的 JSON。访问不存在的 Web 路由会返回 HTML `404`，如果请求的 `Accept` 优先选择 JSON 媒体类型，则返回 JSON。

## 路由不存在

应用的 `withExceptions` 策略对最终生效的 API 路径或期望 JSON 的请求返回 JSON，其他缺失路由返回 HTML。响应格式改变，404 状态保持。缺失静态文件也使用此策略；HEAD 保留状态和内容类型，正文为空。

路径规则匹配生效前缀下的路径，如前缀 `/v1/internal` 下的 `/v1/internal/users`。前缀为空时，应用骨架根据请求对 JSON 的偏好选择异常响应。

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```

## HTTP 异常 {#http-exceptions}

在路由处理器或中间件中返回 `Err(abort(status, message, headers))`，即可中止处理并渲染 HTTP 异常。异常处理器保留状态码和响应头，并按请求选择 HTML 或 JSON。消息应适合直接返回客户端。意外的内部错误仍遵循生产环境的脱敏策略。

```rust
use openrust::http::{HeaderMap, HttpError, Response, StatusCode, abort};

async fn maintenance() -> Result<Response, HttpError> {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", "60".parse().unwrap());
    Err(abort(StatusCode::SERVICE_UNAVAILABLE, "Maintenance", headers))
}
```

## 自定义 HTTP 错误页

添加按状态码命名的视图，例如 `resources/views/errors/404.blade.html`。Blade 错误页可以读取 `$status` 和 `$message`，也支持普通的 `errors/404.html`。可以用 `errors/4xx.blade.html` 或 `errors/5xx.blade.html` 处理同类错误。404、500 和 503 使用对应的专属视图或框架回退页。响应沿用原有状态码和 JSON 协商规则。

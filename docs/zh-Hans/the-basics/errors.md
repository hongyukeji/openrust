# 错误处理

## 报告异常

在 `bootstrap/app.rs` 的 `withExceptions` 闭包中配置报告。使用 `report::<E>` 匹配异常类型或其来源。回调按注册顺序执行，`ReportDecision::Stop` 停止后续回调和默认报告。

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
    app.report(&error)?;
    app.report(&error.clone())?;
    Ok(())
}
```

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

框架默认根据请求对 JSON 的偏好选择响应。应用骨架在 `withExceptions` 中为 `api/*` 和期望 JSON 的请求启用 JSON 异常响应。可用 `shouldRenderJsonWhen` 自定义策略。

生产环境设为 `APP_DEBUG=false`。通用 500 的默认 JSON 消息为 `Server Error`，默认 HTML 页面省略异常来源。调试模式下，内置服务端错误响应包含有数量及长度上限的来源消息，输出到 HTML 时会转义。自定义回调负责控制所返回的响应内容。

例如，访问不存在的 API 路由会返回 `404` 和包含 `message` 字段的 JSON。访问不存在的 Web 路由会返回 HTML `404`，如果请求的 `Accept` 优先选择 JSON 媒体类型，则返回 JSON。

## 路由不存在

应用的 `withExceptions` 策略对匹配 `api/*` 的路径或期望 JSON 的请求返回 JSON，其他缺失路由返回 HTML。响应格式改变，404 状态保持。缺失静态文件也使用此策略；HEAD 保留状态和内容类型，正文为空。

```sh
curl -i http://localhost:8000/missing
curl -i http://localhost:8000/api/missing
curl -i -H 'Accept: application/json' http://localhost:8000/missing
```

# 错误处理

## 传播领域错误

对无须额外上下文的固定错误转换，使用 `?`。例如在返回 `HttpError` 的控制器或中间件中，可直接传播配置读取错误：

```rust
use openrust::{http::HttpError, support::facades::Config};

fn locale_form_field() -> Result<String, HttpError> {
    Ok(Config::string("app.locale_form_field")?)
}
```

配置、容器、翻译、视图、数据库、日志、Redis、Hash、JSON 序列化、I/O 与异步任务调度错误进入 HTTP 边界后保留原始来源，默认归入 `Internal`。响应状态为 500，由现有异常处理流程负责报告、调试展示和生产环境脱敏。数据库错误的默认转换不会将 `ModelNotFound` 改为 404；返回模型结果的路由仍按自己的既有规则处理。

图片领域错误以及 `openrust-horizon`、`openrust-pulse`、`openrust-pennant` 的错误同样默认转换为 `Internal`，保留领域来源，不自动展开内嵌错误改变状态码。

底层模块继续返回自身领域错误。容器解析失败可直接传播到配置、翻译、视图、数据库、缓存、队列、事件、日志等对应的领域结果；认证中的数据库、缓存、Hash、Session、配置和容器错误保留为 `AuthError::Backend`。不要为了使用 `?` 让通用组件依赖 HTTP。

`?` 只转换当前返回边界的错误，不会自动逐层调用多个 `From`。返回表达式需要时使用 `Ok(operation()?)`，或在返回类型明确时使用 `map_err(Into::into)`。闭包有多个可能的错误类型时，显式声明其 `Result` 类型。

保留需要路径、操作名、请求或阶段上下文的 `map_err`，以及状态码分类、恢复、重试和脱敏逻辑。文件系统、校验、集合调试与认证已有专用 HTTP 转换；强制包装为 `Internal` 的调用不能在未核对语义时直接改成 `?`。

## 报告异常

在 `bootstrap/app.rs` 的 `with_exceptions` 闭包中配置报告。使用 `report::<E>` 匹配异常类型或其来源。回调按注册顺序执行，`ReportDecision::Stop` 停止后续回调和默认报告。

通过同一 `with_exceptions` 回调配置[队列任务按异常停止重试](../digging-deeper/queues.md#stopping-retries-by-exception)。

默认以 `Level::Error` 将可报告的异常写入应用配置的日志渠道。HTTP 报告记录状态码，默认消息省略来源详情。Artisan 保留 stderr 输出和退出状态。用 `level::<E>` 设定级别，用 `context` 向记录附加字段。公开的 [Context](../digging-deeper/context.md) 字段作为独立元数据写入。

### 忽略异常

默认忽略 HTTP 400、403、404 和 405。用 `dont_report::<E>` 忽略匹配类型，`dont_report_when` 按条件忽略，`stop_ignoring::<HttpError>` 恢复这些 HTTP 错误的报告。忽略规则只影响报告，响应仍按原流程渲染。

### 异常实例去重

启用 `dont_report_duplicates` 后，用 `SharedException` 包装需要共享身份的异常。克隆保留同一身份。可以显式调用 `Application::report`，也可以将共享异常作为 `HttpError::Internal` 的来源返回。消息相同的独立异常仍可分别报告。

```rust
use openrust::{
    contracts::debug::SharedException,
    foundation::Application,
    log::Level,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .with_exceptions(|exceptions| {
            exceptions
                .level::<std::io::Error>(Level::Warning)
                .dont_report_duplicates();
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

在 `with_exceptions` 中通过 `throttle::<E>` 注册策略。第一个匹配且返回策略的回调生效。`ReportThrottle::Lottery(Lottery::odds(1, 100)?)` 用于采样，`ReportThrottle::Limit` 用于固定窗口配额，`ReportThrottle::Unlimited` 允许直接报告。

把下面的 `configure` 传给 `with_exceptions`，每分钟最多报告两次 I/O 异常。

```rust
use openrust::{
    cache::rate_limiting::Limit,
    foundation::configuration::{Exceptions, ReportThrottle},
};

fn configure(exceptions: &mut Exceptions) {
    exceptions.throttle::<std::io::Error>(|_| {
        Some(ReportThrottle::Limit(Limit::per_minute(2).by("transport")))
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
        .with_exceptions(|exceptions| {
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

框架默认根据请求对 JSON 的偏好选择响应。应用骨架在 `with_exceptions` 中为 最终生效的 API 路径 和期望 JSON 的请求启用 JSON 异常响应。可用 `should_render_json_when` 自定义策略。

生产环境设为 `APP_DEBUG=false`。通用 500 的默认 JSON 消息为 `Server Error`，默认 HTML 页面省略异常来源。调试模式下，内置服务端错误响应包含有数量及长度上限的来源消息，输出到 HTML 时会转义。自定义回调负责控制所返回的响应内容。

例如，访问不存在的 API 路由会返回 `404` 和包含 `message` 字段的 JSON。访问不存在的 Web 路由会返回 HTML `404`，如果请求的 `Accept` 优先选择 JSON 媒体类型，则返回 JSON。

## 路由不存在

应用的 `with_exceptions` 策略对最终生效的 API 路径或期望 JSON 的请求返回 JSON，其他缺失路由返回 HTML。响应格式改变，404 状态保持。缺失静态文件也使用此策略；HEAD 保留状态和内容类型，正文为空。

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

## Exceptions 门面 {#exceptions-facade}

`Exceptions` 解析当前应用的 `dyn ExceptionHandler` 绑定。HTTP 中间件错误边界、应用报告与 Artisan 失败都解析同一契约；请求执行期间替换处理器后再抛错，也使用新处理器。Worker 报告原本已解析此契约。绑定缺失时返回错误，不会悄悄复用最初的具体处理器。

```rust
use openrust::{support::facades::Exceptions, log::LogResult};

async fn report_failure() -> LogResult<()> {
    Exceptions::report(std::io::Error::other("报告失败")).await
}
```

`report` 接收拥有所有权的 Rust 错误；`should_report`、`should_stop_retries`、`render` 和 `render_for_console` 转发既有契约，`handler` 返回当前 Arc。`should_report` 只查询报告资格，不消耗采样或限流配额，实际报告时才执行这些策略。启动配置继续写在 `with_exceptions`。运行期现已提供 `reportable`、`renderable`、`dont_report`、`ignore`、`stop_ignoring`、`dont_report_when`、`dont_retry`、`dont_retry_when`、`dont_flash`、`level`、`build_context_using`、`respond_using`、`should_render_json_when`、`dont_report_duplicates` 和带类型的 `throttle_using`。`is_reporting` 查询同一异常实例是否正在被报告，不是报告资格查询，也不消耗报告限流配额。自定义处理器须显式支持运行期配置与此只读查询，否则返回错误。异常 fake／断言接口仍待补齐。

### 可报告回调句柄 {#reportable-handlers}

`reportable` 返回已注册的 `ReportableHandler`。回调返回 `ReportDecision::Stop` 时，本次报告不再执行后续回调或默认日志。调用 `.stop()` 也会设置此行为，注册完成后修改句柄仍生效；句柄及其克隆共享停止标志。需要匹配所有错误时使用 `reportable_all`，无需逐类型注册。

```rust
use openrust::{contracts::container::BindingResolutionError, foundation::configuration::ReportDecision, support::facades::Exceptions};

fn configure_reports() -> Result<(), BindingResolutionError> {
    Exceptions::reportable::<std::io::Error>(|_| ReportDecision::Continue)?.stop();
    Exceptions::reportable_all(|_| ReportDecision::Stop)?;
    Ok(())
}
```

`handles` 用于检查类型回调是否匹配某个 `Exception`；`invoke` 遇到不匹配类型会返回错误。停止标志不阻止当前回调本身执行，而是在它返回后阻止后续报告。已有启动配置 `exceptions.report(...)` 保留原来的链式配置返回值，需要句柄时使用启动配置 `exceptions.reportable(...)`。

# 错误处理

显式返回业务 JSON 错误的示例见[业务响应](./responses.md#business-responses)。

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

默认 HTML 响应可在 `resources/views/errors` 下添加 UTF-8 视图。异常数据、发布与回退顺序见[自定义错误页](#custom-http-error-pages)。JSON 响应使用独立渲染流程。

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

默认处理器将 API 路由及生效 API 前缀下的异常渲染为 [`ApiResult`](./responses.md#business-responses)。其他路由默认根据请求对 JSON 的偏好选择响应，可用 `should_render_json_when` 配置。

生产环境设为 `APP_DEBUG=false`。通用 500 的默认 JSON 消息为 `Server Error`，默认 HTML 页面省略异常来源。调试模式下，内置服务端错误响应包含有数量及长度上限的来源消息，输出到 HTML 时会转义。自定义回调负责控制所返回的响应内容。

例如，访问不存在的 API 路由会返回 `404` 和包含 `message` 字段的 JSON。访问不存在的 Web 路由会返回 HTML `404`，如果请求的 `Accept` 优先选择 JSON 媒体类型，则返回 JSON。

## 路由不存在

应用的 `with_exceptions` 策略对最终生效的 API 路径或期望 JSON 的请求返回 JSON，其他缺失路由返回 HTML。响应格式改变，404 状态保持。缺失静态文件也使用此策略；HEAD 保留状态和内容类型，正文为空。

路径规则匹配生效前缀下的路径，如前缀 `/v1/internal` 下的 `/v1/internal/users`。前缀为空时，已匹配的 API 路由仍返回 JSON 异常响应。未匹配的路径按请求对 JSON 的偏好选择响应。

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

## 自定义 HTTP 错误页 {#custom-http-error-pages}

添加 `resources/views/errors/404.blade.html` 定制对应状态页，也支持普通 `404.html`。Blade 接收真实异常实例 `$exception` 和独立的空 `$errors` 错误包，另保留 `$status`、`$message`。

```blade
<h2>{{ $exception->getMessage() }}</h2>
<code>{{ $exception->getStatusCode() }}</code>
```

对象还公开 `getHeaders()`、`getPrevious()`（最多 32 层来源）。Blade 转义输出仍会转义文本；也支持原生名称 `get_message`、`get_status_code`、`get_headers`、`get_previous`。HTTP 边界保留原错误的所有权供本次渲染使用，并发请求与取消不会共享异常。底层仅借用不透明错误的渲染调用无法提供拥有所有权的模板对象，请使用正常 HTTP 边界或 `SharedException`。

发布八份默认模板后即可修改：

```sh
./artisan vendor:publish --tag=laravel-errors
```

此既有外部标签为命令兼容保留。发布默认不覆盖已有文件，显式传入 `--force` 才覆盖。默认模板显示状态及通用标题，不输出内部异常消息。

### 通用 HTTP 错误页 {#fallback-http-error-pages}

先查应用中的精确状态页，不存在时由 `4xx.blade.html` 或 `5xx.blade.html` 接管同类状态。**401、402、403、404、419、429、500、503** 使用自己的内置专用页面，不进入通用回退；须分别创建精确页面覆盖。原状态码与响应头保持，JSON 协商绕过 HTML 模板。

错误模板自身失败时，视图异常先报告，再执行最终 `respond`。`APP_DEBUG=false` 保留原状态并回退内置响应；调试模式将视图异常作为新的 500 错误渲染。生产模板不要包含敏感异常详情。

## Exceptions 门面 {#exceptions-facade}

`Exceptions` 解析当前应用的 `dyn ExceptionHandler` 绑定。HTTP 中间件错误边界、应用报告与 Artisan 失败都解析同一契约；请求执行期间替换处理器后再抛错，也使用新处理器。Worker 报告原本已解析此契约。绑定缺失时返回错误，不会悄悄复用最初的具体处理器。

```rust
use openrust::{support::facades::Exceptions, log::LogResult};

async fn report_failure() -> LogResult<()> {
    Exceptions::report(std::io::Error::other("报告失败")).await
}
```

`report` 接收拥有所有权的 Rust 错误；`should_report`、`should_stop_retries`、`render` 和 `render_for_console` 转发既有契约，`handler` 返回原处理器 Arc，异常替身会被解包。`should_report` 检查重复／忽略规则，并执行采样或限流准入；重复查询也可能消耗配额。每次实际报告只执行一次准入，不要把它当作无副作用的状态探针。启动配置继续写在 `with_exceptions`。运行期现已提供 `reportable`、`renderable`、`dont_report`、`ignore`、`stop_ignoring`、`dont_report_when`、`dont_retry`、`dont_retry_when`、`dont_flash`、`level`、`build_context_using`、`respond_using`、`should_render_json_when`、`dont_report_duplicates` 和带类型的 `throttle_using`。`is_reporting` 查询同一异常实例是否正在被报告，不是报告资格查询，也不消耗报告限流配额。自定义处理器须显式支持运行期配置与此只读查询，否则返回错误。异常替身与断言见下文。

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

### 异常测试 {#exception-testing}

在当前应用门面作用域中调用 `Exceptions::fake()`，或用 `TestApp::fake_exceptions(&[])` 保存返回的句柄，跨 HTTP、Artisan 调用进行断言。空选择列表记录所有可报告异常。`ExceptionType::of::<E>()` 精确选择报告的根类型；`including_sources::<E>()` 才显式检查错误源链。类型断言仍匹配根类型，不把嵌套源错误算作另一条报告。

```rust
use openrust::{http::HttpError, testing::TestApp};

async fn check_reports(app: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let exceptions = app.fake_exceptions(&[]).await?;
    let _response = app.get("/failure").await?;
    exceptions.assert_reported::<HttpError>();
    exceptions.assert_reported_where::<HttpError>(|error| error.to_string().contains("failure"));
    exceptions.assert_reported_count(1);
    Ok(())
}
```

句柄和门面都提供 `assert_reported`、`assert_not_reported`、对应的 `_where` 条件断言、`assert_reported_count` 和 `assert_nothing_reported`。`reported()` 返回共享的原始错误；执行条件回调时不持有记录列表锁。HTTP 渲染、验证重定向和控制台渲染仍使用原处理器，选中并记录的报告不再重复写入其日志，未选中的错误正常委托。`handler()` 返回替身背后的处理器；重复安装替身使用新的记录列表，不嵌套旧替身。`set_handler` 更换委托处理器并保留既有记录。

`Exceptions::report(error)` 和 `Application::report_owned(error)` 接收错误所有权，HTTP、Artisan 和 Worker 边界也保留共享错误。自定义调用方若只有借用的非透明错误，须先持有 `SharedException` 再借用；替身无法安全保存任意短期借用，因此明确返回错误，不降级为字符串快照。不同应用及并发任务不会共享替身记录。

`throw_on_report()` 会在记录后中止受保护的测试调用。HTTP 调用通过自身的 `Result` 返回原始错误；`TestApp::try_artisan(args)` 返回 `Result<TestCommand, SharedException>`，也包括 `queue:work` 报告的任务错误。兼容的 `artisan(args)` 仍返回 `TestCommand`；显式启用重抛模式时，它以携带原始 `SharedException` 的 panic 重抛。需要可失败调用的 Rust 测试优先使用 `try_artisan`。

其他异步入口可用 `Exceptions::catch_reported(future).await` 包裹。每次捕获使用词法任务局部通道，报告后返回原始错误并取消本次调用，后续代码不会继续执行；嵌套捕获恢复外层作用域，并发请求与取消不会遗留通道。捕获作用域外直接 `report(...).await` 会返回含原始错误的 `Err`，不会挂起任务。`throw_first_reported()` 返回首条报告的错误，空列表则成功。此测试控制不会把生产日志失败改成 panic。

### 异常上下文 {#exception-context}

`Exceptions::register()` 转发实际处理器的注册钩子，标准处理器没有额外注册工作。自定义处理器可以覆写，异常替身会继续转发给原处理器。

`exception_context::<E>(callback)` 为具体 Rust 错误注册字段。错误自身有 `context()` 方法时，显式传入 `|error| error.context()`；`std::error::Error` 未定义这个可选方法。`build_context_using(callback)` 保留单参数便捷用法；`build_context_using_with_context(callback)` 同时接收异常和此前累积的字段，按注册顺序执行，后返回的同名字段覆盖前值。

`build_context_for_exception(&exception)` 返回异常专属字段。`context_for_exception(&exception).await?` 返回 `ExceptionContext`：`fields` 保存合并字段，`exception` 保留原始异常借用，不把错误对象伪装成 JSON 值。默认上下文在异常字段后合并当前默认 Guard 已解析的用户 ID，因此其 `userId` 优先；假值标识会被省略。HTTP 错误边界会在记录前惰性解析默认 Guard。认证失败保留原异常报告；认证内部再次报告不会递归解析同一应用的 Guard。嵌套应用及取消后的调用各自恢复上下文。

实际日志处理器使用同一个上下文构造器。`SharedException` 传输包装不会在 JSON 日志中多出一层原因，也不会让原错误的上下文回调执行两次；真实源错误仍保留在异常链中。其他应用或请求读取、报告上下文时不会复用此前已认证身份。

路由已经持有 `&mut Request` 时，使用 `context_for_exception_with_request(request, &exception).await?` 或 `report_for_request(request, error).await`；也可通过 `request.with_container(...)` 借出请求后使用环境门面。这样显式处理可变借用，惰性认证不依赖不安全的全局请求引用。无请求参数的上下文入口会尝试解析可访问的环境请求；HTTP 之外不附带请求身份。

自定义处理器若未实现 `register`、`build_context_for_exception` 或 `context_for_exception`，会明确返回不支持此能力的错误，经过替身也一样；只有已实现操作且确实没有字段时才返回空上下文。

### 异常映射 {#exception-mapping}

`Exceptions::map::<来源类型, 目标类型>(callback)` 注册类型转换，启动配置对应 `exceptions.map::<来源类型, 目标类型>(callback)`。只应用首个匹配映射；同一来源类型重新注册会覆盖回调并保留原顺序，结果不会递归映射。Rust 类型与构造回调替代动态类名字符串，来源／目标类型无效时由编译器拒绝。

对 Rust 枚举，可用 `map_optional::<E, 目标类型>(callback)` 选择变体：匹配时返回 `Some(mapped)`，不匹配时返回 `None`，保留原错误并继续检查后续映射。这样只修改某种状态时，不必把所有 `HttpError` 变体都转换。

```rust
use openrust::{contracts::container::BindingResolutionError, support::facades::Exceptions};

fn configure_mapping() -> Result<(), BindingResolutionError> {
    Exceptions::map::<std::num::ParseIntError, _>(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, error.clone())
    })?;
    Ok(())
}
```

实际报告在忽略规则、报告回调和日志之前应用映射。渲染单独应用映射，因此一次 HTTP 失败可能在报告、渲染阶段各调用一次。`should_report` 查询传入的异常，不先执行映射；选中的替身记录原始异常，渲染仍委托原处理器应用映射。

映射后的根类型决定报告、渲染和忽略规则；保留的源错误只作为诊断链，不再误触发旧类型的规则。映射为 `HttpError` 时保留其状态和响应处理。`should_render_json_when` 现在接收真实 `Exception`，包括映射后的业务异常；需要按类型判断时用 `downcast_ref::<E>()`，不会用合成 HTTP 错误代替业务错误。

包装错误具有可选内层异常时，可用 `inner_exception::<包装类型>(callback)` 返回 `Option<SharedException>`，显式接入 Rust `Error` 未定义的内层错误方法。存在内层错误时优先于普通映射，直接返回且不再次映射；`None` 继续匹配。克隆共享内层句柄可保留原始实例。

### 关闭异常处理 {#without-exception-handling}

使用 `without_exception_handling(&[])` 让 HTTP 测试调用返回未处理错误，`with_exception_handling()` 恢复最初的原处理器。非 Clone 错误仍保留真实的拥有所有权的实例。路由不存在时补充方法和不含查询参数的 URL，原始 `HttpError::NotFound` 留在来源链中。

```rust
use openrust::{contracts::debug::Exception, http::HttpError, testing::TestApp};

async fn check_unhandled(app: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.without_exception_handling(&[]).await?;
    let error = app.get("/missing").await.expect_err("expected an unhandled error");
    assert!(Exception::Error(error.as_ref()).downcast_chain_ref::<HttpError>().is_some());
    app.with_exception_handling().await?;
    Ok(())
}
```

`handle_exceptions(&types)` 保留所选类型的正常渲染，`handle_validation_exceptions()` 保留正常验证响应。`ExceptionType::matching::<HttpError>(predicate)` 可选择 Rust 枚举的具体变体。重复切换不会嵌套旧的关闭处理器。关闭前或关闭后安装的 Fake 都保留记录列表；关闭时记录原本被忽略的异常，恢复后重新遵守原报告规则。控制台失败仍正常渲染，其他应用保持各自模式。

正式 `Exceptions::render` 返回可失败的装箱错误结果，关闭处理时保留原始共享错误。借用的不透明错误不能被长期保存：请传入 `SharedException`、使用拥有错误所有权的 HTTP 测试入口，或处理独立的 `ExceptionOwnershipError`。`SharedException::from_exception` 克隆已有共享所有者；再次将共享错误传给 `new`／`from_arc` 时保留原身份，包括映射重复返回同一目标时，确保重复报告判断正确。

`Exception::downcast_chain_ref::<E>()` 显式读取诊断来源，包括映射前异常；`including_sources` 选择器也使用它。映射后的规则仍只匹配新根。来源搜索最多检查 32 个节点，循环 `source()` 不会挂起测试或报告。

`TestApp` 自动捕获未处理错误。直接调用底层 Kernel 的 Future 时，用 `Exceptions::catch_reported(future).await` 包裹；只返回 `Response` 的旧处理器方法在显式关闭模式下以 `SharedException` 为载荷 panic。需要 `Result` 时使用可失败的门面或测试入口。

### 所有异常的限流策略 {#global-exception-throttle}

通过 `Exceptions::throttle_using_all` 注册全局策略，或在启动配置中使用 `configuration.throttle_all`。它与 `throttle_using::<E>` 共用有序回调集合。返回 `None` 继续查找；返回 `Unlimited` 明确放行并停止查找。

```rust
Exceptions::throttle_using_all(|exception| {
    if exception.downcast_ref::<TransientFailure>().is_some() {
        Some(ReportThrottle::Limit(Limit::per_minute(5)))
    } else {
        None
    }
})?;
```

未指定 `.by(...)` 时按实际 Rust 错误类型分组，不按消息分组。拥有错误的 `Exceptions::report(error)` 自动保留类型；借用查询使用 `Exception::typed(&error)`，具体类型的 `Arc<E>` 使用 `SharedException::from_typed_arc(error)`。映射后的报告按目标类型计费；显式 `.by("supplier-timeout")` 可以让不同类型共享配额。

已擦除类型的 `Arc<dyn Error + Send + Sync>` 或 `Exception::Error(&dyn_error)` 无法恢复 Rust 具体类型名。请显式指定 `.by(...)`，或使用上述构造器保留类型。全局无键策略遇到缺失类型信息时沿用限流失败放行规则，保留原异常报告，不会把所有未知类型悄悄混入同一配额。原有具类型策略仍可为旧借用调用推断匹配类型。


## report 助手 {#the-report-helper}

导入 `openrust::support::report`，报告一个拥有所有权的异常后继续当前流程，不生成响应。它使用当前应用的处理器，沿用忽略策略、回调、限流和去重。异步等待其 `LogResult`；日志失败仍传播错误。同一异常需要多次报告时，使用 `SharedException` 并克隆共享句柄。助手不会替换或安装处理器。

```rust
use openrust::{http::{HttpError, Response, StatusCode}, support::report};

pub async fn recover_order() -> Result<Response, HttpError> {
    let error = std::io::Error::other("recoverable-order-failure");
    report(error).await?;
    Response::json(StatusCode::OK, serde_json::json!({"recovered": true}))
}
```


## 异常类型自身的方法 {#exception-type-behavior}

实现 `ExceptionBehavior`，并在 `with_exceptions` 中用 `use_exception::<E>()` 注册具体类型一次。标准 `Error` 不公开可选方法，因而使用显式类型元数据替代反射。处理器在原实例上调用方法。自身 `report` 先于全局报告回调：`Stop` 表示已处理，`Continue` 使用普通回调和日志。自身 `render` 优先于全局渲染器，`None` 允许正常回退。类型上下文每次报告计算一次，先于全局上下文和默认 guard 用户 ID 合并。重复注册无副作用且按应用隔离；运行期 Exceptions 门面也提供同一注册入口。

对忽略类型实现 `ShouldntReport`，并在经过类型擦除的 `Error` 边界之前调用 `into_exception()`。这样标记与原始共享实例一起保留，无需额外配置忽略回调。未经转换直接传递原类型，标准 trait 对象无法恢复这一标记。也可显式调用 `SharedException::unreported(error)`。忽略只影响报告，不阻止 HTTP 渲染；映射成另一根类型后采用新类型的策略。

```rust
use std::{error::Error, fmt};
use openrust::{
    contracts::debug::{ExceptionBehavior, ShouldntReport},
    foundation::configuration::{Exceptions, ReportDecision},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
};

#[derive(Debug)]
pub struct OrderError { pub handled: bool }
impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("订单无效。") }
}
impl Error for OrderError {}
impl ExceptionBehavior for OrderError {
    fn report(&self) -> ReportDecision {
        if self.handled {
            eprintln!("已由订单监控处理。");
            ReportDecision::Stop
        } else { ReportDecision::Continue }
    }
    fn render(&self, request: &Request) -> Option<Response> {
        request.is("api/orders/render").then(||
            Response::new(StatusCode::CONFLICT, Body::from("订单校验失败。")))
    }
    fn context(&self) -> LogContext {
        [("order_id".into(), 7.into())].into_iter().collect()
    }
}

#[derive(Debug)]
pub struct QuietError;
impl fmt::Display for QuietError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("无需报告的订单异常。") }
}
impl Error for QuietError {}
impl ShouldntReport for QuietError {}

pub fn configure(exceptions: &mut Exceptions) {
    exceptions.use_exception::<OrderError>();
}

pub fn reject_order(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = request.route("kind").unwrap_or_default();
        let source: Box<dyn Error + Send + Sync> = if kind == "quiet" {
            Box::new(QuietError.into_exception())
        } else { Box::new(OrderError { handled: kind == "handled" }) };
        Err(HttpError::Internal { source })
    })
}
```

## 异步命名视图响应 {#async-exception-rendering}

将 `configure_async` 传给 `with_exceptions`，并按下面模板创建 `resources/views/errors/invalid-order.blade.html`。

```rust
use openrust::{
    foundation::configuration::Exceptions,
    http::StatusCode,
    support::facades::Response,
    view::ViewData,
};

fn configure_async(exceptions: &mut Exceptions) {
    exceptions.render_async::<std::io::Error>(|error, request| {
        Box::pin(async move {
            let data = ViewData::from_iter([
                ("message".into(), error.to_string().into()),
            ]);
            let response = Response::view_for_request(
                "errors.invalid-order", data, StatusCode::CONFLICT,
                Default::default(), request,
            ).await?;
            Ok(Some(response))
        })
    });
}
```

```blade
<h1>{{ $message }}</h1>
```

`render_async::<E>` 接收装箱的异步结果：`Ok(Some(response))` 处理异常，`Ok(None)` 继续后续回调与默认渲染，`Err(HttpError)` 报告新错误并渲染其默认响应。同步和异步回调按注册顺序执行，最终响应只经过一次 `respond`。回调借用原异常和可变请求，因此 `Response::view_for_request` 复用同一视图工厂、请求与会话。

运行期通过 Facade 使用 `Exceptions::renderable_async::<E>`。异常类型可覆盖 `ExceptionBehavior::render_async`；默认实现委托同步 `render`，保持类型自身方法优先且不重复执行，均由 `use_exception::<E>` 接入。显式异步入口避免为了 I/O 阻塞执行器；同步 `Exceptions::render` 仅执行同步回调，正常 HTTP 处理与 `ExceptionHandler::render_http` 等待两种回调。

## 过期页面重定向与消息闪存 {#respond-with-session}

最终回调可将 419 响应重定向，并为下一次请求闪存消息。需要可变请求时使用 `respond_async`，它替换先前的最终回调。将 `configure_response` 传给 `with_exceptions`。

```rust
use openrust::{
    contracts::debug::Exception,
    foundation::configuration::Exceptions,
    http::{HttpFuture, Request, Response},
    support::facades::Redirect,
};

const MESSAGE: &str = "页面已过期，请重试。";

fn expired_response<'a>(
    response: Response,
    _: &'a Exception<'_>,
    request: &'a mut Request,
) -> HttpFuture<'a, Response> {
    Box::pin(async move {
        if response.status().as_u16() != 419 {
            return response;
        }
        let redirect = Redirect::back(request, Some("/form"), Default::default())
            .and_then(|redirect| redirect.with(request, "message", MESSAGE.into()));
        match redirect {
            Ok(redirect) => redirect.into(),
            Err(_) => response,
        }
    })
}

fn configure_response(exceptions: &mut Exceptions) {
    exceptions.respond_async(expired_response);
}
```

Web 会话中间件在此回调后持久化闪存数据。示例在重定向或会话不可用时保留原响应。运行期通过 `Exceptions::respond_using_async` 注册；只读调整继续使用同步 `respond`，同步底层渲染不执行异步回调。

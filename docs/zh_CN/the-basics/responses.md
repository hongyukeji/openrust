# HTTP 响应

## 创建响应

用 `Response::json` 返回 JSON 数据和明确的状态码。通过 `header` 或 `withHeaders` 设置类型化 HTTP 响应头。用 `Response::stream` 从 Rust 流逐块发送正文。

输入读取和 JSON 响应的完整示例见[控制器](./controllers.md)。

### 向响应附加响应头 {#attaching-headers-to-responses}

用 `withoutHeader` 删除一个类型化响应头，用 `withoutHeaders` 批量删除。两者都返回响应，可以继续链式调用。

#### 缓存控制中间件 {#cache-control-middleware}

在需要缓存策略的路由上使用 `cache.headers`。选项名使用 `snake_case`，用分号分隔。中间件处理成功的 GET 和 HEAD 响应。指定 `etag` 时沿用已有的验证器，或用与 Laravel `xxh128` 相同的 XXH3-128 摘要计算响应体。流式响应可用 `etag=value` 显式提供验证器，无需缓冲整个流。

```rust
Route::getWith("/cacheable", cache_response, |route| {
    route.middleware_name("cache.headers:public;max_age=2628000;etag");
    Ok(())
})?;
```

`last_modified` 接受 Unix 时间戳或日期表达式。`If-None-Match` 或 `If-Modified-Since` 匹配时可以返回无正文的 `304 Not Modified`。当请求同时提供两个验证条件且响应有 ETag 时，优先比较 ETag。

## 向响应附加 Cookie

在包含 `AddQueuedCookiesToResponse` 的中间件栈中，用 `Cookie::queue` 将 Cookie 加入队列，处理器返回后由中间件写入响应。也可以用 `response.cookie(&cookie, request)?` 直接附加。把 `EncryptCookies` 放在队列中间件之前，让它同时保护直接附加和排队写入的 Cookie。 工厂从 Session 配置继承路径、域、Secure 和 SameSite 默认值。有效期设为零会创建浏览器会话 Cookie。

```rust
use openrust::{
    cookie::CookieOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Cookie,
};

fn remember(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let cookie = Cookie::make("theme", "dark", 30, &CookieOptions::default())?;
        Cookie::queue(cookie)?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

## JSON 业务响应

需要统一业务响应格式的路由可以使用 `ApiResponse`。响应包含 `success`、`code` 和 `message`，按需附带 `data`、分页 `meta` 或字段 `errors`。`success(data)` 默认返回 HTTP 200。使用 `failure` 时传入状态码，也可以通过 `with_status` 设置。

```rust
use openrust::{
    http::{ApiResponse, PageMeta, StatusCode},
    routing::RoutingError,
    support::facades::Route,
};

fn register() -> Result<(), RoutingError> {
    Route::get("/items", || {
        ApiResponse::success([1, 2]).with_meta(PageMeta {
            page: 2,
            per_page: 2,
            total: 4,
            total_pages: 2,
        })
    })?;

    Route::get("/example-conflict", || {
        ApiResponse::failure(StatusCode::CONFLICT, "conflict", "The item already exists.")
    })
}
```

`ApiResponse::empty()` 返回省略 `data` 的成功响应。HTTP 204 响应的正文为空。请给 `failure` 传入可安全展示给客户端的消息，需要返回字段错误时使用 `with_errors`。

序列化失败交给[异常处理](errors.md)。内置异常和健康检查各自使用原有的响应格式。统一业务响应是 OpenRust 在 [Laravel JSON 响应机制](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#json-responses)之上的扩展。

## 重定向响应

使用 `Redirect::to` 跳转到 URL，或用 `Redirect::route` 跳转到命名路由。`RedirectOptions` 默认使用 302，并支持响应头。`Redirect::guest` 将访问目标存入当前会话，`Redirect::intended` 读取并清除它。会话没有访问目标时使用回退地址。

用 `Redirect::action` 跳转到[已注册的控制器动作](./urls.md#urls-for-controller-actions)。

`Redirect::refresh` 重定向到不带查询串的当前路径。`Redirect::secure` 从应用路径生成 HTTPS 目标。`Redirect::away` 将显式 HTTP(S) URL 或相对引用写入 `Location`，调用时请传入可信目标。`Redirect::signedRoute` 和 `Redirect::temporarySignedRoute` 重定向到绝对签名 URL。`Redirect::back` 依次使用同源 Referer、上一会话 URL 和回退地址。`Redirect::getIntendedUrl` 读取当前预期地址，`Redirect::setIntendedUrl` 显式写入预期地址。两个方法都接收当前请求。

路由级重定向可使用 `Router::redirect` 或 `Router::permanentRedirect`，把来源参数代入目标路径。目标中的可选段（如 `/articles/{slug?}`）没有对应值时会省略。`destination` 和 `status` 是保留的来源参数名。

### 重定向时闪存会话数据 {#redirecting-with-flashed-session-data}

在启用会话的 Web 路由中，使用 `with` 闪存值，`withInput` 保留输入，`withErrors` 附加命名的 `MessageBag`。Rust 方法显式接收当前请求的可变引用。处理器返回时，将重定向转换为 HTTP 响应。

```rust
use openrust::{
    http::{HttpError, Request, Response},
    support::facades::Redirect,
};
use serde_json::json;

async fn save_profile(request: &mut Request) -> Result<Response, HttpError> {
    let redirect = Redirect::to("/profile", Default::default())?
        .with(request, "status", json!("saved"))?
        .onlyInput(request, &["name"])
        .await?;
    Ok(redirect.into())
}
```

向 `withInput` 传入 `None` 时，读取请求的表单或 JSON 字段和查询参数。上传文件会被排除。使用 `onlyInput` 或 `exceptInput` 按点路径筛选字段。下一次请求可用 `request.old(Some("name"))` 读取旧输入，并从会话读取闪存值。

## 其他响应类型 {#other-response-types}

### 视图响应 {#view-responses}

用 `Response::view` 渲染已准备好的视图，并指定状态码和响应头。可以传入 `view("name")`，也可以用 `View::first` 门面选择第一个可用模板。渲染保留当前请求的上下文。

### JSONP 响应 {#jsonp-responses}

用 `Response::jsonp(status, Some(callback), data)` 将 JSON 数据传给 JavaScript 回调。回调名支持点分名称和方括号字面量键。无效回调名返回 HTTP 400。传入 `None` 返回普通 JSON 响应。

### Markdown 响应 {#markdown-responses}

用 `Response::markdown(status, content)` 发送 Markdown 源文本，媒体类型为 `text/markdown; charset=utf-8`。由客户端决定如何渲染。

### 空响应 {#empty-responses}

用 `Response::noContent()` 返回无正文的 HTTP 204。用 `Response::empty(status)` 指定其他状态码，用 `withHeaders` 附加响应头。

### 文件下载 {#file-downloads}

用 `Response::download(path, name, headers, "attachment").await` 下载本地文件。传入 `Some(name)` 指定下载名，传入 `None` 沿用原文件名。使用 `"inline"` 可指定内联处置。

### 文件响应 {#file-responses}

用 `Response::file(path, headers).await` 返回本地文件、媒体类型和长度。两种方法都将应用选定的普通文件按块发送，默认采用公开缓存策略。

HTTP Kernel 在响应中间件后处理单个字节区间。`If-Range` 可匹配强 ETag 或文件的 `Last-Modified`。HEAD 返回完整文件的元数据，正文为空。需要缓存重新验证和 `304 Not Modified` 时，为路由配置 `cache.headers`。格式无效或含多个区间的请求返回完整文件。

## 流式响应 {#streamed-responses}

使用 `Response::stream` 发送原始数据块时，响应会带上 `X-Accel-Buffering: no`。

### 在浏览器中消费流式响应 {#consuming-streamed-responses}

使用 Fetch 的正文读取器可以逐块读取内容。对于流式 JSON 文档，可累积文本，在流结束后解析。

SSE 可以用 `EventSource` 按事件名监听，收到默认结束标记后关闭连接。

```javascript
const source = new EventSource('/events');
source.addEventListener('snapshot', (event) => {
    renderSnapshot(JSON.parse(event.data));
});
source.addEventListener('update', (event) => {
    if (event.data === '</stream>') source.close();
});
```

### 流式 JSON 响应 {#streamed-json-responses}

通过 `Response::streamJson` 和 `StreamedJson::object` 将数据行流放进 JSON 对象。`StreamedJson::items` 在每行到达时序列化。固定值使用 `StreamedJson::value`，嵌套数组使用 `StreamedJson::array`。使用 `StreamedJson::entries` 可将键值对逐项写入 JSON 对象。

流式 JSON 与 JSON 事件载荷沿用 Laravel 的默认字符串转义。SSE JSON 保留无需转义的非 ASCII 文本。

```rust
use futures_util::stream;
use openrust::http::{HttpError, Response, StatusCode, StreamedJson};

let users = stream::iter([Ok::<_, HttpError>(1), Ok(2)]);
let data = StreamedJson::object([("users", StreamedJson::items(users))]);
let response = Response::streamJson(StatusCode::OK, data);
```

### 服务端发送事件（SSE） {#event-streams}

`Response::eventStream` 发送 `text/event-stream` 并关闭缓存和代理缓冲。`StreamedEvent::message` 使用 `update` 事件。命名文本事件使用 `StreamedEvent::new`，JSON 数据使用 `StreamedEvent::json`。默认结束事件携带 `</stream>`。`Response::eventStreamWithEnd` 接收自定义结束事件，传入 `None` 可省略结束事件。

响应头发出后，JSON 或下载数据源失败会以传输错误结束正文。应用中的 SSE 数据源失败时，会交给当前应用的异常处理器报告，连接关闭且不发送结束事件。

### 流式下载 {#streamed-downloads}

`Response::streamDownload` 按块发送下载内容。提供文件名时，框架按指定的处置方式生成 `Content-Disposition`，并按需附加 UTF-8 文件名。文件名为 `None` 时，由调用者提供的响应头决定处置方式。

## 响应宏 {#response-macros}

在 Service Provider 的 `boot` 中注册异步响应宏。宏接收 JSON 值参数并返回 HTTP 响应。

```rust
use openrust::{
    http::{HttpError, Response as HttpResponse, StatusCode},
    support::facades::Response,
};

Response::r#macro("caps", |arguments| async move {
    let [serde_json::Value::String(value)] = arguments.as_slice() else {
        return Err(HttpError::InvalidRequest);
    };
    HttpResponse::json(StatusCode::OK, value.to_uppercase())
})?;
```

在路由或控制器中按名称调用。

```rust
Response::call("caps", vec!["hello".into()]).await
```

`hasMacro` 检查当前工厂。`mixin` 接收显式列出的 `ResponseMacro` 和替换开关。`flushMacros` 清空当前应用的注册表。每个应用使用自己的响应工厂。

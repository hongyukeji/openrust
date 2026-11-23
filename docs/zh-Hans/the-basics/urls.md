# URL 生成

## 命名路由 URL

通过 `Url::route` 传入路由名称与有序参数。传 `true` 返回绝对 URL，传 `false` 返回路径。`RouteParameter` 支持命名参数和位置参数。替换路由占位符后剩余的参数会进入查询串。

用 `RouteParameter::model("article", &article)?` 或 `RouteParameter::positional_model(&article)?` 传入 Eloquent 模型。`{article:slug}` 通过 Eloquent 读取 `slug`，应用对应访问器或类型转换。没有指定绑定字段的占位符使用模型的路由键。生成 URL 时借用模型，只读取选中的字段，无需查询数据库。必填字段缺失返回 `UrlError::MissingParameter`，访问器或类型转换失败返回 `UrlError::ModelAttribute`。

```rust
use openrust::{
    database::eloquent::Model,
    routing::{RouteParameter, UrlResult},
    support::facades::Url,
};

fn article_link(article: &impl Model) -> UrlResult<String> {
    Url::route("article", &[RouteParameter::model("article", article)?], true)
}
```

## 签名 URL

用 `Url::signed_route` 生成长期链接，或用 `Url::temporary_signed_route` 和 `SystemTime` 设置到期时间。为目标路由挂载与签名模式对应的 `signed` 或 `signed:relative` 中间件。篡改或过期的链接返回 HTTP 403。签名按配置原样使用 `APP_KEY`，轮换时可以通过 `APP_PREVIOUS_KEYS` 保留旧链接。

### 在请求上验证签名 {#validating-signed-route-requests}

可以直接在请求实例上验证签名链接。绝对签名显式传入 `true`；相对签名用 `has_valid_relative_signature()`。Rust 要求显式传入绝对模式参数。

```rust
use openrust::http::{HttpError, HttpFuture, Request, Response, StatusCode};

fn unsubscribe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if !request.has_valid_signature(true)? {
            return Ok(Response::empty(StatusCode::UNAUTHORIZED));
        }
        Response::json(StatusCode::OK, request.query("email")?)
    })
}
```

用 `has_valid_signature_while_ignoring(&["page", "order"], true)` 或 `has_valid_relative_signature_while_ignoring(&["page", "order"])` 允许指定查询键变化。被忽略的值不受签名保护。签名只覆盖 URL，不覆盖 JSON 或表单正文：即使是 GET 请求，也应通过 `request.query(...)` 读取已签名值；`input(...)` 可能读到正文中未签名的覆盖值。

## 请求 URL 与资源 URL

HTTP 请求内生成链接时，依次使用显式 `APP_URL`、当前请求的协议和主机、从监听配置推导的 `app.url`。Artisan 直接使用 `app.url`。监听地址的回退规则见[配置](../getting-started/configuration.md)。`Url::asset` 支持 `ASSET_URL`。普通 URL 可使用 `Url::to`、`Url::current` 和 `Url::query`。

`Url::current()` 返回去掉查询串的当前 URL。`Url::full()` 包含原始请求路径和查询串。`Url::previous(request, fallback)` 依次使用同源 Referer、上一会话 URL 和回退地址。`Url::previous_path(request, fallback)` 只返回路径，并移除应用基路径和末尾的斜杠。在 Web 路由中，`request.session()?.previous_route()` 返回上一次记录的路由名。`previous_uri()` 在存在上一 URL 时返回解析后的 `openrust::support::Uri`。

## 请求级 URL 默认值 {#default-values}

生成链接前调用 `Url::defaults`，通常放在路由中间件中。多次调用会合并当前请求的默认值，后续请求有独立的默认值。显式具名和位置参数可覆盖默认值。使用自定义绑定字段时，指定 `"article:slug"` 一类的键。默认值也适用于动作 URL、签名链接和路由重定向。在 Artisan 中，可以用 `Url::generator()?.with_defaults(...)` 给单个生成器设置默认值，再导入 `openrust::contracts::routing::UrlGenerator` 调用生成器方法。

```rust
use openrust::{http::Request, routing::UrlResult, support::facades::Url};

fn articles_url(request: &mut Request, locale: &str) -> UrlResult<String> {
    Url::defaults(request, &[("locale", locale)]);
    Url::route("articles.index", &[], true)
}
```

## 控制器动作 URL {#urls-for-controller-actions}

注册 Rust 处理器时为路由设置稳定的动作标识。用 `Url::action` 生成 URL，用 `Redirect::action` 跳转。参数规则与命名路由相同。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError},
    support::facades::Url,
};

const SHOW: &str = "ArticleController@show";

fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/articles/{article}", show)?.set_action(SHOW)?;
    Ok(())
}

fn article_url() -> Result<String, openrust::routing::UrlError> {
    Url::action(SHOW, &[RouteParameter::named("article", "42")], true)
}
```

域名路由优先，同类生效路由中取首条注册。Rust 注册编译好的处理器，因此显式声明动作标识。
## 生成策略

克隆请求的 `UrlGenerator` 后，可调用 `force_scheme`、`force_https`、`force_root_url`、`format_host_using`、`format_path_using`。这些方法需要该生成器的可变借用，不修改其他请求的策略。传入 `None` 清除强制协议或根地址。格式化器作用于生成路径和命名路由，包括签名地址；验证签名时使用生成时相同的绝对或相对模式。可信代理需单独配置，使请求来源使用预期的转发域名和协议。


## 资源来源与默认参数

通过 `UrlGenerator::use_origin(Some(root))` 覆盖生成 URL 的来源，传入 `None` 恢复请求或配置来源。`use_asset_origin(Some(root))` 覆盖资源来源，`None` 清除资源来源（包括配置的 CDN）。这些修改属于当前生成器实例及其克隆。`secure_asset(path)` 请求 HTTPS，但保留显式资源来源。`asset_from(root, path, secure)` 使用自定义根地址，协议采用当前请求或强制协议。`get_default_parameters()` 读取命名默认参数。`Url` Facade 同样提供 `secure_asset`、`asset_from` 和 `get_default_parameters`。

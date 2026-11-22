# URL 生成

## 命名路由 URL

通过 `URL::route` 传入路由名称与有序参数。传 `true` 返回绝对 URL，传 `false` 返回路径。`RouteParameter` 支持命名参数和位置参数。替换路由占位符后剩余的参数会进入查询串。

用 `RouteParameter::model("article", &article)?` 或 `RouteParameter::positional_model(&article)?` 传入 Eloquent 模型。`{article:slug}` 通过 Eloquent 读取 `slug`，应用对应访问器或类型转换。没有指定绑定字段的占位符使用模型的路由键。生成 URL 时借用模型，只读取选中的字段，无需查询数据库。必填字段缺失返回 `UrlError::MissingParameter`，访问器或类型转换失败返回 `UrlError::ModelAttribute`。

```rust
use openrust::{
    database::eloquent::Model,
    routing::{RouteParameter, UrlResult},
    support::facades::URL,
};

fn article_link(article: &impl Model) -> UrlResult<String> {
    URL::route("article", &[RouteParameter::model("article", article)?], true)
}
```

## 签名 URL

用 `URL::signedRoute` 生成长期链接，或用 `URL::temporarySignedRoute` 和 `SystemTime` 设置到期时间。为目标路由挂载与签名模式对应的 `signed` 或 `signed:relative` 中间件。篡改或过期的链接返回 HTTP 403。签名按配置原样使用 `APP_KEY`，轮换时可以通过 `APP_PREVIOUS_KEYS` 保留旧链接。

## 请求 URL 与资源 URL

HTTP 请求内生成链接时，依次使用显式 `APP_URL`、当前请求的协议和主机、从监听配置推导的 `app.url`。Artisan 直接使用 `app.url`。监听地址的回退规则见[配置](../getting-started/configuration.md)。`URL::asset` 支持 `ASSET_URL`。普通 URL 可使用 `URL::to`、`URL::current` 和 `URL::query`。

`URL::current()` 返回去掉查询串的当前 URL。`URL::full()` 包含原始请求路径和查询串。`URL::previous(request, fallback)` 依次使用同源 Referer、上一会话 URL 和回退地址。`URL::previousPath(request, fallback)` 只返回路径，并移除应用基路径和末尾的斜杠。在 Web 路由中，`request.session()?.previousRoute()` 返回上一次记录的路由名。`previousUri()` 在存在上一 URL 时返回解析后的 `openrust::support::Uri`。

## 请求级 URL 默认值 {#default-values}

生成链接前调用 `URL::defaults`，通常放在路由中间件中。多次调用会合并当前请求的默认值，后续请求有独立的默认值。显式具名和位置参数可覆盖默认值。使用自定义绑定字段时，指定 `"article:slug"` 一类的键。默认值也适用于动作 URL、签名链接和路由重定向。在 Artisan 中，可以用 `URL::generator()?.withDefaults(...)` 给单个生成器设置默认值，再导入 `openrust::contracts::routing::UrlGenerator` 调用生成器方法。

```rust
use openrust::{http::Request, routing::UrlResult, support::facades::URL};

fn articles_url(request: &mut Request, locale: &str) -> UrlResult<String> {
    URL::defaults(request, &[("locale", locale)]);
    URL::route("articles.index", &[], true)
}
```

## 控制器动作 URL {#urls-for-controller-actions}

注册 Rust 处理器时为路由设置稳定的动作标识。用 `URL::action` 生成 URL，用 `Redirect::action` 跳转。参数规则与命名路由相同。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError},
    support::facades::URL,
};

const SHOW: &str = "ArticleController@show";

fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/articles/{article}", show)?.setAction(SHOW)?;
    Ok(())
}

fn article_url() -> Result<String, openrust::routing::UrlError> {
    URL::action(SHOW, &[RouteParameter::named("article", "42")], true)
}
```

域名路由优先，同类生效路由中取首条注册。Rust 注册编译好的处理器，因此显式声明动作标识。
## 生成策略

克隆请求的 `UrlGenerator` 后，可调用 `forceScheme`、`forceHttps`、`forceRootUrl`、`formatHostUsing`、`formatPathUsing`。这些方法需要该生成器的可变借用，不修改其他请求的策略。传入 `None` 清除强制协议或根地址。格式化器作用于生成路径和命名路由，包括签名地址；验证签名时使用生成时相同的绝对或相对模式。可信代理需单独配置，使请求来源使用预期的转发域名和协议。

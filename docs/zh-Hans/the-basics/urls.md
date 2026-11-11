# URL 生成

## 命名路由 URL

通过 `URL::route` 传入路由名称与有序参数。传 `true` 返回绝对 URL，传 `false` 返回路径。`RouteParameter` 支持命名参数和位置参数。替换路由占位符后剩余的参数会进入查询串。可用 `RouteParameter::model` 传入 Eloquent 路由键。

```rust
use openrust::{
    routing::{RouteParameter, UrlResult},
    support::facades::URL,
};

fn article_link() -> UrlResult<String> {
    URL::route("article", &[RouteParameter::named("article", "42")], true)
}
```

## 签名 URL

用 `URL::signedRoute` 生成长期链接，或用 `URL::temporarySignedRoute` 和 `SystemTime` 设置到期时间。为目标路由挂载与签名模式对应的 `signed` 或 `signed:relative` 中间件。篡改或过期的链接返回 HTTP 403。签名按配置原样使用 `APP_KEY`，轮换时可以通过 `APP_PREVIOUS_KEYS` 保留旧链接。

## 请求 URL 与资源 URL

HTTP 请求内使用当前请求的协议和主机。应用作用域内的命令行使用 `APP_URL`。`URL::asset` 支持 `ASSET_URL`。普通 URL 可使用 `URL::to`、`URL::current` 和 `URL::query`。

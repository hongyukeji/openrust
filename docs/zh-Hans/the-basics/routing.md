# 路由

在 `routes/web.rs` 声明 Web 路由。应用启动时通过 `openrust::routing::Router` 注册这些路由和 `/up` 健康检查。需要读取请求的处理器接收可变的 `Request` 并返回异步 HTTP 结果。框架调用 Web 路由模块的 `register()` 时绑定当前应用的 Router。导入 `openrust::support::facades::Route` 后，可以用 `Route::get("/", || view("welcome"))` 声明命名视图。

在 `routes/api.rs` 声明 API 路由。应用骨架默认挂载到 `/api` 并应用 `api` 中间件组。设置 `ROUTING_API_PREFIX=/v1/internal` 可修改 `routing.api_prefix`，在 Builder 中调用 `Routing::apiPrefix("v2")` 则优先于配置。首尾斜杠可以省略。设置 `ROUTING_API_PREFIX=` 或传入空字符串可省略前缀，`api` 中间件组仍会应用。

在 `routes/api.rs` 的无参 `register() -> Result<(), RoutingError>` 中使用 `Route` Facade 声明 API 端点。路由路径相对于 API 前缀。GET 路由需要命名、模型绑定或路由级中间件时，可使用 `Route::getWith`。

`{id}` 表示必填参数。在末尾参数上加 `?` 即可省略它，例如 `/profiles/{id?}/{tab?}` 或 `/report/{name}.{format?}`。使用 `request.route("id")` 读取匹配值。省略参数时返回 `None`，也可以用路由的 `defaults` 提供字符串默认值。参数后面还有必填参数或固定文本时，该参数仍须提供。

用 `Route::r#where` 指定正则表达式，也可使用 `where_number`、`where_alpha`、`where_alpha_numeric`、`where_uuid`、`where_ulid` 或 `where_in`。在注册路由前调用 `Router::pattern`，可给后续路由的同名参数设置约束。单条路由上的约束优先。`where_in` 将值拼成正则的备选分支，正则元字符保留原义。OpenRust 使用 Rust 的 `regex` 语法，PCRE 的环视和反向引用会在注册时被拒绝。

匹配时按 UTF-8 对路径执行百分号解码，并忽略查询字符串。解码结果无效时返回 HTTP 400。匹配路由和检查可用方法时都应用约束。例如 `/profiles/{id?}` 是限制数字 ID 的 GET 路由时，`POST /profiles/7` 返回 405 和 `Allow`。如果没有其他路由匹配该路径，`POST /profiles/alice` 返回 404。

`Router::get` 同时注册 GET 和 HEAD。HEAD 保留响应状态和响应头，发送空响应体。未知路径返回 404。路径存在但方法不受支持时返回 405，并附带 `Allow` 响应头。路径存在且未显式注册 OPTIONS 时，路由器返回 200 并列出允许的方法。同一方法有多条路由可匹配时，使用最先注册的匹配项。

## 挂载静态目录

应用骨架先挂载网站公共目录，再挂载公共存储磁盘。目录和 URL 前缀见[公共文件配置](../getting-started/configuration.md#public-files)。

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

相对目录从应用根目录解析。例如，`public/css/app.css` 对应 `/css/app.css`。如需关闭公共磁盘挂载，移除 `routes/web.rs` 中的 `Route::staticDisk("public")?` 调用。网站目录默认挂到 `/`，公共磁盘默认挂到 `/storage`。两个挂载都能响应同一个 URL 时，框架按声明顺序返回第一个命中的文件。按骨架的顺序，请求 `/storage/avatar.png` 时，`public/storage/avatar.png` 优先于 `storage/app/public/avatar.png`。公共磁盘无需创建 `public/storage` 符号链接。应用路由优先，只有未匹配任何应用路由的 GET 和 HEAD 请求才会查找文件。文件不存在或请求指向目录时返回 404，符号链接无法访问挂载目录外的文件。

静态文件有可用的修改时间时，响应会携带 `Last-Modified`。GET 或 HEAD 请求可以把该值放入 `If-Modified-Since` 重新验证文件。文件未修改时返回 `304 Not Modified`，响应体为空。

## 健康检查

`bootstrap/app.rs` 的 `health("/up")` 注册健康端点。普通请求返回包含“Application up”的 HTML 页面，期望 JSON 的请求返回 `{"status":"up"}`。两者均返回 200，HEAD 不发送正文。HTML 标题使用经过转义的应用名称。

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

这表示 HTTP 引导已成功。诊断事件监听器及其失败响应等待事件分发器实现。健康页面使用本地 HTML，未使用 Laravel 的 Blade 视图和外部样式资源。

## 查看路由列表

用 `route:list` 查看生效的路由，用 `--json` 输出结构化结果。可按路径、方法、名称、域名、处理器或中间件筛选。`--except-path` 接收逗号分隔的子串。`--sort=definition` 保留注册顺序，`--reverse` 反转结果。

```sh
./artisan route:list --json --method=HEAD
```

## 路由参数

必填参数写成 `{id}`，末尾的可选参数写成 `{id?}`，默认值用 `defaults` 设置。参数不符合约束时，继续匹配其他路由，均未匹配则返回 404。

## 路由分组

分组共享 URI 前缀、名称前缀、域名、中间件和约束。域名限定路由应先于相同路径的不限域名路由注册。域名和路径的占位符都可以通过 `request.route` 读取。

分组闭包接收一个可变的 Rust 路由器，可用它配置已注册的路由。GET 路由同时接受 HEAD，自动 OPTIONS 根据匹配的路径和域名返回允许的方法。

```rust
use openrust::{routing::{RouteGroup, RoutingError}, support::facades::Route, view::view};

fn register() -> Result<(), RoutingError> {
    Route::group(
        RouteGroup::new()
            .domain("{tenant}.example.test")
            .prefix("admin")
            .name("admin.")
            .r#where("id", "[0-9]+")?,
        |router| {
            router
                .get("/reports/{id?}", || view("welcome"))?
                .name("reports.show")
                .defaults("id", "1");
            Ok(())
        },
    )
}
```

## 跨源资源共享（CORS）

在 `config/cors.rs` 中配置路径、来源、方法和请求头。全局 `HandleCors` 中间件对匹配的浏览器预检返回空正文 HTTP 204，也会为实际响应应用策略，包括 404。在动态策略中，被拒绝的来源不会收到来源许可。只配置一个来源时可发送固定值，浏览器仅允许该来源。

未设置 `CORS_PATHS` 时，`cors.paths` 沿用 Laravel 的默认路径 `api/*` 和 `sanctum/csrf-cookie`。`.env.example` 中的 `CORS_PATHS=*` 对所有路径启用 CORS。用逗号分隔的路径列表（如 `CORS_PATHS=api/*,webhooks/*`）可以限制范围，`CORS_PATHS=` 可关闭 CORS。跨域凭据默认关闭。路径映射可按主机覆盖路径，用数字字符串键表示共用路径。`allowed_origins_patterns` 使用 Rust 正则引擎支持的模式。HTTP 内核在启动时加载并校验策略。

```bash
curl -i -X OPTIONS http://127.0.0.1:8000/api/example \
  -H 'Origin: https://example.com' \
  -H 'Access-Control-Request-Method: GET'
```

## 定义限流器

在服务提供器的 boot 阶段注册策略，再用 `middleware_name("throttle:uploads")` 附加到路由。用 `Limit::by` 划分计数键，`response` 自定义拒绝响应，或用 `after` 选择哪些响应消耗额度。拒绝返回 HTTP 429，并附带 `Retry-After`、`X-RateLimit-Reset`、`X-RateLimit-Limit` 和 `X-RateLimit-Remaining`。

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Limit},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::{RateLimiter, Route},
};

fn register_limits() -> CacheResult<()> {
    RateLimiter::r#for("uploads", |_| Limit::perMinute(5))
}

fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new(), |router| {
        router.get("/uploads", upload)?.middleware_name("throttle:uploads");
        Ok(())
    })
}

fn upload(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}
```

### 使用 Redis 节流

在 `withMiddleware` 中调用 `middleware.throttleWithRedis()`，让命名节流别名使用默认 Redis 连接。使用相同 Redis 连接设置和键前缀的进程共享配额。Redis 原子检查并消耗各项额度。使用 `after` 的规则在响应返回后计数。

## 路由模型绑定

使用 `Route::model::<Post>("post")?` 在应用 Router 注册显式绑定。也可以通过 Router 返回的路由调用 `.model::<Post>("post")?`，为该路由声明模型参数。使用 `request.route_model::<Post>("post")` 读取已解析的模型。模型缺失返回 404，可通过 `missing` 自定义响应。

模型默认按主键查询。可覆盖 `getRouteKeyName` 使用 slug，也可为单条路由指定 `{post:slug}`。嵌套的自定义键通过前一个已绑定的模型限定查询范围。使用 `scopeBindings` 为默认键开启作用域，或通过 `withoutScopedBindings` 关闭。子模型通过 `resolveChildRouteBinding` 选择类型化关系。`withTrashed` 可包含软删除记录。默认 `web` 和 `api` 组在处理器前执行绑定。

## 重定向路由

使用 `Route::redirect` 注册重定向并指定 HTTP 状态。`Route::permanentRedirect` 返回 301。`/articles/{id}` 这样的目标可以复用匹配路由的参数。

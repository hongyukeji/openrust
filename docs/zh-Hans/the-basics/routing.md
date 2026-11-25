# 路由

在 `routes/web.rs` 声明 Web 路由。应用启动时通过 `openrust::routing::Router` 注册这些路由和 `/up` 健康检查。需要读取请求的处理器接收可变的 `Request` 并返回异步 HTTP 结果。框架调用 Web 路由模块的 `register()` 时绑定当前应用的 Router。导入 `openrust::support::facades::Route` 后，可以用 `Route::get("/", || view("welcome"))` 声明命名视图。

默认应用已经预设并注册 `routes/api.rs` 和 `config/api.rs`，编写普通 API 不需要 Sanctum。需要 API 认证时再运行 `./artisan install:api`，安装 Sanctum 并向未改动的空 API 骨架加入受保护的 `/user`。已有业务路由默认保留，只有显式 `--force` 才覆盖；保留自定义路由文件时，手动追加受保护入口。在 `routes/api.rs` 中定义 API 路由。应用从 `config/api.rs` 加载 `api.prefix`，并应用 `api` 中间件组。默认前缀为 `/api`。在 `.env` 中设置 `API_PREFIX=api/admin` 可挂载到 `/api/admin`，设置 `API_PREFIX=` 可省略前缀。在 `with_routing` 中显式调用 `Routing::api_prefix(...)` 时，以该调用为准。 重复安装时保留已有的 `config/api.rs`，包括使用 `--force` 时。

在 `routes/api.rs` 的无参 `register() -> Result<(), RoutingError>` 中使用 `Route` Facade 声明 API 端点。路由路径相对于 API 前缀。GET 路由需要命名、模型绑定或路由级中间件时，可使用 `Route::get_with`。

`{id}` 表示必填参数。在末尾参数上加 `?` 即可省略它，例如 `/profiles/{id?}/{tab?}` 或 `/report/{name}.{format?}`。使用 `request.route("id")` 读取匹配值。省略参数时返回 `None`，也可以用路由的 `defaults` 提供字符串默认值。参数后面还有必填参数或固定文本时，该参数仍须提供。

用 `Route::r#where` 指定正则表达式，也可使用 `where_number`、`where_alpha`、`where_alpha_numeric`、`where_uuid`、`where_ulid` 或 `where_in`。在注册路由前调用 `Router::pattern`，可给后续路由的同名参数设置约束。单条路由上的约束优先。`where_in` 将值拼成正则的备选分支，正则元字符保留原义。没有显式约束的路由使用 Rust 的线性正则引擎。显式约束使用 PCRE2，支持环视、命名反向引用和 `(*MARK:...)`。例如，`r"(?!admin\z)[a-z]+"` 排除 `admin`。匹配使用 UTF-8 和 Unicode 字符属性。无效表达式会使注册失败。运行时达到 PCRE2 的匹配、深度或堆内存上限时，按未匹配处理。在 `/paired/{id}/{tab?}` 中，将 `tab` 的约束设为 `r"\k<id>"`，可要求传入的 `tab` 与 `id` 相同。反向引用可指向同一路径或同一域名中的其他捕获组。分组和全局约束的引用在注册路由时解析。

匹配时按 UTF-8 对路径执行百分号解码，并忽略查询字符串。解码结果无效时返回 HTTP 400。匹配路由和检查可用方法时都应用约束。例如 `/profiles/{id?}` 是限制数字 ID 的 GET 路由时，`POST /profiles/7` 返回 405 和 `Allow`。如果没有其他路由匹配该路径，`POST /profiles/alice` 返回 404。

`Router::get` 同时注册 GET 和 HEAD。HEAD 保留响应状态和响应头，发送空响应体。未知路径返回 404。路径存在但方法不受支持时返回 405，并附带 `Allow` 响应头。路径存在且未显式注册 OPTIONS 时，路由器返回 200 并列出允许的方法。同一方法有多条路由可匹配时，使用最先注册的匹配项。

## HTTP 方法

单一方法使用 `get`、`post`、`put`、`patch`、`delete`、`options` 或 `query`。`Route` 和 `Router` 均提供这些入口；`get` 同时接受 HEAD。`Router` 返回可变路由对象，可继续配置名称、中间件、约束和默认值；Facade 声明返回 `Result<(), RoutingError>`。

```rust
use openrust::{http::Method, support::facades::Route};

Route::get("/users", UserController::index)?;
Route::post("/users", UserController::store)?;
Route::put("/users/{id}", UserController::replace)?;
Route::patch("/users/{id}", UserController::update)?;
Route::delete("/users/{id}", UserController::destroy)?;
Route::options("/users", UserController::options)?;
Route::query("/users/search", UserController::search)?;
Route::r#match(&[Method::PUT, Method::PATCH], "/profile", ProfileController::update)?;
Route::any("/callback", CallbackController::handle)?;
```

多个指定方法共用处理器时使用 `r#match`，集合包含 GET 时自动补充 HEAD。空集合返回 `RoutingError::MissingMethods`。只有端点确实需要接受全部支持的方法时才使用 `any`，集合依次为 GET、HEAD、POST、PUT、PATCH、DELETE、OPTIONS、QUERY，不包含 TRACE 或 CONNECT。

对于相同方法、域名和注册路径，后声明覆盖先声明的处理器；`any` 和 `r#match` 也遵循此规则，没有独立优先级。不同但重叠的路径模式按声明顺序首次匹配，因此应将具体路径放在较宽泛的模式之前。

## 视图路由

GET/HEAD 路由只返回视图时，使用 `Route::view`。前两个参数是 URI 和视图名称。Rust 要求显式传入第三个选项参数；`Default::default()` 表示空数据、状态 200 和不附加响应头。

```rust
use openrust::{routing::ViewRouteOptions, support::facades::Route, view::ViewData};

Route::view("/preview", "welcome", Default::default())?;
Route::view("/hello/{name}", "welcome", ViewRouteOptions {
    data: ViewData::from_iter([("name".into(), "Taylor".into())]),
    ..Default::default()
})?;
```

模板放在 `resources/views`，例如 `welcome.blade.html`。路径和域名捕获参数覆盖 `data` 的同名值，随后执行视图创建器和组合器。用 `ViewRouteOptions` 的 `status` 和 `headers` 设置自定义响应。PHP 中在状态参数位置传响应头数组的写法，改用显式 `headers` 字段。参数名称不要使用 `view`、`data`、`status` 或 `headers`。当前路由默认值只承载字符串，不支持通过 `Route::defaults` 任意修改这四项类型化视图设置；绑定模型对象也不会自动转换为视图数据。

`Router::view` 返回可变路由。Facade 声明使用 `Route::view_with(uri, view, options, |route| { route.name("welcome"); Ok(()) })`，在注册借用范围内配置路由。渲染使用当前应用的视图工厂和中间件。模板不存在或创建器／组合器失败时，沿用普通 HTTP 异常管线。

## 后备路由

在 `routes/web.rs` 中声明后备路由，处理没有普通路由匹配的 GET/HEAD 请求。后备路由继承当前组的前缀、域名和中间件。已匹配处理器返回错误或 404 响应时，不会再次尝试后备路由。

```rust
use openrust::support::facades::Route;

Route::fallback(NotFoundController::show)?;
```

普通路由始终优先，即使后备路由先声明。多个后备路由可匹配时按注册顺序选择；同一方法、域名和注册路径的后声明覆盖先声明。未匹配的 POST 在后备路由匹配其路径时返回 405 和 `Allow: GET, HEAD`；自动 OPTIONS 返回 200。后备路由也是已注册路由，优先于下文的静态文件挂载。

Facade 需要配置时使用 `Route::fallback_with(handler, |route| { route.name("missing"); Ok(()) })`。`Router::fallback` 返回可变路由；`route.fallback()` 可标记已有路由，`route.set_fallback(false)` 取消标记。路由缓存保持该优先级。


处理器可以显式返回另一条命名路由的响应。`respond_with_route` 使用同一个请求 URI 重新绑定目标路由、运行其路由中间件，并替换请求的当前路由参数与名称；不会生成重定向，也不会将请求 URI 改成目标路径。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Route,
};

fn show_missing(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respond_with_route("missing", request))
}
```

先按上文用 `fallback_with` 登记 `missing`，再将 `show_missing` 登记为处理器。Rust 显式借用可变请求；HTTP 内核通过应用容器提供共享的运行期路由器，每个请求保留独立路由状态。名称不存在时返回错误，渲染为 HTTP 500。直接使用 `Router` 时也提供同名方法。运行期路由器在 HTTP 启动后可解析；路由注册仍使用注册作用域。

## 挂载静态目录

应用骨架先挂载网站公共目录，再挂载公共存储磁盘。目录和 URL 前缀见[公共文件配置](../getting-started/configuration.md#public-files)。

```rust
Route::static_disk("web")?;
Route::static_disk("public")?;
```

`web` 和 `public` 磁盘配置在 `config/filesystems.rs`，默认根目录分别是 `public` 和 `storage/app/public`，默认 URL 分别是 `app.url` 和 `app.url` 加 `/storage`。挂载路径从 URL 提取，包含应用 URL 自带的路径前缀。应用路由优先。磁盘挂载按声明顺序查找，第一个存在的公开文件生效。两个挂载都拒绝符号链接，因此会跳过 `public/storage` 链接，由 `public` 磁盘直接提供文件。只有未匹配应用路由的 GET 和 HEAD 请求会读取文件，缺失文件和目录请求返回 404。

静态文件有可用的修改时间时，响应会携带 `Last-Modified`。GET 或 HEAD 请求可以把该值放入 `If-Modified-Since` 重新验证文件。文件未修改时返回 `304 Not Modified`，响应体为空。

## 健康检查

`bootstrap/app.rs` 的 `health("/up")` 注册健康端点。健康时返回 200。HTML 页面的标题显示应用名称，正文显示 “Application up” 和从接收请求到渲染的耗时。期望 JSON 的请求返回 `{"status":"up"}`。HEAD 不发送正文。

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

端点会分发 `openrust::foundation::events::DiagnosingHealth`。可以注册同步监听器检查数据库或缓存等依赖。监听器失败时，框架报告错误并返回 500，响应为 “Application experiencing problems” 页面或 `{"status":"down"}`。调试模式下由常规异常处理器渲染错误。健康页内置样式。维护模式下仍可访问配置的健康路径。

## 查看路由列表

用 `route:list` 查看生效的路由，用 `--json` 输出结构化结果。可按路径、方法、名称、域名、处理器或中间件筛选。`--except-path` 接收逗号分隔的子串。`--sort=definition` 保留注册顺序，`--reverse` 反转结果。

```sh
./artisan route:list --json --method=HEAD
```

用 `-v` 查看每条路由声明的中间件。用 `-vv` 展开中间件组，按请求执行顺序显示类型名称，并应用排除规则和保留别名参数。输出范围为路由中间件。使用 `--json` 时，`middleware` 仍是数组，`--middleware` 按当前详细级别的内容筛选。

```sh
./artisan route:list -vv --json --middleware=SubstituteBindings
```

路由列表保留 `{post:slug}` 等绑定字段、可选标记和 `{tenant}.example.test` 等域名模板。文本和 JSON 输出都包含通过 `set_action` 登记的控制器动作。同一方法、域名和路径的后一次注册会替换前一次注册，即使绑定字段不同。其他 HTTP 方法仍使用原路由。

```sh
./artisan route:list --domain=example.test --action=PostController --json
```

## 路由缓存 {#route-caching}

使用 `route:cache` 为部署后的应用生成匹配索引。

```sh
./artisan route:cache
./artisan route:clear
```

缓存按 HTTP 方法和路径首段保存候选项。Rust 仍在启动时注册处理器、模型绑定和中间件。匹配索引保留原注册顺序。索引缺失、不可读或与当前路由结构不匹配时，使用常规匹配。路由改变后重新运行 `route:cache`，用 `route:clear` 移除索引。

默认路径为 `storage/framework/cache/routes.json`。用 `APP_ROUTES_CACHE` 指定绝对路径或相对应用根目录的路径。使用配置缓存时，将此项设为服务的进程环境变量，因为启动时可能跳过 `.env`。

启动后，`get_cached_routes_path()` 返回解析后的路径，`routes_are_cached()` 表示本次启动是否加载了有效索引。

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

未设置 `CORS_PATHS` 时，`cors.paths` 沿用 默认路径 `api/*` 和 `sanctum/csrf-cookie`。API 路由前缀与 CORS 路径独立配置。若 API 挂载到 `/v1`，可设置 `CORS_PATHS=v1/*,sanctum/csrf-cookie` 为这些路径启用 CORS。设置 `CORS_PATHS=*` 可覆盖所有路径，设置 `CORS_PATHS=` 可关闭 CORS。跨域凭据默认关闭。路径映射可按主机覆盖路径，用数字字符串键表示共用路径。`allowed_origins_patterns` 使用 Rust 正则引擎支持的模式。HTTP 内核在启动时加载并校验策略。

在 `with_middleware` 中调用 `middleware.skip_cors_when(|request| request.is("api/internal/*"));`，可在当前应用中按请求跳过 CORS。任一回调返回 `true` 时，`HandleCors` 将请求交给后续中间件并保留原有响应头。预检 `OPTIONS` 同样适用。

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
    RateLimiter::r#for("uploads", |_| Limit::per_minute(5))
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

用 `middleware_name("throttle:30,0.5,uploads:")` 将额度设为每 30 秒 30 次。只写 `throttle` 时默认每分钟 60 次。`throttle:5|20,1,uploads:` 分别给访客和已认证用户每分钟 5 次和 20 次。访客按路由声明的域名和连接对端 IP 共享额度，已认证请求按认证标识共享额度。为不同规则设置不同的前缀即可分别计数。

使用 `throttle:5|upload_limit,1,uploads:` 可以从认证用户读取 `upload_limit`，访客仍使用固定额度。数据库用户提供器可读取字段。应用模型可通过 `Authenticatable::get_auth_attribute` 暴露属性，并委托 `Model::get_attribute_value` 应用访问器与类型转换。额度接受有符号 64 位范围内的非负数和数值字符串。属性缺失或非数字属于配置错误。

### 使用 Redis 节流

在 `with_middleware` 中调用 `middleware.throttle_with_redis()`，让命名和数字节流别名使用默认 Redis 连接。使用相同 Redis 连接设置和键前缀的进程共享配额。Redis 原子检查并消耗各项额度。使用 `after` 的规则在响应返回后计数。

使用[默认 Redis 集群](../database/redis.md#clusters)时，同一命名策略的计数器位于同一哈希槽，Redis 可以一起检查和计数。不同命名策略可以使用不同的槽。从旧集群键格式升级后，命名策略开启新的计数窗口，旧计数器按原到期时间清理。

## 路由模型绑定

使用 `Route::model::<Post>("post")?` 在应用 Router 注册显式绑定。也可以通过 Router 返回的路由调用 `.model::<Post>("post")?`，为该路由声明模型参数。使用 `request.route_model::<Post>("post")` 读取已解析的模型。模型缺失返回 404，可通过 `missing` 自定义响应。

模型默认按主键查询。可覆盖 `get_route_key_name` 使用 slug，也可为单条路由指定 `{post:slug}`。嵌套的自定义键通过前一个已绑定的模型限定查询范围。使用 `scope_bindings` 为默认键开启作用域，或通过 `without_scoped_bindings` 关闭。子模型通过 `resolve_child_route_binding` 选择类型化关系。`with_trashed` 可包含软删除记录。默认 `web` 和 `api` 组在处理器前执行绑定。

## 重定向路由

使用 `Route::redirect` 注册重定向并指定 HTTP 状态。`Route::permanent_redirect` 返回 301。`/articles/{id}` 这样的目标可以复用匹配路由的参数。

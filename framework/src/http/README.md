# HTTP 能力

OpenRust 框架基于 `axum` 和 `tower` 构建的 HTTP 层。

- [`HttpPlugin`]：通过 `app.plugin(HttpPlugin)` 注册 `serve` 命令、默认 middleware 栈、存活探针 `/health` 和就绪探针 `/ready`。
- [`AppBuilderHttpExt::routes`]：应用添加路由的入口。
- [`AppBuilderHttpExt::http_layer`]：在所有路由外层注册共享 Tower 请求上下文，并置于路由级 layer 之外。
- [`Routes`]：基于 `axum::Router` 的轻量路由约定，提供 `get` / `post` / …、`nest(prefix, …)`、`merge`、`layer`。
- [`Resource`] 与 [`ResourceActions`]：只注册实际存在的 controller action；`Routes::resource` 将 `list` / `create` 映射到集合路由，将 `get` / `update` / `delete` 映射到 `{id}`。`resource_with` 可通过 `only` 或 `except` 过滤 action。
- [`Created<T>`] 与 [`NoContent`]：成功响应封装；前者可选携带 `Location`（semantic 模式为 201），后者返回省略 `data` 的成功包装（HTTP 200）。
- [`PageParams`] 与 [`Paginated<T>`]：校验 `page` / `per_page` 查询参数，并返回含数组 `data` 和分页 `meta` 的 API 封装。
- 统一响应使用 `ApiResponse<T>`；错误响应额外包含 `request_id` 和 RFC 3339 UTC 字符串 `timestamp`（毫秒精度），成功响应不重复包含诊断字段。
- JSON 业务 API 统一挂在 `/api` 下，包括根 API `GET /api` 与其子路径 `/api/*`；使用框架 `Json`、`ApiResponse` 等响应类型时，成功和错误都遵循统一 JSON 包装。`/health`、`/ready` 与可选 `/metrics` 是独立运维端点，不属于业务 API。
- 公开站点由 `[server].public_dir` 指定，默认是应用工作目录下的 `public/`；其他文件从站点根按 URL 路径提供。`[server].root_target` 默认 `file:/index.html`，也支持 `forward:/path`、`redirect:/path` / `redirect:https://host/path`，以及启用 `view` feature 并注册 `ViewPlugin` 后的 `view:index.html`。视图模板放在 `[view].directory`（默认 `views/`），静态资源仍放在 `public/`。`APP_SERVER_PUBLIC_DIR` 和 `APP_SERVER_ROOT_TARGET` 可在 `.env` 或进程环境覆盖 TOML；进程环境优先。生产部署不得将包含 `.env` 的应用根目录设为静态公开目录。API 与 `/ws/*` 的未匹配路径保留框架 404 响应，不回落到静态文件。
- [`SortParams`]：只按显式字段白名单解析 ADR-008 定义的逗号分隔排序语法；应用将字段名映射为固定 ORM 列。
- [`Json<T>`]、[`Path<T>`] 与 [`Query<T>`]：兼容 axum 的 extractors，并将拒绝结果包装为 API 响应。`Json<T>` 还实现了 `IntoResponse`。
- [`Validated`]：在 OpenRust 的 `Json<T>` 或 `Query<T>` extractor 成功后运行 `validator`。
- [`Inject`]：提取由 `AppBuilder::provide` 注册的服务。
- [`ClientIp`]：默认解析 socket peer；只有来自已配置可信代理 IP/CIDR 的请求才接受 `X-Forwarded-For`。
- [`RateLimitLayer`]：通过已注册的共享 `RateLimiter` 对选定路由限流。IP key 遵循 `ClientIp` 的可信代理规则；认证用户 key 需要外层 `AuthLayer`；自定义 key 函数可取得请求及策略 scope。
- [`middleware::apply`]：依次应用 request ID、tracing span、CORS、timeout、gzip/br 压缩和 panic 捕获。`build_router` 会在路由 layer 执行前将最终 `AppState` 注入请求扩展，使能力 middleware 能使用应用注册表。启用的 CORS 配置无效时，会在开始监听前返回配置错误。

`/health` 是进程存活探针；`/ready` 要求 listener 正在接收流量且所有依赖探针成功，否则返回 503。DatabasePlugin 检查每个已配置连接池，CachePlugin 对每个 store 执行只读读取探针；应用插件也可通过 `Readiness::register_probe` 注册最多 64 个稳定命名的异步探针。每轮最多并发 16 个探针、总时限 2 秒且全局仅允许一轮并发检查；错误详情不返回给请求方，服务端只记录稳定错误码和探针名。探针须只读且有界。该 HTTP 端点不代表 gRPC health、实时连接或任意外部依赖的状态。收到 SIGINT/SIGTERM 后，服务器先将自身标为 not-ready 并停止接受连接，再按 `[server].shutdown_timeout_secs` 等待在途请求；到期会合作取消请求、强制终止剩余 server task 并以 `http_shutdown_timeout` 失败退出。请求取消会丢弃本地 handler future，不能撤销已提交的数据库或外部副作用。

启用 `metrics` Cargo feature 后，HTTP 请求会记录请求数、耗时和在途数；指标标签只包含固定的 HTTP 方法类别和状态类别，不包含路径、请求标识、用户信息或错误内容。`/metrics` 默认不注册。仅当 `[server].metrics_endpoint_enabled = true` 时才开放 Prometheus scrape 端点；生产部署应通过监听网络、反向代理或防火墙限制采集访问，不要将端点公开给普通客户端。日志中的 HTTP 请求 span 只记录方法，不记录完整 URI、查询参数、request ID 或 `traceparent`。

`[server].max_concurrent_requests` 默认是 1024，允许配置范围为 1..=65536。达到上限时新请求立即收到 503，不排入等待队列。许可覆盖响应体读取生命周期：body 读完、读取失败或客户端丢弃响应时释放；未消费且仍持有的流式响应会继续占用许可。

`[server.compression] enabled = true` 会根据 `Accept-Encoding` 协商 gzip 和 Brotli；设置为 `false` 可关闭压缩。`[server.cors]` 默认禁用。启用时，将 `enabled` 设为 `true`，并配置 `allowed_origins = ["https://example.com"]`（未启用 credentials 时也可用 `["*"]`）。Methods、request headers、exposed headers、credentials 和预检 `max_age_secs` 均可配置。具体 origin 必须是 `http://` 或 `https://`，后接主机和可选端口，不可包含路径、查询参数或尾部斜杠。CORS 列表中不能混用通配符和具体值；启用 credentials 时，methods、request headers 和 exposed headers 也不能使用通配符。路由构建时若发现配置无效，会返回包含配置 key 和转义值的错误。CORS layer 包裹 timeout layer，因此 408 响应仍保留允许来源信息。`build_router(&App)` 返回 `Result<Router>`；`serve` 会先构建 router，再绑定 listener。

`[server].trusted_proxy_ranges` 默认为空。条目接受 IP 地址或 CIDR。若直接 socket peer 位于配置范围内，`ClientIp` 会解析 `X-Forwarded-For`，从右向左遍历可信代理链，并返回第一个不可信地址；否则忽略转发头。可信范围配置无效时 router 构建失败；来自可信 peer 的转发值格式错误时返回 bad-request 响应封装。

使用 route layer 可应用共享固定窗口配额。应用必须注册 `RateLimiter`，通常通过 `DatabasePlugin` 注册；存储故障会失败关闭。每个 scope 应对应一个 endpoint/policy，并在所有路由实例中使用相同的 limit/window。`by_user` 要求先添加认证 layer，使其先于限流 layer 执行；允许匿名请求的路由也可使用 `by_user_or_client_ip`。

```rust,ignore
use std::time::Duration;
use openrust::http::{AuthLayer, AuthMode, RateLimitLayer, Routes};

fn routes(routes: Routes) -> Routes {
    routes.get("/api/profile", profile)
        .layer(RateLimitLayer::by_user("profile:read", 60, Duration::from_secs(60)).layer())
        .layer(AuthLayer::required(AuthMode::Jwt))
}
```

每个后续 route layer 都会包裹先前的 layer。在此示例中，`AuthLayer` 位于最外层并先执行，之后限流 layer 读取经过验证的账户身份。只有应用了该 layer 的路由会受限；除非显式包裹，health 与 fallback 路由不受影响。自定义 key 函数应返回有界、稳定且不含秘密的身份值。

```rust
use openrust::http::{AppBuilderHttpExt, Inject, Json, Routes, Validated};
use openrust::core::{AppBuilder, Result};
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
struct CreatePost { #[validate(length(min = 1, max = 120))] title: String }

struct Posts;

async fn create(Inject(_posts): Inject<Posts>, Validated(Json(input)): Validated<Json<CreatePost>>) -> Result<Json<String>> {
    Ok(Json(input.title))
}

fn register(app: &mut AppBuilder) {
    app.insert(Posts).routes(|r| r.nest("/api", |api| api.post("/posts", create)));
}
```

Resource controller 会显式列出 handler；未实现的 action 不会创建路由。过滤器会在 controller 注册 action 后应用：

```rust
use openrust::http::{Action, Resource, ResourceActions, Routes};

struct Posts;
async fn list() -> &'static str { "posts" }
async fn get() -> &'static str { "post" }

impl Resource for Posts {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions.list(list).get(get)
    }
}

fn routes(routes: Routes) -> Routes {
    routes.nest("/api", |api| {
        api.resource_with::<Posts>("/posts", |a| a.only(&[Action::List]))
    })
}
```

注册集合路由和 `{id}` 成员路由前，Resource path 会去掉尾部斜杠；`nest("/posts", …)` 中的空路径会响应 `/posts` 和 `/posts/{id}`。多次调用 `only` 会保留 action 列表的交集。经 `AppBuilderHttpExt::routes` 组装时，重复路由（包括相同路径位置上的成员参数名不是 `id`）会由 `build_router` 转为 `http_route_registration_failed` 启动错误；直接使用 Axum 构建 API 时仍由调用方处理路由冲突。

`PageParams` 默认为 `page=1&per_page=20`；直接提取时接受 `page >= 1`、`1 <= per_page <= 100`，且 offset 不得超过 `i64::MAX`。无效值会返回包含字段级 `errors` 的 `validation_failed` API 封装。`Query<PageParams>` 只负责反序列化，不会校验这些边界。`offset()` 从 0 开始，`limit()` 返回页大小。`Paginated::new(items, params, total)` 使用类型化 `PageMeta` 输出 `{"success": true, "code": "ok", "message": "ok", "data": [...], "meta": {"page": 1, "per_page": 20, "total": 57, "total_pages": 3}}`。

### 数据输出与查询边界

应返回应用 DTO，而不是直接序列化 SeaORM model。DTO 本身就是响应字段白名单；若 model 包含密码哈希、内部备注或其他私有列，切勿为其派生 `Serialize`。使用带 `#[serde(deny_unknown_fields)]` 的类型化 query struct，让拼写错误或不支持的过滤字段直接失败。将每个过滤字段映射到固定的 SeaORM 列，并在排序映射列之前校验 `sort`：

```rust,ignore
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PostQuery {
    status: Option<String>,
    sort: Option<String>,
}

#[derive(serde::Serialize)]
struct PostResponse {
    id: i64,
    title: String,
}

let sort = SortParams::parse(query.sort.as_deref(), &["created_at", "title"])?;
let mut select = post::Entity::not_deleted();
if let Some(status) = query.status {
    select = select.filter(post::Column::Status.eq(status));
}
for field in sort.fields() {
    let order = match field.direction() {
        SortDirection::Ascending => sea_orm::Order::Asc,
        SortDirection::Descending => sea_orm::Order::Desc,
    };
    let column = match field.name() {
        "created_at" => post::Column::CreatedAt,
        "title" => post::Column::Title,
        _ => return Err(openrust::Error::BadRequest("unsupported sort field".into())),
    };
    select = select.order_by(column, order);
}
```

`SortParams` 会拒绝不支持、重复、格式错误或过长的值，且不会回显请求内容。它不负责生成 SQL 或决定授权。Query DTO 字段构成过滤白名单；应显式构造 SeaORM `Condition`，不要将请求字段名插入 SQL。

`openrust generate model post --soft-delete title:string` 与对应的 `generate migration posts --soft-delete title:string` 会添加可空 `deleted_at` 和显式的 `Entity::not_deleted()` scope。该 scope 必须主动使用；直接调用 `Entity::find()` 或原始 SQL 仍会包含已删除记录。恢复记录时，在 `ActiveModel` 中将 `deleted_at` 设为 `None`。两个命令都可使用 `--uuid-id` 选择 `UUIDv7` 主键；现有项目仍默认使用整数 ID。

关联关系应通过 SeaORM `Relation` / `Related` 为生成的外键定义边，然后使用 `Entity::find_with_related(related::Entity)` 预加载集合；也可先分页读取父记录，再调用 SeaORM 2 的 `load_many(related::Entity, db)`。不要在循环中逐条查询每个父记录。应为每个相关 entity 显式应用授权、软删除和租户条件；关系加载不会推断这些应用策略。批量加载前先分页父记录，以限制父子两侧的处理量。

请求解析遵循 [ADR-008](../../../.agents/project/decisions/adr-008-api-conventions.md)：格式错误的 JSON、path 或 query 使用 `bad_request`（semantic 模式 HTTP 400）；缺少 `Content-Type` 或类型不是 JSON 时使用 `unsupported_media_type`（415）；结构有效但缺少字段或字段类型错误的 JSON 使用 `validation_failed`（422），并尽可能提供可读 `message` 和字段 `errors`；超过 axum 默认 2 MiB 限制的请求体使用 `payload_too_large`（413）。校验规则失败同样使用 422。未匹配路由使用 `not_found`（404），不支持的方法使用带 `Allow` 的 `method_not_allowed`（405），请求超时使用 `request_timeout`（408）。括号内的 HTTP 状态码仅在 `semantic` 模式适用。这些响应均使用带 `x-request-id` 的 `application/json` 封装；启用 CORS 时也会应用对应头部。CORS 预检请求（含 `Access-Control-Request-Method` 的 `OPTIONS`）无论允许或拒绝都保留 tower-http 的空 body；浏览器只使用协议响应头。普通跨域错误仍会包装，并为允许的 origin 返回 CORS 头。Handler 生成的客户端错误保留自身 message。原生 axum extractors 仍可通过 [`axum`] 使用。

`Created::with_location("/api/posts/42", Json(post))?` 会返回 `Located<T>` 响应。无效的 `Location` header 值会返回一个渲染为 `internal_error` 封装的 `Error`（semantic 模式 HTTP 500）；构造过程不会 panic。不需要 location 时仍可使用 `Created(Json(post))`。

### 出站 HTTP 客户端

启用可选的 `http-client` feature 后即可使用 `openrust::http::HttpClient`。克隆 client 会共享连接池。默认连接超时为 5 秒、总超时为 30 秒、响应 body 上限为 1 MiB，且不会自动跟随重定向。支持 JSON、URL 编码表单、multipart 和流式请求体；响应状态会作为数据返回，不会当成传输错误。`HttpResponse::next_chunk` 在流式读取响应时会持续检查总 body 上限。丢弃请求 future 会取消本地等待，但无法撤销远端服务器已经收到的工作。

```rust,ignore
use openrust::http::{HttpClient, HttpClientConfig};

let client = HttpClient::new(HttpClientConfig::default())?;
let response = client.get("https://api.example.test/status").send().await?;
if response.status().is_success() {
    let value: serde_json::Value = response.json().await?;
}
```

client 默认不重试。应用如需重试，必须显式通过 `idempotent()` 标记操作并传入 `RetryPolicy`；策略最多尝试五次，对传输故障、429、502/503/504 使用有上限的指数退避与全抖动，并遵循受 `max_delay` 和 `retry_after_cap` 约束的秒数或 HTTP 日期 `Retry-After`。需要重放时会拒绝只有一次机会的流式请求体。重定向默认关闭以避免目标被隐式改变，但这不能替代应用侧的 SSRF 目标校验。系统代理环境变量会被忽略；确实需要代理时，请显式配置 `HttpClientConfig::proxy`。`HttpTransport` 和 `TransportResponse::from_bytes` 可用于确定性 fake；注入的 transport 接收构造好的请求，除非其自身选择联网，否则不会访问网络。传输和解码错误会隐藏底层详情；应用不应记录 authorization header 或敏感 body。请显式检查返回状态并处理非 2xx 响应。

### 具名依赖隔离与熔断

调用共享外部服务时，可为每个服务创建具名 `DependencyClient`。同一 `HttpClient` 克隆及同名、同策略的入口共享并发许可、等待队列与熔断状态；同名策略不一致会在创建时返回配置错误。容量和等待数分别有上限，等待超时计入请求总时限；取消会释放等待名额和并发许可。响应正文消费完成或响应对象释放后才归还并发许可。熔断按 transport/timeout 错误和 5xx 响应计失败，4xx 计成功；状态仅在当前进程内共享。

```rust,ignore
use openrust::http::{
    CircuitBreakerPolicy, DependencyPolicy, HttpClient, HttpClientConfig,
};
use std::time::Duration;

let client = HttpClient::new(HttpClientConfig::default())?;
let payments = client.dependency("payments", DependencyPolicy {
    max_in_flight: 16,
    max_waiting: 32,
    circuit_breaker: Some(CircuitBreakerPolicy {
        failure_threshold: 5,
        reset_timeout: Duration::from_secs(10),
        half_open_max_calls: 1,
    }),
})?;
let response = payments
    .get("https://payments.example.test/health")
    .send()
    .await?;
```

`circuit_breaker: None` 时仍执行并发隔离。半开状态只允许配置数量的探测调用；取消探测会释放名额。每个名称仅在进程内限流，多个 API 副本要共享容量需由部署层实现。`DependencyClient` 支持 GET、POST 与通用 method，并提供 JSON、body、headers、query、bearer auth；响应 body 读取期间仍占用许可。

### 受限目标

处理用户提供的 URL 或远程读取时，可选择 `HttpClient::restricted`。普通 `HttpClient::new` 保持原有行为。受限客户端默认拒绝所有主机，必须通过 `allow_host` 登记目标；仅允许 HTTP/HTTPS 和 80/443 端口、拒绝 URL 用户信息，并在 reqwest 实际连接使用的 DNS resolver 中检查全部解析地址。混合公网与私网结果会整体拒绝，避免连接器再做一次未校验解析。自动重定向仍关闭，受限模式不接受显式代理。

```rust,ignore
use openrust::http::{HttpClient, HttpClientConfig, HttpTargetPolicy};

let mut policy = HttpTargetPolicy::new();
policy.allow_host("api.example.com")?;
let client = HttpClient::restricted(HttpClientConfig::default(), policy)?;
let response = client.get("https://api.example.com/health").send().await?;
```

内网目标必须按主机、网段和端口显式登记：

```rust,ignore
use openrust::http::{HttpClient, HttpClientConfig, HttpTargetPolicy};

let mut policy = HttpTargetPolicy::new();
policy.allow_internal("files.internal", "10.20.0.0/16".parse()?, [8443])?;
let client = HttpClient::restricted(HttpClientConfig::default(), policy)?;
```

调用 `allow_host` 后，白名单会限制所有请求的精确主机名；它不会绕过解析地址检查。每个内网主机仅可登记一次 CIDR 与端口集合，且该主机只接受登记端口。受限请求不能自定义 `Host` 头，也不能通过 `with_transport` 接入自定义 transport。此策略只约束经该客户端发出的请求，不约束 SeaORM、存储驱动、应用自行创建的客户端、网络 NAT 或透明代理，也不能替代部署侧出口规则。

```rust,ignore
use std::time::Duration;
use openrust::http::{HttpClient, HttpClientConfig, RetryPolicy};

let client = HttpClient::new(HttpClientConfig::default())?;
let policy = RetryPolicy::new(3, Duration::from_millis(100), Duration::from_secs(1))?;
let response = client
    .get("https://api.example.test/idempotent-resource")
    .idempotent()
    .send_with_retry(policy)
    .await?;
```

`[api] http_status = "always_ok"` 是默认值：所有框架 JSON 封装都返回 HTTP 200。`semantic` 沿用 ADR-008 的状态码表。middleware 会将选定模式和 request ID 保存在请求级 task context 中，使成功响应、提取错误、fallback、timeout 和 panic 响应遵循同一策略。JSON `/health` 响应也使用相同封装；文件、流和 HTML 等非 JSON 响应保持原始格式。

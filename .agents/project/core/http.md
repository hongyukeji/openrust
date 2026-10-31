# `openrust::http`

> 受众：AI Agent
> 更新：2026-10-31
> 摘要：HTTP 层当前事实。

Session请求层已接入统一领域；D1限定验收范围、真实网络/双库证据和完整后继缺口见 [Session当前事实](session.md) 与 [ADR-082](../decisions/adr-082-session-store-and-flash.md)。旧HTTP/Cookie子簇不自动证明整个Web栈完成。

## 边界

基于 `axum` 0.8 + `tower-http`。提供路由约定、提取器、默认中间件、`/health` 与 `serve`；`[api].http_status` 默认 `semantic`，统一 JSON envelope 保留 HTTP 状态，只有显式 `always_ok` 才映射为 200。core 已提供 `RateLimiter`，DatabaseServiceProvider 提供双库共享 store，AuthFlows 与显式 `RateLimitLayer` 已复用它。OpenAPI schema 与 `/docs` 尚未实现。

## D7验收边界

真实请求Route/Resolver与受管理`RouteMethods`已按[ADR-083](../decisions/adr-083-matched-route-context.md)完成限定验收；D7真实Route/Resolver与受管理方法声明已通过限定验收：final3完整12门禁、默认695/29 ignored、全特性1403/103 ignored、根PG18/MySQL8.4各64、fresh5/2/2、真实TCP7及不同模型复审；忽略项不算真实服务。根target清零，五旧consumer不变，限定提交以Git为准；完整Routing/Request与previous/Precognition仍开放。[证据](../memory/testing.md#c04-d7-真实路由上下文限定子范围已验收)。旧R3-E/F中与新契约冲突的任意方法fallback、方法miss优先于fallback和域/path同名拒绝已纠正，旧验收记录只保留历史事实。

## D8 限定实现与验收边界

依据 [ADR-084](../decisions/adr-084-session-previous-and-precognition.md)，`ManagedHandler` 在参数提取后处理确认的 Precognition 请求，成功返回空 204 和成功头，不调用业务函数；原生 Axum 逃生入口不自动提供此保证。内置 `precognitive` 别名需由路由显式启用，客户端头只表示尝试，不能自行确认预检。

`Validated<Json<T>>` / `Validated<Query<T>>` 使用同一 `Validate::rules()`、`Rules` 与 `Rule` 执行器。`Validate-Only` 在原始输入上筛选既有规则，成功或失败都会提前结束；后置提取器不会执行。访问控制应放在路由 middleware 或 `Validated` 之前，唯一模块采用 middleware 路径；不能承诺任意签名中的后置鉴权都执行。完整 FormRequest、规则集与 CSRF 仍是后继范围。

当前稳定输入下 workspace 全特性1439 passed/104 ignored，fmt、默认/全特性严格Clippy、Rustdoc、feature检查与deny已通过；忽略或缺URL返回不算真实服务。根 PostgreSQL18/MySQL8.4已分别实际通过模块HTTP67项与 Session previous 精确1项；测试数据库清理所有权回归PG3/My2通过。Rust1.97全特性全目标check亦已通过；新生成应用普通5/双库各2项、真实TCP6项与最终CLI精确快照1项均已通过；仅关闭D8限定范围，不关闭完整HTTP能力。

## D9 旧输入（限定子范围已验收）

`Request::flash/flash_only/flash_except/flush`通过同一输入仓储调用Session；`RequestInfo::old/old_or/old_all`区分缺失/null/默认。无Session可读取空值或默认，写入报错；手动flash不默认剔除密码。单次预算原子、空筛选及预检保存边界见[Session](session.md#d9-旧输入限定子范围已验收)。

唯一模块、生成helper、main门禁、真实双库和TCP已完成[限定验收](../memory/testing.md#c04-d9-main-组合限定子范围验收通过)。文件输入合并、RedirectResponse与CSRF/FormRequest仍开放。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `provider` | `HttpServiceProvider` 在 register 登记 server schema，在 boot 贡献 `serve`、`/health` 与 `/ready`；启用 `metrics` feature 并配置 `[server].metrics_endpoint_enabled` 后注册 `/metrics`。应用 Provider 在 `BootContext::routes` 贡献业务路由；`build_router(&App) -> Result<Router>` 向请求扩展注入最终 `AppState` 与请求 `Scope`，供 Inject/Auth/RateLimit 使用；经应用路由组合发现的 Axum 冲突转换为 `http_route_registration_failed` 启动错误，直接调用底层 Axum API 不在转换范围 |
| `site` | `[server].public_dir` 默认 `public`，`tower-http::ServeDir` 将站点目录映射至 URL 根；`root_target` 支持 `file:`、`forward:`、`redirect:`，以及启用 `view` feature 并注册 `ViewServiceProvider` 后的 `view:`。视图首页在 router 构建时验证模板。D10-C当前修复在所有业务候选均未命中且无方法 miss 时，先将根 GET/HEAD 交站点目标，再考虑用户 fallback/静态首页；默认 `file:/index.html` 保持已有静态优先、缺失时用户 fallback 的组合，同值显式配置不区分来源；业务 handler 的404/405仍为终态，方法 miss 仍沿既有 fallback/Allow 规则。内部转发用私有目的 URI 分派 fallback/静态资源，保留 RequestInfo 的外部 OriginalUri；HEAD站点出口只保留status/headers/extensions并置空drop正文，不poll流或补Content-Length；站点外层middleware短路不在该保证内。默认首页/fallback、内部forward与HEAD修复已通过不同模型审查；v9当前router106项、view-only6项与view-session HTTP7项及适用main门禁实际通过，HEAD元数据/流2项按等价证明复用。fresh普通7项/MySQL认证2项通过，公开consumer1项及最终no-session/MySQL8.4.13 TCP已通过，自有库/端口/敏感文件清理完成，已由 `784045c` 限定提交；默认 `/api/*` 和 `/ws/*` 未匹配请求保留框架 JSON 404，不回落到静态文件；应用显式注册 fallback 后可接管这些未匹配路径，GET/HEAD 先尝试静态资源 |
| `routes` | `Routes`：`route(RouteMethods)` / `get` / `post` / `put` / `patch` / `delete` / `head` / `options` / `query` / `on` / `any` / `name` / `resource` / `resource_with` / `fallback` / `group(RouteGroup, f)` / `nest(prefix, f)` / `merge` / `layer` / `middleware_alias` / `middleware_group`；每层 router 仅允许一个 fallback，重复注册稳定报错；组内 fallback 限定组前缀并继承组 middleware，保留完整 `OriginalUri`。GET/HEAD 未匹配请求先尝试静态资源，miss 后将原请求完整传给 handler；显式fallback仅GET/HEAD且参与alternate判断，OPTIONS有候选时合成200、Allow及真实Route，其余方法按候选返回405。D7限定子范围已验收。`RouteGroup::domain` 支持继承/覆盖及 domain-first 候选，内部 miss 保留原请求与 body；`DomainParams<T>` 独立提取 DNS 标签，业务 404/405 不会触发二次派发。Host/URI authority 冲突失败关闭，忽略转发 Host。域 URL 使用登记模式与单标签参数，scheme/port 来源为 `app.url`。`BootContext` 可登记可复用 Tower middleware alias 与递归 group，`RouteGroup` 和 `Routes` 按名称引用，未定义/重名/循环显式报错；没有应用注册表时引用会在公开 `Routes::into_router() -> Result<_>` 或 `build_router` 失败，不静默忽略；直接 Tower Layer 入口保留。`RouteGroup` 可组合路径前缀、原样组合 `name_prefix` 并按声明顺序限定 middleware；`nest("")` 或 `"/"` 走 merge；`RouteUrlGenerator` 从 `app.url` 生成具名绝对 URL，不采信请求 Host |
| `route` / `routing` | D7公开`Route` owned快照、`RouteResolver`与`RouteMethods`；`RequestInfo`和`Request`读取真实名称、参数/default与名称通配。默认slot逐请求共享，应用resolver可替换；内置404/405/静态直出无业务Route，业务错误和route middleware短路保留。受管理方法包括QUERY、自定义方法及固定Laravel八种any；原生Axum入口不自动提供上述保证。D7限定子范围已通过完整门禁；可选参数、声明defaults/约束及模型/枚举绑定仍开放 |
| `resource` | `Resource` 显式登记动作；`#[openrust::resource]` 从 controller inherent impl 的动作关联函数生成实现；`only` 连续调用取交集、`except` 裁剪；路径去尾斜杠后注册集合与成员，`update` 同时映射 PUT / PATCH |
| `response` | `Created<T>` → 默认 201（显式 `always_ok` 时为 200），`with_location` 返回 `Result<Located<T>>`（非法值 → 500 包装）；`NoContent` → 200 成功包装并省略 `data`；无载荷的错误包装也省略 `data`，已有载荷保持原样；直接提取 `PageParams` 默认 1/20，并校验页数、每页 1..=100 与 offset 不超过 `i64::MAX`；`Paginated<T>` 输出 `data` 与 `meta` |
| `request` | `RequestInfo` 是 parts-only 快照，使用 OriginalUri 路径并与 Json 正文提取共存；host 复用域路由 authority，header/bearer 文本解码失败返回稳定错误，Debug 不暴露 URI/头。协商按 ADR-078 固定 Laravel 13.39/Symfony 8 的参数规范、质量排序、格式别名与匹配方向；headers 64KiB/128值、Accept 16KiB/128项超限拒绝。完整输入/Session/FormRequest 不由此簇覆盖 |
| `request` URL（C04-D6 限定子范围已验收） | Info提供请求origin、完整URL、路径解码/分段、星号匹配与prefetch；拥有唯一query bag的Request提供增删query。原始OriginalUri规范化与可变query来源分开，JSON来源优先于GET/HEAD；地址沿现严格authority和实际/可信proxy proto快照，不从URI https推TLS。128KiB输出及UTF-8适配见[ADR-078](../decisions/adr-078-request-inspection.md)。真实PHP两线差分、完整12门禁、唯一模块、fresh5/2/2、根双库各63与真实TCP/TLS代理10场景均通过，详见[testing](../memory/testing.md#c04-d6-request-url限定子范围已验收)；不关闭完整Request或Session。 |
| `request` 输入（C04-B1 已验收） | body-owning `Request` 复用RequestInfo和query/json/form bags；typed Json/Query共享默认trim/null策略，原文Bytes不变。InputNormalization作为Tower Layer或extensions覆盖；PHP bracket/键规则、动态读取修改及typed query转换按ADR-079，multipart/Session/高级typed helpers仍未由本簇交付。 |
| `request` 类型读取（C04-B2 已验收） | Request与Fluent复用下层support的data/scalar内核；Stringable、保键Collection、BackedEnum与InputNumber承载类型读取，条件结果明确区分主callback假值回自身与default直接返回。Collection JSON边界拒绝非有限浮点，标量cast仍保留；HTTP trait实现留在HTTP层，无support反向依赖。日期/区间已接公开入口，CalendarContext来自最终app.timezone与现extensions覆盖，每次date读clock；FixedClock对应Carbon test-now、SystemClock使用生产语义，两种参考分列。固定source scanner、POSIX尾规则、格式字节/错误边界与公开入口已通过本簇定向、workspace/feature/MSRV、真实双库、生成应用、TCP和不同模型复审；不代表完整Carbon工具箱或完整C04验收。 |
| Cookie（C04-C 已验收） | [ADR-081](../decisions/adr-081-request-response-cookies.md)：独立wire解析、direct-key bag、请求隔离CookieJar、值工厂及现响应组合；队列先附加再复用Encryptor加密，旧key解密/当前key写入、叶名认证、错误与预算边界明确。最终Session配置延迟装配、原始凭据重复检查、可信transport和空domain响应identity已回归；workspace/feature/MSRV/deny/docs、根PG18/MySQL8.4各48项、三个生成项目及真实TLS均通过，证据见[testing](../memory/testing.md)。显式plain不提供保护，不代表完整Laravel web默认栈、Session/flash/CSRF已完成。 |
| Cookie/Session保护接线（C04-D4限定范围已验收） | Cookie中间件向请求写入无密钥的私有保护事实，Session公开raw context构造不能自报可信；实际认证与最终保存核完整性。Cookie/Session机制自身的prepare/save/保护/预算错误响应统一no-store与pragma no-cache，不搬旧响应Cookie、长度或正文；正常响应不变。真实Router红绿、双库Auth与TLS终止代理/HTTP浏览器证据按[testing](../memory/testing.md)分别记录，D4完整门禁/生成在47cd7b8限定子范围已收口；完整Session仍未完成。 |
| `extract` | `Inject<T>`（从 `AppState` 取服务，缺失 → 500 包装）；OpenRust `Json<T>` / `Path<T>` / `Query<T>`（解析失败 → 400/415/422/413 包装），`Validated<Json<T>>` / `Validated<Query<T>>`（先解析，规则失败 → 422，嵌套路径 `a.b[0].c`） |
| `middleware` | `apply(router, &ServerConfig) -> Result<Router>`：请求 ID → 传播 → `TraceLayer` span（不记录 URI、请求 ID 或 traceparent）→ 可选 CORS → 超时 408 包装 → 可选 gzip/br 压缩 → `CatchPanic` → 500 包装；启用 `metrics` feature 时记录固定方法/状态类别、耗时和在途请求数；404 fallback、405 fallback（保留 `Allow`）、413 包装；启用 CORS 时在组层前逐项校验配置；响应上下文同时建立 request ID/locale/deadline/cancellation 的 `ExecutionContext`，非法 request ID 移除后由 middleware 生成新值 |
| `rate_limit` | `RateLimitLayer` 显式包裹路由并调用 AppState 中注册的 `RateLimiter`；key 支持可信客户端 IP、（启用 auth 时）已验证用户/匿名回退及自定义策略；store 错误关闭，429 沿用统一错误与 `Retry-After`。未包裹路由、health/fallback 不自动限流 |
| `fallback` | `Routes::fallback` 仅处理未匹配路由；组级 fallback 限定前缀并遵循组 middleware，GET/HEAD 先查静态资源，保留路径下默认仍返回框架 JSON 404。重复静态完整 scope 稳定报 `http_fallback_duplicate`；参数化/通配符组前缀优先报 `http_fallback_scope_unsupported`，避免 Axum catch-all panic |
| `sort` | `SortParams::parse` 按 ADR-008 `sort=-created_at,title` 语法解析，限制长度/字段数，拒绝重复、未知或格式错误字段；调用方必须提供字段 allowlist，再显式映射到 SeaORM 列。过滤器应使用 `deny_unknown_fields` 的输入 DTO 并构造显式 `Condition`，输出用响应 DTO 做字段 allowlist |
| `health` | 独立运维路径 `GET /health` → success `ApiResponse<Health>`，`data.status = "ok"`；`GET /ready` 在 listener 可接流量且依赖探针成功时为 200，否则 503。业务 JSON API 根为 `GET /api`，子路由统一使用 `/api/*` |
| 并发边界 | `server.max_concurrent_requests` 默认 1024、范围 1..=65536；饱和时快速返回 503，不排队。许可覆盖 response body 到 EOF、读取失败或丢弃；未消费的长流持续占用许可 |
| `serve` | `serve(&App)` 绑定 `[server] host:port`，收到 SIGINT/SIGTERM 后停止接入并按 `shutdown_timeout_secs` drain；到期取消请求并强制 join server task，返回 `http_shutdown_timeout`；`ServeCommand` |

`pub use axum` 供门面 re-export。

CORS 具体 origin 仅接受 `http://` 或 `https://` 加主机及可选端口，不接受路径、查询或尾部 `/`；错误包含配置键和转义后的原值。方法、请求头与暴露头的 `"*"` 不能与具体值混用，启用 credentials 时三者均不得使用 `"*"`。CORS 位于超时层外，408 响应保留允许来源头。

CORS 预检（`OPTIONS` 且带 `Access-Control-Request-Method`）保持 tower-http 的空正文，普通跨域错误仍为包装响应并携带允许来源头。分页 `meta` 使用 `PageMeta`。

## 后续边界

- 路由限流仅覆盖显式包裹的路由；默认不覆盖 health、fallback 或其他能力层。认证限流需确保 `AuthLayer` 在请求方向先执行。
- `SortParams` 仅负责安全解析，不提供 ORM 字段反射、过滤 DSL 或自动授权；应用仍须按资源授权并将排序/过滤白名单映射到查询。DTO/查询与关联加载示例见 [HTTP README](../../../src/http/README.md)。
- OpenAPI schema 与 `/docs`（后续里程碑）；`utoipa` / `utoipa-axum` 尚未引入，统一响应需与生成文档一致。
- `Readiness::is_ready()` 仍只表示 listener 生命周期；`/ready` 聚合 DatabaseServiceProvider 的已连接池、CacheServiceProvider 已解析的 default/named store 和应用显式注册的依赖探针。具名 cache store 不为健康检查提前建立连接。探针最多注册 64 个、每轮最多并发 16 个、总时限 2 秒，失败只向客户端返回通用 503，并只在日志记录探针名与稳定错误码。此端点不覆盖 gRPC health、WebSocket/SSE、跨节点实时投递或任意应用原生依赖；metrics/OTel 已有专项实现，完整生产就绪验收仍待完成。
- request ID 只作为队列 request metadata 与 worker span 的日志关联，不表示 W3C trace context/OpenTelemetry span link；gRPC G2 仍须按 ADR-040 复验 deadline、cancel、locale 与 trace 传播。

## 验证

`cargo test -p openrust --test http_router --test http_resource`：`tests/http/router.rs` 用 `oneshot` 覆盖 health 统一包装及请求 ID、`Inject` 命中与缺失、`Validated` 的 422 / 400 / 200、panic → 500、压缩协商、CORS 响应与配置错误；`tests/http/resource.rs` 覆盖路由动作、裁剪、嵌套、响应状态与分页错误/结构。具名路由、middleware alias/group 与 fallback 定向回归覆盖组名前缀、参数 URL、调用顺序与作用范围、缺失注册表 fail closed、重名/未知/循环错误、fallback 的静态优先/原始 URI/body/组范围与 405 边界；R3-C 至 R3-E 全量证据见 [testing](../memory/testing.md#r3-c-laravel-命名路由与具名-url-生成)、[R3-D](../memory/testing.md#r3-d-laravel-middleware-alias-与-group) 和 [R3-E](../memory/testing.md#r3-e-laravel-fallback-routes)。启用 `view` 时另运行 `cargo test -p openrust --features view --test http_view`。

## 出站 HTTP 客户端

`http-client` 是可选 feature，公开 `openrust::http::HttpClient`、`HttpClientConfig`、`HttpTargetPolicy`、`HttpRequest`、`HttpResponse`、`HttpTransport`、`HttpTransportRequest`、`HttpResponseBody`、`TransportResponse`、`RetryPolicy`、`DependencyClient`、`DependencyPolicy` 与熔断策略。reqwest 0.13 Rustls client 的 clone 共享连接池。默认 connect timeout 5s、total timeout 30s、response read 上限 1 MiB、自动 redirect 关闭、系统 proxy 环境变量不继承（proxy 必须显式配置，Debug 脱敏）。`HttpClient::restricted` 是独立 opt-in：默认拒绝全部主机，登记后仅允许 HTTP/HTTPS、显式端口、无 URL 用户信息、不接受自定义 `Host` 头、精确 host allowlist、全部 DNS 解析结果和公网地址策略在请求/实际解析处分别校验；主机 + 单个 CIDR + 端口集合可以显式允许可信内网，内网登记主机只接受登记端口，代理与 custom transport 不可用。域名解析结果会由同一个 reqwest resolver 传给 connector，禁止二次查询产生的 TOCTOU；IP 字面量在发送前检查。仅此入口受约束，不覆盖原始 SeaORM、存储或任意第三方网络调用。响应状态不自动转为错误；错误文本不暴露 URL/头/正文。明确超限的 `Content-Length` 在返回响应前拒绝，未知长度的 body 通过逐块限额 reader 消费；取消只停止本地 future 等待，不承诺撤销已送达的远端操作。

调用韧性为显式 opt-in：`HttpClient::dependency(name, policy)` 按名称共享有界并发/等待队列和可选熔断；同名配置不一致返回错误。排队、发送和显式重试共用客户端总时限，取消释放队列名额/并发许可；响应正文消费完成或响应对象释放后归还许可。熔断仅进程内共享，transport/timeout 错误及 5xx 计为失败，4xx 与成功响应清除失败计数；本地策略、配置和正文限额错误不计作依赖故障。默认关闭熔断。重试必须通过 `idempotent()` 启用，最多五次，使用全抖动指数退避；只对 429、502、503、504 读取受 `max_delay`、`retry_after_cap` 限制的 `Retry-After`。一次性流 body 不可重放。gRPC 尚未实现，不声称已复用或验证该策略。`HttpClient::with_transport` 支持不受限的测试/应用 transport 注入，自定义 transport 同样受总超时和 body 限额保护。验证命令见 `memory/testing.md` / `memory/verification.md`。

## C04-D10-A 错误包接入

纯错误包由公开 `openrust::support` 提供，唯一模块 POST `/api/example/message-bag` 经正式路由调用；受管理HTTP不默认注入共享错误状态。公开消费、生成及真实TCP限定验收见[testing](../memory/testing.md#c04-d10-a-错误包限定子范围已验收)。这份 D10-A 证据不证明下面 D10-B 的新行为。

## C04-D10-B 请求绑定重定向（限定子范围已验收，已提交a2befef）

`Redirector::from_info(&RequestInfo)` 借用请求来源并构建普通 `to/back` 响应；`Redirector::from_request(&Request)` 另可读取同一规范化输入。`RedirectResponse` 经 `IntoResponse` 输出目标 `Location`、HTML 正文和文本类型，默认 302；允许状态、目标与 HTTP 头失败通过 `Result` 传播。`back` 按有效 Referer、Session previous、有效 fallback、请求根选择；目标解析不使用响应状态推断路由命中。普通重定向不依赖 Session；需要闪存时 `with_input/only_input/except_input` 以及 `with_errors` 使用当前 Session，缺 Session 返回错误。

`with_errors` 支持 `RedirectErrors::Text/Entries/Provider`，将命名包写入 Session `errors`，下一请求经 `RequestInfo::session()?.read_errors()?` 读取；同一请求的旧输入使用 `old_all/old`。手动输入闪存不应用异常处理器 `dontFlash`，敏感字段由业务显式排除；文件输入未并入规范化输入 bag。Session codec 在属性锁外执行，写入候选须通过类型、格式与单次预算校验后才发布；自定义类型通过 Provider 的公开 `ErrorBagCodecs` 扩展登记，内置包无需配置。唯一模块的 `draft_redirect` 路由与 `draft_errors` 服务展示业务调用和跨请求恢复，生成测试在 CLI 模板中提供同类公开消费。

本簇当前源码通过严格默认/全特性门禁、无Session真实TCP、MySQL8.4私有库TCP与缺MFA表启动拒绝；普通生成应用7项含重定向公开helper，MySQL生成2项仅认证回归，PG常规验收保留发布矩阵。证据与旧失败见[限定验收](../memory/testing.md#c04-d10-b-请求绑定重定向与-session-错误包限定子范围已验收)。不据此关闭完整 Redirector、异常 `dontFlash` 或验证失败自动重定向；D10-C请求局部视图共享已另行限定验收并提交 `784045c`，见[view](view.md)。

## C04-D10-D 命名路由重定向（限定子范围已验收，提交见Git）

固定 framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的 `Illuminate/Routing/Redirector.php:152–155`：`route` 调用 URL generator 的 `route` 后交给 `to`，默认状态302、头集合为空。Rust `Redirector::route(name: &str, parameters: &BTreeMap<String, String>)` 保留该顺序；`route_with(name, parameters, status, headers)` 提供显式状态与头，最终复用 `to_with` 的校验、Location/HTML规则。

`RequestInfo` 从请求 extensions 捕获可选 `RouteUrlGenerator` clone，共享同应用既有Arc，不复制索引、不引入全局当前请求；后续替换extensions不改变已捕获快照。调用方沿用 `Redirector::from_info/from_request`，返回值保留原请求借用，继续组合 `with_input/only_input/except_input/with_errors`；普通命名重定向无Session亦可用，闪存仍要求Session。缺生成器沿用 `http_route_url_unavailable`，未知名与缺必需参数沿用 `http_named_route_not_found`、`http_route_parameter_missing`，不改既有错误状态。

URL继续使用 `app.url` 与登记域模式，不采信请求Host；参数仍限 `BTreeMap<String, String>`、必需域/路径段和编码后的额外query。PHP mixed/model/enum参数、可选参数与missingNamedRouteResolver未等价实现；action、guest/intended、signed及完整Redirector/L020.02仍开放，无新依赖或ADR。唯一模块与CLI模板已接真实调用，v2定向、双严格Clippy、main11项、最终构建、fresh普通7项、MySQL8.4.13认证2项及公开helper1项、无Session/MySQL真实TCP均已通过限定验收；提交见Git，见[当前验收](../memory/testing.md#c04-d10-d-命名路由重定向当前验收中未提交)。


## C04-D10-E scheme 与 intended（限定子范围已验收，提交见Git）

沿请求绑定的 `Redirector` 增加 `to_with_scheme`、`secure/secure_with`；协议选项仅影响相对目标，合法绝对或特殊 URL 原样保留，不修改请求快照或配置，不表示启动 HTTPS listener。Session feature 提供 `guest/guest_with`、`intended/intended_or/intended_with` 与原值 `get_intended_url/set_intended_url`，沿现 Session 普通属性而非 flash 保存意图。

固定上游的生产顺序为 guest 先保存再构造响应，intended 先 pull 再转换与构造，失败不撤销既有保存或消费。GET、真实路由、非 `expects_json` 使用完整请求 URL，其余使用 previous；Session truthy previous 保留 JSON 原值，Referer/fallback 经 URL 生成，back 直接响应 previous，不再二次绝对化。缺失使用默认值，显式 null/false/零与空值保持各自语义；数组/对象在消费后返回错误。JSON 对象对应关联数组，不宣称支持任意 PHP Stringable 对象。

唯一模块与生成 helper 已真实接入并验收：定向10/main11、新server/CLI构建、fresh普通7、MySQL8.4.13认证2及额外公开helper1、无Session/MySQL真实TCP4/9场景通过。独立源码/最小修复/验收驱动复审均无剩余问题，Clippy首轮与生成根URL fixture旧失败保留；最后仅模板+快照字面量修复，按固定输入证明复用不受影响运行门禁，新CLI和fresh生成实际重验。资源收尾已完成，限定提交见Git，不关闭完整Redirector/L020.02；[当前证据](../memory/testing.md#c04-d10-e-scheme-与-guestintended验收中未提交)。


## C04-D10-F refresh 与 away（限定子范围已验收，提交见Git）

`refresh/refresh_with` 使用当前请求path后经既有to生成目标，丢弃query，默认302/空头；`away/away_with` 直接交同一响应构造器，保持目标原值。均保留from_info/from_request原借用、无Session可用及输入/错误闪存组合。共享构造器集中校验HTTP范围、空目标、允许重定向状态与类型化头；to按上游先生成目标再构造响应，guest先put/intended先pull的副作用边界不变。Symfony8只作为ADR-078独立固定比较点，不称Laravel已解析依赖。

核心和唯一模块/模板已交付并独审，定向10/main11、新server/CLI、fresh普通7/MySQL认证2+额外公开helper1、无Session/MySQL真实TCP7/12均通过；默认809/34 ignored、全特性1595/108 ignored，忽略项不算真实服务。资源收尾已完成，限定提交见Git。模块POST刷新返回同路径真实GET草稿，首GET读取Session闪存后正常老化；无Session仍可用。外链是受控固定目标，不读取用户URL。完整状态/目标矩阵留核心unit/router，模块、生成与真实TCP分别验证公开接线。执行与证据见当前[todo](../todo.md)。


## C04-D10-G 动作身份与 action URL（限定子范围已验收，提交见Git）

`RouteAction`保留字符串与二元组形态；`get_action/on_action`将动作身份与真实handler同次登记，`Route::action`反映真实匹配元数据。组namespace影响声明规范化，Boot默认根namespace与生成器单实例setter影响查询；clone和已捕获请求的namespace独立，索引保持不可变共享。`RouteUrlGenerator::action`与named route复用域/路径/query渲染，匿名动作应用也装配生成器；`Redirector::action/action_with`先生成再构造响应，延续无Session普通跳转和原请求借用闪存链。

固定PHP实际集合探针7+2案只证明集合索引边界：完整原始拼接键覆盖、声明方法顺序、域优先左侧union及前导反斜线不对称。请求分派仍有结构化(domain,path)与PHP碰撞隐藏路由的差异，见[ADR-083](../decisions/adr-083-matched-route-context.md)及[todo](../todo.md)；混合/位置/模型参数、可选段、relative及完整Controller/URLGenerator未完成。字符串动作空成员边界已修并经不同模型delta复审关闭；最终核心11项在两feature组合、router117/113、双严格Clippy及main11项通过，默认824/34ignored、全特性1611/108ignored，ignored不算真实服务。新server/CLI、fresh普通7/MySQL认证2+公开helper1及真实TCP10/15均实际通过，精确专属包清理完成；原错误fixture/lint/gRPC及生成前置失败保持原记录，范围与复用见[当前证据](../memory/testing.md#c04-d10-g-动作身份与-action-url限定子范围已验收提交见git)。不据此关闭完整L020.02。

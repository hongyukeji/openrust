# HTTP 能力

OpenRust 框架基于 `axum` 和 `tower` 构建的 HTTP 层。

- [`HttpServiceProvider`]：在 `register` 登记 HTTP 配置与能力，在 `boot` 贡献 `serve` 命令、默认 middleware 栈、`/health` 和 `/ready`。
- `BootContext::routes`：应用 Provider 在 `boot` 阶段添加路由。
- `BootContext::http_layer`：在所有路由外层注册共享 Tower 请求上下文，并置于路由级 layer 之外。
- [`Routes`]：基于 `axum::Router` 的路由约定，提供 `get` / `post` / …、`group`、`nest`、`merge`、`layer` 与 `fallback`；`route(path, RouteMethods)` 接受 `routing::{get, post, put, patch, delete, head, options, query, on, any}` 受管理方法声明，可链式追加不同方法。`on` 校验并规范化 HTTP 方法字符串，`any` 仅包含 Laravel 固定的 GET、HEAD、POST、PUT、PATCH、DELETE、OPTIONS、QUERY。[`RouteGroup`] 支持可嵌套路径前缀与组级 Tower middleware。middleware 默认按声明顺序进入请求；命名项按公开优先级调整并去重，组外路由不受影响。
- [`Resource`] 与 [`ResourceActions`]：只注册实际存在的 controller action；`Routes::resource` 将 `list` / `create` 映射到集合路由，将 `get` / `update` / `delete` 映射到 `{id}`。`resource_with` 可通过 `only` 或 `except` 过滤 action。
- [`Created<T>`] 与 [`NoContent`]：成功响应封装；前者可选携带 `Location`（semantic 模式为 201），后者返回省略 `data` 的成功包装（HTTP 200）。
- [`PageParams`] 与 [`Paginated<T>`]：校验 `page` / `per_page` 查询参数，并返回含数组 `data` 和分页 `meta` 的 API 封装。
- 统一响应使用 `ApiResponse<T>`；错误响应额外包含 `request_id` 和 RFC 3339 UTC 字符串 `timestamp`（毫秒精度），成功响应不重复包含诊断字段。
- JSON 业务 API 统一挂在 `/api` 下，包括根 API `GET /api` 与其子路径 `/api/*`；使用框架 `Json`、`ApiResponse` 等响应类型时，成功和错误都遵循统一 JSON 包装。`/health`、`/ready` 与可选 `/metrics` 是独立运维端点，不属于业务 API。
- 公开站点由 `[server].public_dir` 指定，默认是应用工作目录下的 `public/`；其他文件从站点根按 URL 路径提供。根 GET/HEAD 在普通业务路由路径均未命中后，先执行自定义 `[server].root_target`，再考虑用户 fallback；即使 `public/index.html` 存在，也不会覆盖自定义根目标。默认值 `file:/index.html` 保留静态首页模式：存在文件时返回文件，缺失时允许用户根 fallback 接管；显式配置相同值也使用此模式。`[server].root_target` 默认 `file:/index.html`，也支持 `forward:/path`、`redirect:/path` / `redirect:https://host/path`，以及启用 `view` feature 并注册 `ViewServiceProvider` 后的 `view:index.html`。视图模板放在 `[view].directory`（默认 `views/`），静态资源仍放在 `public/`。`APP_SERVER_PUBLIC_DIR` 和 `APP_SERVER_ROOT_TARGET` 可在 `.env` 或进程环境覆盖 TOML；进程环境优先。生产部署不得将包含 `.env` 的应用根目录设为静态公开目录。API 与 `/ws/*` 的未匹配路径保留框架 404 响应，不回落到静态文件。

站点响应边界会在业务或站点处理完成后清空 HEAD 正文，保留状态、全部响应头和扩展；原正文直接 drop，不读取流，也不推断或补入 Content-Length。GET 响应不变。此保证覆盖经过站点 service 的响应；站点外层 middleware 自行短路构造的响应不在此边界内。
- [`SortParams`]：只按显式字段白名单解析 ADR-008 定义的逗号分隔排序语法；应用将字段名映射为固定 ORM 列。
- [`Json<T>`]、[`Path<T>`] 与 [`Query<T>`]：兼容 axum 的 extractors，并将拒绝结果包装为 API 响应。`Json<T>` 还实现了 `IntoResponse`。
- [`Validated`]：普通请求在强类型 `Json<T>` / `Query<T>` 解析后执行公开字段规则；已确认的 Precognition 可在构造完整 DTO 前只验证所选字段。
- [`Inject`]：提取由容器实例或异步工厂注册的服务。
- [`ClientIp`]：默认解析 socket peer；只有来自已配置可信代理 IP/CIDR 的请求才接受 `X-Forwarded-For`。
- [`RateLimitLayer`]：通过已注册的共享 `RateLimiter` 对选定路由限流。IP key 遵循 `ClientIp` 的可信代理规则；认证用户 key 需要外层 `AuthLayer`；自定义 key 函数可取得请求及策略 scope。
- [`middleware::apply`]：依次应用 request ID、tracing span、CORS、timeout、gzip/br 压缩和 panic 捕获。`build_router` 会在路由 layer 执行前将最终 `AppState` 注入请求扩展，使能力 middleware 能使用应用注册表。启用的 CORS 配置无效时，会在开始监听前返回配置错误。

## 请求检查与内容协商

`RequestInfo` 是不消费正文的只读请求提取器，可放在 `Json<T>` 等唯一正文提取器前面。`path()` 返回去首尾斜线的原始路径（根路径为 `/`），`method()` / `is_method()` 检查方法；`header()`、`bearer_token()` 返回 `Result<Option<&str>>`，非法文本头为稳定的 400 错误。`headers()` 保留只读二进制头访问。`host()` 与域路由共用严格 authority 校验，不采信转发 Host；客户端 IP 继续使用 `ClientIp`。

启用 `view` 时，`Views::render_for_request(name, &RequestInfo, &context)` 使用本请求的 `errors` 对象；`RequestInfo::put_view_error_bag` 可在处理器中添加或替换命名包，后续渲染可见。Session Provider 若存在，会在 handler 前恢复错误包；未安装时为空包。该对象不存放在全局视图环境。

```rust,ignore
async fn store(
    request: openrust::http::RequestInfo,
    openrust::http::Json(input): openrust::http::Json<serde_json::Value>,
) -> openrust::Result<openrust::http::Json<serde_json::Value>> {
    let is_json = request.is_json()?;
    Ok(openrust::http::Json(serde_json::json!({
        "path": request.path(), "is_json": is_json, "input": input,
    })))
}
```

用 `accepts(&["application/json"])` 检查 MIME，用 `prefers(&["html", "json"])` 取得原候选格式，用 `format("html")` 取得默认格式名；`wants_json()` 与 `expects_json()` 分别检查首选 Accept 及 AJAX/PJAX 语义。协商保留固定 Laravel/Symfony 的质量排序、通配符、参数与 q=0 行为，不能将它当成授权或凭据验证。缺少或为空的 Accept 使 `accepts` 为真、`prefers` 为 None；应用明确选择默认表示。协商不会自动改写已有异常策略，返回不同表示的 handler 应设置 `Vary: Accept`。完整调用见 [唯一业务示例](../../modules/example/README.md)。

检查快照限制 headers 总计 64 KiB/128 个值、Accept 16 KiB/128 项，超限拒绝且不截断。`Debug` 不输出头、凭据或 URI/query；显式读取后的日志脱敏仍由调用方负责。这些 helper 不等同于完整 Laravel Request：输入修改、Cookie、Session、用户解析、FormRequest、flash 与文件能力按各自公开接口使用或交付，不由检查快照隐式提供。


### 请求地址

`RequestInfo`读取请求地址，`RouteUrlGenerator`按已登记路由和`app.url`生成出站链接，两者用途不同。地址方法返回`Result`，不从应用配置补造缺失的请求Host，也不将绝对URI的`https`视为实际TLS。

```rust,no_run
use openrust::http::{Json, Request};

async fn address(request: Request) -> openrust::Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "url": request.url()?,
        "full_url": request.full_url()?,
        "next_page": request.full_url_with_query(&serde_json::json!({"page": 2}))?,
        "without_page": request.full_url_without_query(&["page"])?,
        "segments": request.segments()?,
        "first": request.segment(1)?,
        "matches": request.is(&["api/*"])?
    })))
}
```

`full_url()`按原始查询串规范化：顶层键排序，空格编码为`%20`，数组使用显式索引；`full_url_with_query`和`full_url_without_query`使用当前query输入，反映GET/HEAD的`merge`或`replace`，不混入POST正文。增参为顶层覆盖而非深合并；删参先匹配完整顶层键，再处理点路径。固定Laravel在当前query bag为空时以原始`fullUrl`为基础追加问号；因此GET `replace({})`清空输入但原URI仍有query时，增参保留上游可能出现第二个问号的行为。若需完全替换原请求地址，应由应用明确构造目标URL。地址可能含业务敏感参数，应用应按用途选择返回字段，不将完整URL写入公开日志。

`root()`返回协议和主机；`http_host()`保留非默认端口，`port()`返回显式或协议默认端口。实际安全状态沿`RequestSecurity`或可信peer/proto链确定，未信任转发头不生效；代理Host/Port/Prefix的完整支持尚未交付。`decoded_path()`百分号解码但保留加号，`segments()`解码后分段，`segment(1)`从第一段起；非法UTF-8显式报错。`is`和`full_url_is`采用区分大小写的星号模式。`prefetch`仅识别对应头值完整等于prefetch。

查询原文限制16KiB/1024字段，编码和完整URL限制128KiB；模式最多128项、合计16KiB，超限显式413。Session previous 与可选 Precognition 的接入方式见下节；完整 FormRequest、URI 对象及官方前端包不由 URL helper 隐式提供。

### 当前路由上下文

`RequestInfo` 与受管理的 `Request` 可读取 `route()`、`route_name()`、`route_is(&[...])`、`route_parameter(name)` 和 `route_parameter_or(name, default)`；均返回 `Result`。无实际匹配时，`route_is` 为 `false`，其余路由读取为 `None`；无路由时也不采用参数默认值。公开 `Route` 是当前请求的 owned 快照，包含可空名称、去首斜线的声明 URI、可空域模板、方法列表和按绑定顺序保存的 `serde_json::Value` 参数。域与路径同名时路径值覆盖，具名模式中的 `*` 匹配完整字符串任意部分。当前参数是已解码路径与域标签值，模型、枚举与显式 binder 仍需后继能力。

框架只在真实受管理路由命中后发布上下文：全局 middleware 调用下游前可见 `None`，下游返回后可见命中；路由 middleware 短路或业务主动返回错误仍保留命中。内置404/405和静态文件没有业务 Route；显式 GET/HEAD fallback 实际执行时有 Route，静态资源仍先处理。当前方法没有普通路由时，GET/HEAD 可进入 fallback；OPTIONS 有可用 alternate 时合成状态200、空正文、`Allow` 及真实 OPTIONS Route。`RequestInfo::get_route_resolver` / `set_route_resolver` 允许可信应用为本请求包装或替换同步解析器。原生 Axum `Router`/`MethodRouter` 可单独使用，但不会自动生成完整受管理路由上下文。

## 字段规则与 Precognition

输入类型实现 `validation::Validate::rules()`，以一份字段规则供普通请求、预检及跨协议 `validation::validate(value)` 共用。规则按声明顺序执行；自定义规则使用 `Rule::custom`，读取当前字段和原始输入，不需要修改框架源码。

```rust,ignore
use openrust::prelude::*;

#[derive(serde::Deserialize)]
struct CreatePost { title: String, body: String }

impl Validate for CreatePost {
    fn rules() -> Rules {
        Rules::new()
            .field("title", [Rule::required(), Rule::string(), Rule::min(3)])
            .field("body", [Rule::required(), Rule::string()])
    }
}
```

普通处理器仍提取 `Validated<Json<CreatePost>>`。JSON 保留原有严格 DTO 类型约束，Query 保留已有字符串反序列化方式；规则本身的值语义与 DTO 能否构造是两层检查。serde 默认值在 DTO 构造时生效，规则读取原始规范化字段；未提供字段不会因 serde 默认值而自动视为已提供。`length` / `range` 保留原有应用规则原语，不代表完整 Laravel Validator 规则集。

在 Provider 贡献的路由组上显式使用 `RouteGroup::new().middleware_alias("precognitive")`，即可启用内置可选中间件；登记能力不会默认启用全站预检。也可显式装配 `handle_precognitive_requests`。请求头 `Precognition: true` 仅表示尝试，`RequestInfo` / `Request` 的 `is_precognitive()` 读取由中间件确认的请求状态；未挂载时，同名头不会阻止业务执行或 Session 保存。

确认后，受管理 Handler 按原 Axum 顺序解析参数，提取失败保留拒绝响应，全部成功返回空 204 与 `Precognition-Success: true`，不调用业务函数。存在 `Precognition-Validate-Only` 时，`Validated` 只执行筛选后的既有字段规则，成功即可提前结束，缺少未选必填字段不要求先构造完整 DTO。模式按逗号分隔且不去空白，`*` 匹配一个非空点分段；缺少该头与存在空值不同。未选规则不会执行，包括自定义回调。

普通响应也追加 `Vary: Precognition`；确认响应另含 `Precognition: true`。中间件直接返回 204 不等于验证成功，不会据此补成功头。已执行的中间件、前置参数提取及选中校验的副作用不由预检回滚。认证与授权必须放在路由中间件，或放在 `Validated` 之前的参数提取器中：字段预检成功会提前返回 204，失败会返回字段错误，排在它之后的提取器都不会执行，不能承担该请求的访问控制。唯一模块将认证放在路由中间件，先完成认证再验证；不能仅因 Handler 签名含后置认证参数就声称预检已经授权。原生 Axum Router/Handler 逃生入口不自动获得受管理分派保证；自定义 Handler 通过公开 `ManagedHandler` 实现提取契约。

### Session previous

启用 Session 后，GET 真实路由下游返回才记录完整 URL 与可空路由名，不按响应状态过滤。处理器读取 `session.previous_url()` / `previous_route()` 时看到的是此前请求的值；下次同一会话请求才能看到本次记录。匿名路由写入显式 null，HEAD、ajax、prefetch 不更新 previous，但仍正常保存其他 Session 属性。

已确认 Precognition 仍启动 Session、执行 GC 与处理响应 Cookie，只跳过自动 driver.save 和 previous 更新；这不禁止应用自行调用保存，也不承诺其他中间件无副作用。单纯发送头不会跳过保存。公开 previous helper 沿既有 `get` 保留 `Option<Value>` 的缺失/null 区分，setter 沿既有状态与预算检查；`has_previous_uri` 只检查非 null，不解析 URI 对象。业务参考[唯一模块 Session Controller](../../modules/example/http/controllers/session.rs)。

## Cookie 读取、队列与响应

宿主通过 `CookieServiceProvider` 登记能力，业务从同一请求作用域提取 `CookieJar`。`RequestInfo` 与唯一正文提取器 `Request` 共用 Cookie 视图：`cookie(name)` 读取直接键，`cookie_or(name, default)` 在缺失或解密后 null 时采用默认值；`cookies()` 读取整个快照，`has_cookie(name)` 对空串仍为真。它们不使用普通输入的 dot 查找或 trim/null 转换。业务应只选取所需 Cookie，不把整个快照或原始头回传给客户端。

`jar.factory().make(name, value, minutes, options)` 创建值对象；0 分钟为会话 Cookie，`forever` 为 400 天。`jar.queue(cookie)` 在本请求响应阶段发送，`jar.expire(name, options)` 排队删除标记；删除必须使用原有 path/domain。队列按 name/path 标识，修改相同位置不改变顺序；domain 不参与队列标识。不同请求的队列互不共享。

`CookieResponseExt::with_cookie` / `with_cookies` 将 Cookie 对象附加到既有 `Json`、统一响应或其他响应；`without_cookie_named` 通过明确传入的工厂创建删除标记。业务仍使用现有响应与异常策略，不需要另一套正文协议。实际调用及路由见[唯一模块 Cookie Controller](../../modules/example/http/controllers/cookie.rs)。

`CookieServiceProvider::new()` 要求 `encryption` feature 与已登记的 `Encryptor`，缺失时启动失败。入站使用现密钥环解密，出站使用当前密钥；完整 Cookie 叶名参与受认证载荷，不能将其他名称的密文移作本名。解密失败使该根 Cookie 为 null，已认证但叶名不符仅使该叶为 null。`except` 按各阶段实际名称精确匹配：入站为 wire 解析后的根键，出站为 Cookie 对象的字面名称。普通无括号名称可双向排除；例如排除 `prefs` 不会阻止出站 `prefs[color]` 加密，回送后解析出的 `prefs` 根却会跳过解密。不要把根名配置当成出站名称前缀规则；业务宜使用一致的普通名称。显式 `plain()` 不提供机密性或完整性保护，不能当成 Laravel 默认受保护 Web 栈；没有自动降级。

工厂默认 path `/`、httpOnly=true、SameSite=Lax，domain 与 secure 未指定。有 Session 配置时读取最终共享配置；`with_cookie_config` 在对应 Provider boot 时覆盖，后续显式 overlay 按原 Provider 顺序生效，`with_factory` 仅覆盖 Cookie 工厂。最终配置冻结后才装配 Session 层，避免原生 Session 与 Cookie 工厂使用不同快照。当前 Session 自身默认 secure=true，不能据此声称所有 Session 默认已与上游完成对齐。

secure 未指定时，原生 TLS 宿主可注入 `RequestSecurity`；代理部署仅接受现有可信代理配置匹配的 peer 所发首个 `X-Forwarded-Proto` 值的首个逗号 token，`https/on/ssl/1` 表示安全传输。客户端自行构造的 HTTPS URI 不证明 TLS，非可信来源的转发头不生效；显式 secure=false 保持覆盖。应用无需另建代理信任配置。

受管理 Router 保留原始 Cookie 头供严格认证重复检查，给 Session 与 Cookie 读取提供同一解密视图；`RequestInfo::header` 仍是原始 wire 逃生口。原生 Axum Router 或未安装 Provider 的入口不自动具有这些保护。wire 解析保留 PHP SAPI 的 raw URL decode 与重复键规则，不等同于表单 `parse_str`；非法 UTF-8 返回 400，输入树及响应预算超限返回 413，非法应用参数返回 500，均不回显值。响应先排队再加密，失败不输出部分 Cookie，也不回退明文。

## 统一请求输入

`openrust::http::Request` 是唯一正文所有者，复用 `RequestInfo` 的检查方法并提供 `input` / `json` / `query` / `post`、`all` / `keys`、存在性、筛选和修改。它与 `Json<T>` 不在同一 handler 中并列；需要 DTO 时使用 `deserialize_json::<T>()`、`deserialize_query::<T>()` 或 `deserialize_input::<T>()`。只需检查 headers 且继续用 `Json<T>` 时仍选 `RequestInfo`。`content()` 返回有界的原始字节，输入修改不会重写原文；不要把原文或密码写入日志。

JSON 类型优先，包括 GET/HEAD；其他 GET/HEAD 取 query，其他方法取 form。`all` / `input` 采用 source 与 query 的顶层左并集，body 的 null 或完整对象会遮住同名 query 值。`input("profile.name")?` / `json("profile.name")?` 支持 dot 与读取通配；`query("profile.name")` / `post(...)` 只取直接键。读取结果为 Option，资源超限另由 Result 返回；`has(&["name"])` 包括 null，`filled(&["name"])` 对空白/null 为假、对 false/0/空数组为真。

`only` / `except` 返回筛选值；`merge` 按 dot 写入（星号是字面键），`merge_if_missing` 按合并后可见输入判断，`replace` 只换当前 source，query fallback 仍可出现。修改失败不保留部分结果，也不会再次 trim。PHP form/query bracket 数组、重复覆盖、数字/稀疏混合键和规范键均由同一解析器处理；稀疏数组保留原索引，必要时表示为 JSON object。

受管理的 `Request`、`Json<T>`、`Query<T>` 及 `Validated` 默认先去首尾不可见字符，再将空字符串转 null。默认 trim 排除仅完整键 `current_password`、`password`、`password_confirmation`，嵌套 `user.password` 仍处理；密码空串也转 null。用 `InputNormalization::disabled()` 作为选定路由组的 Tower Layer 可关闭两项转换，也可用 builder 独立配置或向请求 extensions 插入策略来条件跳过。原生 Axum 提取器不受这套策略管理。

正文按 Axum 受限读取，默认2MiB，可由应用显式 `DefaultBodyLimit` 调整；query限制16KiB，URL编码输入最多1024个原始非空字段对，值树最多1024 entries/64层，完整展开键累计20MiB。超限明确拒绝，不截断；非法UTF-8的form/query报400，不用替代字符改写。动态 `Request` 保留 Laravel 畸形JSON为空bag、scalar转数组的语义；typed `Json<T>` 继续严格返回语法400、媒体415和类型422。规范化 typed JSON 仅改写值字符串，保留原始数字词法以支持 Rust `u128` / `i128`，并使用有界额外缓冲。动态 `Request` bags 使用标准 `serde_json::Value` 数值表示；需要保持128位整数精度时选择 typed `Json<T>`，不要先经动态Value往返。

multipart 在显式选择 `Request` 时只保存有界原文，本簇不提供其文本/文件bag合并；需要现有流式上传时使用对应公开上传接口。Cookie 通过独立接口读取；Session、old input/flash、FormRequest及完整文件语义按各自能力交付，不因增加此提取器而自动具备。

### 类型便捷读取

`string` / `str` 返回 `support::Stringable`；`integer`、`float`、`boolean` 按 Laravel 的宽松转换读取当前输入。显式默认使用 `*_or`，仅缺失字段采用默认值，null仍参与转换；这些方法不替代DTO与业务校验。例如 `integer("quantity")?` 可将 `"2items"` 转为2，要求整个字段为合法整数时应使用校验或严格反序列化。`boolean`识别true、1及大小写不敏感的true/on/yes字符串，不能用“字符串非空”代替。

`array(key)` / `collect(key)` / `fluent(key)`读取指定值；对应`*_all()`读取全部输入，`*_only(keys)`读取选定路径。`Collection`保留PHP整数/字符串键与稀疏性，`values()`才显式重编号；`Fluent`是独立属性快照，修改快照不修改原请求。`fluent_or`对null和缺失都采用默认属性，与标量方法的默认规则不同。

`when_has`、`when_filled`、`when_missing`的主回调返回false、0、空字符串、字符串`"0"`、空数组或null时，结果分支保留原请求；`*_or_else`的默认回调结果直接保留。Rust用`InputOutcome`显式区分请求、主回调结果和默认结果，`and_then`仅继续请求分支。需同时修改请求时选择`*_mut`，避免捕获已借用的请求；框架对象即使为空也按PHP对象视为真，不等同于空数组。

业务枚举实现`support::BackedEnum`，指定`Backing = String`或`i64`并提供`try_from_backing`。`r#enum::<E>(key)`与`enum_value::<E>(key)`共用转换，合法但未知的backing返回None，`enum_or`和惰性的`enum_or_else`提供默认值；不合法容器类型返回安全输入错误。`enums::<E>(key)`过滤不存在的case，保留原键、顺序和重复值；此集合路径另要求`Serialize`以校验有界输出，单值枚举读取不要求序列化。`when_enum`按转换成功的case决定是否调用主回调。

`clamp`使用`support::InputNumber`区分整数和浮点数，按Laravel的`min(max(value, min), max)`选取结果；反转边界不会触发Rust panic。它要求完整数字输入，不能套用`integer`的数字前缀规则；非数字输入采用默认值，再执行相同边界运算。业务校验应独立约束范围、非有限数及响应序列化要求。

可在业务 handler 中直接读取；严格业务规则仍交给校验层。下例只返回明确选择的字段：

```rust
use openrust::http::{Json, Request};

async fn preview(request: Request) -> openrust::Result<Json<serde_json::Value>> {
    let title = request.string("title")?.trim();
    let quantity = request.integer_or("quantity", 1)?;
    let subscribed = request.boolean("subscribed")?;
    let items = request.collect("items")?.to_value()?;
    Ok(Json(serde_json::json!({
        "title": title,
        "quantity": quantity,
        "subscribed": subscribed,
        "items": items,
    })))
}
```

完整业务接线见唯一示例的 [Controller](../../modules/example/http/controllers/example.rs) 与 [路由](../../modules/example/routes/api.rs)；集合、枚举、条件分支及日期读取沿同一 `Request` 入口，不需要新建输入仓储。

### 日期与时间间隔

`date(key)`返回`Result<Option<InputDate>>`，`date_with(key, DateOptions { format, timezone })`支持显式PHP日期格式与时区，格式不是chrono的`%`语法。`app.timezone`默认为UTC，可用`APP_TIMEZONE`覆盖；非法应用时区在启动时拒绝。输入文本自带时区时按PHP规则覆盖选项。`InputDate`保留epoch、微秒与时区身份，`timestamp()`、`timezone()`和`format("Y-m-d H:i:sP")`可供业务使用；需要chrono对象时显式调用可失败的`try_chrono()`，转换失败不代表底层日期不存在。

请求通过现有extensions接收`CalendarContext`；测试可注入`FixedClock`，业务也可实现`CalendarClock`。FixedClock明确采用Carbon的test-now语义；自定义clock默认采用生产规则，需mock语义时显式覆盖`is_test_clock()`，不能将两种模式的缺项、时区和微秒行为混同。每次date调用读取一次clock，同一次调用使用同一基准；`Request`生成的`Fluent`继承该上下文，独立构造的Fluent默认UTC和系统时钟。不要通过修改进程全局TZ或静态测试时钟影响其他请求。

`interval(key)`保留年月日时分秒及带符号微秒，`interval_with_unit(key, IntervalUnit::Hour)`或字符串`"hour"`按显式单位读取。表达式里的`"90 minutes"`仍保留90分钟，不自动改成1小时30分钟；年月不折算为固定秒数。显式单位名称与表达式缩写是不同规则，例如`"m"`可出现在表达式中，不能作为显式单位参数。缺失或空值返回None，非法表达式返回安全输入错误；仍未覆盖的相对区间语法以`input_interval_unsupported`明确拒绝，不静默返回None。

日历解析另限制源字符串与显式参数合计20MiB；默认日期解析的语义操作、区间词项分别限制每次最多1024项操作，超限返回413。区间预算同样覆盖无空格的compact表达式；这些预算与值树展开键预算分别执行。错误不回显原始日期、格式或时区内容。完整PHP/Carbon语法对应仍按[ADR-079](../../.agents/project/decisions/adr-079-request-input.md)推进，当前公开方法不表示整个Carbon工具箱已经交付。

`/health` 是进程存活探针；`/ready` 要求 listener 正在接收流量且所有依赖探针成功，否则返回 503。DatabaseServiceProvider 检查每个已配置连接池，CacheServiceProvider 对已解析 store 执行只读读取探针；应用 Provider 也可通过 `Readiness::register_probe` 注册最多 64 个稳定命名的异步探针。每轮最多并发 16 个探针、总时限 2 秒且全局仅允许一轮并发检查；错误详情不返回给请求方，服务端只记录稳定错误码和探针名。探针须只读且有界。该 HTTP 端点不代表 gRPC health、实时连接或任意外部依赖的状态。收到 SIGINT/SIGTERM 后，服务器先将自身标为 not-ready 并停止接受连接，再按 `[server].shutdown_timeout_secs` 等待在途请求；到期会合作取消请求、强制终止剩余 server task 并以 `http_shutdown_timeout` 失败退出。请求取消会丢弃本地 handler future，不能撤销已提交的数据库或外部副作用。

启用 `metrics` Cargo feature 后，HTTP 请求会记录请求数、耗时和在途数；指标标签只包含固定的 HTTP 方法类别和状态类别，不包含路径、请求标识、用户信息或错误内容。`/metrics` 默认不注册。仅当 `[server].metrics_endpoint_enabled = true` 时才开放 Prometheus scrape 端点；生产部署应通过监听网络、反向代理或防火墙限制采集访问，不要将端点公开给普通客户端。日志中的 HTTP 请求 span 只记录方法，不记录完整 URI、查询参数、request ID 或 `traceparent`。

`[server].max_concurrent_requests` 默认是 1024，允许配置范围为 1..=65536。达到上限时新请求立即收到 503，不排入等待队列。许可覆盖响应体读取生命周期：body 读完、读取失败或客户端丢弃响应时释放；未消费且仍持有的流式响应会继续占用许可。

`[server.compression] enabled = true` 会根据 `Accept-Encoding` 协商 gzip 和 Brotli；设置为 `false` 可关闭压缩。`[server.cors]` 默认禁用。启用时，将 `enabled` 设为 `true`，并配置 `allowed_origins = ["https://example.com"]`（未启用 credentials 时也可用 `["*"]`）。Methods、request headers、exposed headers、credentials 和预检 `max_age_secs` 均可配置。具体 origin 必须是 `http://` 或 `https://`，后接主机和可选端口，不可包含路径、查询参数或尾部斜杠。CORS 列表中不能混用通配符和具体值；启用 credentials 时，methods、request headers 和 exposed headers 也不能使用通配符。路由构建时若发现配置无效，会返回包含配置 key 和转义值的错误。CORS layer 包裹 timeout layer，因此 408 响应仍保留允许来源信息。`build_router(&App)` 返回 `Result<Router>`；`serve` 会先构建 router，再绑定 listener。

`[server].trusted_proxy_ranges` 默认为空。条目接受 IP 地址或 CIDR。若直接 socket peer 位于配置范围内，`ClientIp` 会解析 `X-Forwarded-For`，从右向左遍历可信代理链，并返回第一个不可信地址；否则忽略转发头。可信范围配置无效时 router 构建失败；来自可信 peer 的转发值格式错误时返回 bad-request 响应封装。

使用 route layer 可应用共享固定窗口配额。应用必须注册 `RateLimiter`，通常通过 `DatabaseServiceProvider` 注册；存储故障会失败关闭。每个 scope 应对应一个 endpoint/policy，并在所有路由实例中使用相同的 limit/window。`by_user` 要求先添加认证 layer，使其先于限流 layer 执行；允许匿名请求的路由也可使用 `by_user_or_client_ip`。

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

Laravel 的 `Route::prefix(...)->middleware(...)->group(...)` 可用 `RouteGroup` 表达。嵌套组组合路径前缀，middleware 只覆盖组内路由，调用顺序与请求进入顺序相同：

```rust,ignore
use openrust::auth::{AuthLayer, AuthMode};
use openrust::http::{RouteGroup, Routes};

fn routes(routes: Routes) -> Routes {
    routes.group(
        RouteGroup::new()
            .prefix("/api")
            .middleware(AuthLayer::required(AuthMode::Jwt)),
        |api| api.group(RouteGroup::new().prefix("/v1"), |v1| {
            v1.get("/profile", profile)
        }),
    )
}
```

Provider boot 可用 `BootContext::middleware_alias` 登记一个 Tower `Layer` 的短名称，再用 `middleware_group` 将别名或其他组按声明顺序组合。应用 Provider 贡献的路由组通过 `middleware_alias` / `middleware_group` 引用；也可在带应用注册表的 `Routes` 上引用，以作用于此前登记的路由。没有应用注册表的独立 `Routes` 使用命名 middleware 会在 `into_router()` 返回错误，绝不静默生成未受保护路由。名称重复、格式无效、引用缺失和 group 循环均会使启动或 router 构建明确失败。直接 `.middleware(layer)` / `.layer(layer)` 仍可用于一次性组合：

```rust,ignore
app.middleware_alias("tenant", tenant_layer)?;
app.middleware_group("private-api", ["auth", "tenant"])?;
app.routes(|routes| {
    routes.group(RouteGroup::new().middleware_group("private-api"), |api| {
        api.get("/api/private", private_index)
    })
})?;
```

Provider 可用 `middleware_priority([...])` 设置路由别名优先级，或 `prepend_middleware_priority(name)` / `append_middleware_priority(name)` 调整表首/尾。组展开后沿统一路由链调整倒置的已列优先项，相同命名项只执行一次；未列项保持相对顺序。默认表包含 `precognitive`、`auth`、`throttle`、`auth.session`、`can`，不自动登记或启用未提供的认证等能力；显式空表可清空默认顺序。全局 `http_layer` 与原生逃生入口不参与此表。

Middleware 定义在应用内共享，是否应用由各路由或路由组显式选择。当前 OpenRust 路由按 Provider 贡献，没有 Laravel `web` / `api` 文件约定的自动分组，因此由路由明确选择命名 group。

`Routes::fallback(handler)` 注册 GET/HEAD 显式 fallback；`RouteGroup` 内的 fallback 仅覆盖组前缀，并遵循组 middleware。同方法普通路由优先，GET/HEAD 在没有普通路由命中时仍可执行 fallback，即使同路径存在其他方法。其他方法可得 405；OPTIONS 有 alternate 时合成真实 OPTIONS Route。同一静态完整 scope 的多个 fallback 会在构建时返回 `http_fallback_duplicate`。带路由参数或通配符的组前缀暂不支持 fallback，构建时优先返回 `http_fallback_scope_unsupported`。除交给自定义 `server.root_target` 的未匹配根 GET/HEAD 外，未匹配请求先检查 `public/` 静态资源，命中时返回文件；没有静态资源才执行业务 handler。fallback handler 会收到完整原始 URI、请求头和 body。默认没有业务 fallback 时，`/api/*`、`/ws/*` 继续返回框架 404；应用显式配置 fallback 后可以接管这些路径，包括优先返回对应静态文件。

```rust,ignore
use openrust::http::{RouteGroup, Routes};

async fn page_not_found(request: openrust::http::axum::extract::Request) -> impl openrust::http::axum::response::IntoResponse {
    (openrust::http::axum::http::StatusCode::NOT_FOUND,
     format!("页面不存在：{}", request.uri().path()))
}

fn routes(routes: Routes) -> Routes {
    routes.group(RouteGroup::new().prefix("/pages"), |pages| {
        pages.get("/about", about).fallback(page_not_found)
    })
}
```

路由可以在定义后立即通过 `.name(...)` 命名；组的 `name_prefix` 会与内层名称按原样拼接，因此分隔符应由应用提供，例如 `admin.`。处理器提取 `RouteUrlGenerator` 后，可按名称生成绝对 URL。必需路径参数占用一个路径段，其值会编码为单段；多余参数进入按字典序排列的 query。host/scheme 来自 `app.url` 配置，不读取请求的 `Host`：

```rust,ignore
use std::collections::BTreeMap;
use openrust::http::{Json, RouteGroup, RouteUrlGenerator, Routes};

async fn show_user(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    let params = BTreeMap::from([("id".to_owned(), "42".to_owned())]);
    urls.route("admin.users.show", &params).map(Json)
}

fn routes(routes: Routes) -> Routes {
    routes.group(
        RouteGroup::new().prefix("/api").name_prefix("admin."),
        |api| api.get("/users/{id}", show_user).name("users.show"),
    )
}
```

重复路由名、非法名称、通配符及包含空段的具名路径会在 router 构建时失败。缺少命名路由或必需参数会返回稳定错误并映射为 HTTP 500，符合 Laravel URL 生成异常的服务端错误语义；未命名路由不受 URL 模板限制。

命名路由也可直接用于请求绑定重定向：`Redirector::from_info(&info).route(name, &parameters)` 以默认 302 和空自定义头建立响应；`route_with(name, &parameters, status, headers)` 接受显式状态与 `HeaderMap`。它从当前请求捕获同一应用的 `RouteUrlGenerator`，再复用普通重定向的 `to_with`、Location、HTML 转义和头规则，不依赖 Session：

```rust,ignore
use std::collections::BTreeMap;
use openrust::http::{Redirector, RequestInfo};
use openrust::http::axum::response::{IntoResponse, Response};

async fn redirect_to_user(info: RequestInfo) -> openrust::Result<Response> {
    let parameters = BTreeMap::from([("id".to_owned(), "42".to_owned())]);
    let redirect = Redirector::from_info(&info).route("admin.users.show", &parameters)?;
    Ok(redirect.into_response())
}
```

需要闪存输入时使用 `Redirector::from_request(&request)`；命名跳转返回同一个 `RedirectResponse`，可继续调用 `with_input`、`only_input`、`except_input` 和 `with_errors`，仍借用原请求与 Session。名称或必需参数错误沿用生成器现有错误与 HTTP 500；在应用 HTTP router 之外未捕获生成器时返回 `http_route_url_unavailable`，不会创建全局路由表或静默回退普通路径。

命名重定向沿用现有 `BTreeMap<String, String>`、必需域/路径参数和 query 编码规则；域名模板及 `app.url` 的来源规则不受请求 Host 影响。这是已支持参数形态的接线，不代表 PHP 混合参数、模型/枚举绑定、可选参数或 `missingNamedRouteResolver` 已完成；action 与 signed 重定向仍属后继范围。


### 刷新当前路径与原样目标

`Redirector::refresh()` 以默认302及空自定义头刷新当前请求路径；`refresh_with(status, headers)` 提供显式状态和头。它按 `RequestInfo::path()` 再走普通 `to_with`：保留原路径百分号编码，裁剪首尾斜线，根路径回当前请求根URL，丢弃原query；不使用解码路径或完整URL。

`away(target)` 默认302及空自定义头；`away_with(target, status, headers)` 原样使用目标，不经过绝对URL生成或协议改写。例如 `away("relative/path")` 保留相对 `Location`，外部URL、`//`、锚点及特殊scheme也保持原值。它不自动施加同源或scheme白名单，业务需要限制目标时应显式验证；“不生成或校验URL”不代表放宽HTTP头安全。

两入口复用唯一响应构造器：允许201/301/302/303/307/308，非法状态沿 `redirect_status_invalid`；空目标与不安全 `Location` 沿 `redirect_location_invalid` 返回错误。共享构造按HTTP状态范围、空目标、允许重定向状态、头值的顺序检查；普通 `to` 先生成URL再校验响应，纠正此前提前检查状态的顺序偏差；URL来源和状态同时无效时先传播URL生成错误。因此 `to("")` 是请求根，`away("")` 则失败。Rust在 `HeaderValue` 构造时拒绝CRLF等非法值，不模拟PHP将此类值暂存于HeaderBag后才在发送边界处理的过程。自定义Location/Content-Type由真实目标和HTML类型覆盖，其他头保留；HTML转义及301缓存规则不变。

```rust,ignore
use openrust::http::{Redirector, RequestInfo};

async fn reload(info: RequestInfo) -> openrust::Result<axum::response::Response> {
    use axum::response::IntoResponse;
    Ok(Redirector::from_info(&info).refresh()?.into_response())
}

async fn guide(info: RequestInfo) -> openrust::Result<axum::response::Response> {
    use axum::response::IntoResponse;
    Ok(Redirector::from_info(&info).away("https://laravel.com/docs")?.into_response())
}
```

普通调用无Session亦可用；从 `from_request` 建立的响应仍借用同一规范化输入与Session，可继续 `with_input/only_input/except_input/with_errors`。这些入口不改变guest先保存、intended先消费后建立响应的顺序，也不代表action、signed或完整Redirector已完成。

### 重定向协议与一次返回意图

`Redirector::to_with_scheme(target, status, headers, secure)` 为相对目标或空目标选择协议：`None` 沿当前请求，`Some(true)` 使用 HTTPS，`Some(false)` 使用 HTTP。`secure(target)` 等价于默认302、空自定义头及 `Some(true)`；`secure_with(target, status, headers)` 接受显式状态和头。合法绝对 URL、`//`、锚点及既有特殊 URL 不被强制改写，`secure("http://external.test/path")` 仍保留 HTTP。该选项不改变请求快照、`app.url` 或命名路由生成器；既有 `to/to_with/route` 保留原默认协议和响应规则。`back` 按固定上游直接用previous建立响应：Session相对字符串保持相对Location，数值/true转为字符串；Referer和fallback仍生成URL。此前把Session previous无条件绝对化的偏差已纠正，HTML/头规则仍复用同一响应构造器。

启用 `session` feature 后，`guest(target)` 默认302、空头、不覆盖协议；`guest_with(target, status, headers, secure)` 提供完整选项。它仅在 **`GET`、存在真实匹配 `Route`（含匿名路由）且 `expects_json` 为false** 时记录当前完整URL及规范化query；HEAD、非GET、无Route或 `expects_json` 为true的请求取 Referer→Session previous→请求根。Referer和fallback经过URL生成，Session previous的真值则原样保留类型及相对形态，直到后续 `intended` 才按当时请求的Host/协议生成目标。AJAX/PJAX仍沿现内容协商结果判断。非空序列/JSON对象也先原样保存，后续消费时才报目标类型错误；Session JSON对象对应PHP关联数组，因此空对象/空序列与null、false、0、`"0"`、空串都按PHP数组/标量假值落根fallback，不代表支持任意PHP对象。意图存于普通Session `url.intended`，不是flash，不因下一请求老化而消失；先保存再建立响应，后续目标或状态错误不撤销保存。

`intended()` 只在属性缺失时使用 `/`；`intended_or(default)` 接受可转为JSON的默认值，`intended_with(default, status, headers, secure)` 另收响应选项。它先从Session取出并删除，再转换目标、校验响应，转换或响应失败也不恢复已消费值。显式null/false/空串转为空目标，0转为 `/0`，true转为 `/1`，均不使用缺失默认；数值沿现PHP标量转换。数组/对象返回 `redirect_intended_url_invalid`，对应上游URL字符串操作的类型失败；Rust不模拟PHP的null弃用警告，也不支持JSON之外的可字符串化对象。

`get_intended_url() -> Result<Option<Value>>` 原样读取，`None` 是缺失、`Some(Value::Null)` 是已存null；`set_intended_url(value)` 沿Session普通put并返回自身，不预先验证URL。缺Session、关闭或资源预算错误沿现Session传播，无静默内存回退；预算失败保留原属性。示例中的业务应在完成认证或审核后调用 `intended`：

```rust,ignore
use openrust::http::{Redirector, RequestInfo};

async fn require_login(info: RequestInfo) -> openrust::Result<axum::response::Response> {
    use axum::response::IntoResponse;
    Ok(Redirector::from_info(&info).guest("/login")?.into_response())
}

async fn login_completed(info: RequestInfo) -> openrust::Result<axum::response::Response> {
    use axum::response::IntoResponse;
    Ok(Redirector::from_info(&info).intended_or("/dashboard")?.into_response())
}
```

返回值仍是借用原请求的 `RedirectResponse`；通过 `from_request` 创建后可继续组合 `with_input/only_input/except_input/with_errors`。返回目标沿既有URL规则，可为外部URL，不自动施加同源限制；应用需要限制来源时应在其业务规则中明确验证。这些入口不代表验证失败自动重定向、`dontFlash`、action、signed或完整Redirector已全部提供。

`RouteGroup::domain` 可为路由组指定完整域名模式。域模式先于不限定域的路由匹配，与注册顺序无关；同一请求可用 `DomainParams<T>` 读取域标签参数，路径参数仍由 `Path<T>` 单独提取。嵌套组继承父域，显式声明的新域会覆盖父域。域参数只用于路由分派，不能替代租户认证或授权。域名 URL 从登记模板和显式参数生成，scheme/port 仍取自 `app.url`；不会采信当前 `Host` 或 `X-Forwarded-Host`。域名仅支持 ASCII DNS 标签与预先编码的 punycode，不执行 Unicode IDNA 转换，不支持 IP literal 和尾点；域参数必须是单个标签。

```rust,ignore
use openrust::http::{DomainParams, Json, Path, RouteGroup, RouteUrlGenerator, Routes};
use serde::Deserialize;

#[derive(Deserialize)]
struct TenantDomain { account: String }

async fn tenant_order(
    DomainParams(tenant): DomainParams<TenantDomain>,
    Path(order_id): Path<u64>,
    urls: RouteUrlGenerator,
) -> openrust::Result<Json<String>> {
    let url = urls.route("tenant.orders.show", &std::collections::BTreeMap::from([
        ("account".to_owned(), tenant.account),
        ("id".to_owned(), order_id.to_string()),
    ]))?;
    Ok(Json(url))
}

fn routes(routes: Routes) -> Routes {
    routes.group(RouteGroup::new().domain("{account}.example.test").name_prefix("tenant."), |tenant| {
        tenant.get("/orders/{id}", tenant_order).name("orders.show")
    })
}
```

域分派要求唯一、有效的 `Host`/URI authority；冲突或畸形 authority 失败关闭。端口不参与匹配，转发头不参与 authority 选择。请求 path/method 在域路由中未匹配时会继续检查后续匹配域及普通路由；只有框架私有的 miss 标记会继续分派，业务 handler 返回的 404/405 保持终端行为。所有业务候选都未命中后，根 GET/HEAD 先执行自定义站点根目标；其他请求继续按用户 fallback 与静态文件规则处理。

同一域模式内（以及不限定域的路由中），fallback 优先选择更具体的静态路径 scope，例如 `/api/v2`、`/api`、`/`，不受这几个 scope 的注册顺序影响。不同且重叠的域模式按首次 fallback 声明顺序选择。自定义站点 `root_target` 在业务路由均未命中时优先处理根 GET/HEAD，因此显式域根路由与普通根路由仍先于站点目标；用户 fallback 不会吞掉自定义根目标。内部 forward 的目的路径缺失时仍按目的路径查静态资源或 fallback，不会重新执行根目标；fallback 路由参数使用目的路径，`RequestInfo` 仍保留外部原始 URL 与查询。若根路径已有其他方法的业务路由，GET/HEAD 仍遵循原有方法 miss 规则：有适用 fallback 时可由其接管，否则返回 405 与 Allow；站点目标不接管该方法 miss。业务 handler 自身返回的 404/405 也是终态。使用 `TestApp` 或直接 `oneshot` 测试含域路由的应用时，请显式提供 `Host` 或绝对 URI authority，模拟合法 HTTP 请求。

```rust
use std::sync::Arc;
use openrust::http::{Inject, Json, Routes, Validated};
use openrust::core::{BootContext, BoxFuture, RegisterContext, Result, ServiceProvider};
use serde::Deserialize;
use openrust::validation::{Rule, Rules, Validate};

#[derive(Deserialize)]
struct CreatePost { title: String }

impl Validate for CreatePost {
    fn rules() -> Rules {
        Rules::new().field("title", [Rule::length(Some(1), Some(120))])
    }
}

struct Posts;

async fn create(Inject(_posts): Inject<Posts>, Validated(Json(input)): Validated<Json<CreatePost>>) -> Result<Json<String>> {
    Ok(Json(input.title))
}

struct PostServiceProvider;
impl ServiceProvider for PostServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(Posts));
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|r: Routes| r.nest("/api", |api| api.post("/posts", create)))?;
            Ok(())
        })
    }
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

注册集合路由和 `{id}` 成员路由前，Resource path 会去掉尾部斜杠；`nest("/posts", …)` 中的空路径会响应 `/posts` 和 `/posts/{id}`。多次调用 `only` 会保留 action 列表的交集。同一完整域、URI 与 HTTP 方法的后注册受管理声明覆盖先注册声明；不兼容的 Axum 路径模式冲突仍由 `build_router` 转为 `http_route_registration_failed` 启动错误。

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

请求解析遵循 [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md)：格式错误的 JSON、path 或 query 使用 `bad_request`（semantic 模式 HTTP 400）；缺少 `Content-Type` 或类型不是 JSON 时使用 `unsupported_media_type`（415）；结构有效但缺少字段或字段类型错误的 JSON 使用 `validation_failed`（422），并尽可能提供可读 `message` 和字段 `errors`；超过 axum 默认 2 MiB 限制的请求体使用 `payload_too_large`（413）。校验规则失败同样使用 422。未匹配路由使用 `not_found`（404），不支持的方法使用带 `Allow` 的 `method_not_allowed`（405），请求超时使用 `request_timeout`（408）。括号内的 HTTP 状态码仅在 `semantic` 模式适用。这些响应均使用带 `x-request-id` 的 `application/json` 封装；启用 CORS 时也会应用对应头部。CORS 预检请求（含 `Access-Control-Request-Method` 的 `OPTIONS`）无论允许或拒绝都保留 tower-http 的空 body；浏览器只使用协议响应头。普通跨域错误仍会包装，并为允许的 origin 返回 CORS 头。Handler 生成的客户端错误保留自身 message。原生 axum extractors 仍可通过 [`axum`] 使用。

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

`[api] http_status` 默认是 `"semantic"`：框架 JSON 封装按 ADR-008 保留创建、校验、未找到、限流及服务故障等 HTTP 状态，同时保持统一 envelope。仅显式设置 `"always_ok"` 时才将这些 JSON 状态映射为 HTTP 200。middleware 会将选定模式和 request ID 保存在请求级 task context 中，使成功响应、提取错误、fallback、timeout 和 panic 响应遵循同一策略。JSON `/health` 响应也使用相同封装；文件、流和 HTML 等非 JSON 响应保持原始格式。

# 变更日志

OpenRust 的重要变更记录于此，格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)。OpenRust 尚未发布正式版本。

## [Unreleased]

### Changed

- 命名路由增加永久与临时签名 URL，支持绝对/相对签名、原始查询验签、过期检查及当前/旧应用密钥轮换；`RequestInfo` 与路由 `signed` 中间件提供验证和忽略查询字段入口，无效签名在业务处理前拒绝。唯一模块和生成应用通过公开接口签发并保护草稿链接。当前参数限字符串映射；Vapor 代理原始查询与前缀入口、PHP 对象/数组/Enum 参数表示仍待对应。

- 配置加载增加不可变 `ConfigSources` 与 `ConfigLoader::sources()`，在进入 Figment 投影前保留已有平层 TOML 的声明键序；默认引导一次捕获来源，类型配置和 Provider 复用该输入。仓储、Provider 与 TestApp 沿既有入口接收有序来源，显式 Figment 输入仍是无法恢复历史声明序的有损入口。文件自然排序不等于已实现递归配置发现或文件名命名空间映射，完整 Config 对应继续开放。

- 配置仓储增加运行期 `set` 与有序原子 `set_many`，保留点路径、覆盖位置及稀疏/混合数组；App 与 Provider 共享仓储，`App::section` 读取当前版本，已解析配置及服务保留启动快照。唯一模块展示固定业务键跨请求更新，生成应用与公开 consumer 同步接入；写入不持久化、不跨节点传播、不自动重建资源，来源进入 Figment 前已丢失的初始键序仍是后继缺口。

- Session 纯行为扩展测试并入既有 `http_session` 私有模块，保留全部用例与断言；原独立 `http_session_extensions` Cargo 测试入口移除，真实数据库、Redis 和环境子进程入口保持独立。入口减少不等于已证明固定提速。

- 路由按同一最终声明集合派生实际分派与动作 URL 视图，修正原串 domain/URI 碰撞、域左侧合并及声明首匹配；允许方法来自幸存集合，GET/HEAD 的405及合成OPTIONS遵固定上游顺序与头格式。唯一模块与生成 helper 展示草稿概览覆盖、静态先于参数路由；文章静态导出入口提前于资源 show，既有授权与业务断言保留。

- 增加真实路由动作身份与动作 URL：显式动作和 Rust handler 同次登记，匿名路由也可通过字符串、二元组或裸 invokable 身份查找；请求绑定 `Redirector::action/action_with` 复用同一生成器和原输入/错误闪存链。唯一模块与生成 helper 展示固定动作回跳、真实命中身份及一次性草稿状态；完整参数族、signed 与完整 Redirector 继续保留后继范围。

- 请求绑定重定向新增 `refresh/refresh_with` 与 `away/away_with`：刷新按当前路径生成目标并丢弃 query，外部跳转直接使用目标原值；均复用现有响应构造和输入/错误闪存链。唯一模块通过 POST→同路径 GET 展示草稿，外链使用可信常量；生成 helper 覆盖显式状态/头、刷新后闪存及固定外链，完整 Redirector 仍保留后继范围。

- 请求绑定重定向增加显式协议选择与 `secure`，并提供 Session `guest`/`intended` 及意图读取/设置入口；意图使用普通 Session 属性，先保存或先消费再构造响应。唯一模块与生成 helper 展示非敏感草稿确认回跳、双 Cookie 隔离与一次消费，保留既有输入及错误闪存流程；HTTPS Location 不代表监听器已启用 TLS，完整 Redirector 仍保留后继缺口。

- 请求绑定的 `Redirector` 新增 `route` 与 `route_with`，复用现有命名路由 URL 生成器，默认返回 302，也可指定状态与响应头；保留输入和命名错误包闪存链。唯一业务示例展示无 Session 的命名跳转和审核页错误闪存，生成应用测试同步接入命名路由；完整 Redirector 与自动校验失败重定向仍属后继范围。

- 视图增加 `Views::render_for_request` 与请求私有错误包；Session 启动后、handler 前恢复，根首页和普通请求使用同一入口。显式数据（含 null）覆盖共享错误，自定义消息包保留模板方法派发；无 Session 使用独立空包。请求共享对象与 Session 保存关联，整包替换后断开旧关联，缺失回退对象不写回 Session。模板支持多键、列表与关联字典值、标量多参数及可空格式参数。

- 首页模板读取运行时 `app.name`；自定义根目标在业务路径未匹配时优先于用户 fallback 和静态首页，默认 `file:/index.html` 保留静态首页与 fallback 组合。内部 forward 按目的 URI 分派，保留外部原始 URL。站点响应出口清空 HEAD 正文并保留状态、头与扩展，不推断 Content-Length；站点外层 middleware 短路响应不在此保证内。

- 将三个纯 Session 测试入口聚为一个，保留原源文件与 26 个用例；修复 gRPC 联合生命周期测试释放随机端口后重绑的竞争窗口。新增可接收两个已绑定 listener 的嵌入式联合运行入口，原 `serve all` 配置绑定方式不变。

- 新增公开 `MessageBag`、`ViewErrorBag` 与消息包扩展契约：保序/稀疏键、字段与通配查询、格式化、命名包及 JSON 选项；递归合并通过 `MessageValue` 保留派生数组。纯消息包无需 Session，自定义命名包保留对象身份并派发契约方法。请求局部视图共享通过 `Views::render_for_request` 提供；自动验证失败重定向仍属后继范围。

- Session与Request增加旧输入读写：`flash_input/get_old_input/get_old_input_or/has_old_input`、`Request::flash/flash_only/flash_except/flush`和`RequestInfo::old/old_or/old_all`。复用已有flash老化和单次写入预算回滚，保留缺失/null/默认、字面键优先dot及空筛选语义。手动闪存不默认剔除密码，应用须显式排除敏感字段；文件输入合并仍属后继范围。

- D10-B 公开 `Redirector::from_info/from_request`、`RedirectResponse` 与 `RedirectErrors`，普通重定向携带 `Location` 和 HTML 正文；请求绑定方法显式闪存规范化输入和命名错误包。Session 的 `ErrorBagCodecs` 支持 Provider 扩展并跨请求恢复自定义错误包，编码/解码在属性锁外运行，失败和预算超限不发布半成品。内置包无需登记；请求局部视图共享见本节视图入口，异常 `dontFlash` 与自动验证失败重定向仍属后继范围。

- 修复空库执行框架 `migrate` 时 MFA 表预查阻断启动：仅实际匹配的框架迁移命令免去运行态五表预查，其他命令、普通启动和 `serve` 仍严格校验；不改变迁移参数、回滚或失败清理语义。

- 受管理 Handler 支持显式启用的 Precognition：按参数顺序提取与验证，成功预检返回空 204，不调用业务函数。字段筛选可提前结束；认证和授权须放在路由中间件或验证参数之前，原生 Axum 逃生入口不自动获得此保证。

- `Validated` 的 DTO 改为实现框架 `Validate::rules()`，由同一 `Rules` / `Rule` 执行器处理普通请求与选中字段预检；不再直接以 `validator` 的派生 trait 作为框架验证契约。完整规则集及 FormRequest 仍按后继范围推进。

- Session 增加 previous URL/route helper；真实 GET 路由下游返回后记录当前请求，供后续请求读取，已确认预检跳过自动保存及 previous 更新，单纯客户端头不会改变保存行为。

- `Routes::route` 改为接收公开 `http::routing` 的受管理方法声明；同路径按方法合并与后注册覆盖，保留各处理器的路由身份和中间件。原生 Axum 路由仍作为显式逃生口，不自动承诺受管理请求 Route 上下文。

- 显式 fallback 按 Laravel 使用 GET/HEAD；普通当前方法优先，fallback 参与备用方法与自动 OPTIONS 判断。域名与路径可使用同名参数，绑定时路径值优先；单个模式内部重复参数仍拒绝。

- 正式宿主与生成应用按数据库来源或显式非数据库 Session driver 装配唯一 Provider；数据库应用无需启用 Auth 才能使用 Session。默认 database 驱动且无数据库时不暗用内存，显式 Cookie driver 可在无数据库宿主运行。

- Session 统一到公开 `openrust::session`，认证与业务共享请求状态和 store；属性同步操作、持久化异步执行。默认数据库、120 分钟 lifetime，Cookie 名从最终 `app.name` 推导，Secure 未指定时依可信传输决定；取消请求不后台保存。自定义 driver 使用公开 `SessionStore`，不再依赖旧 tower Session 接口。

- 应用端口环境键统一为 `APP_PORT`（自定义命名空间如 `SHOP_PORT`），映射至内部 `server.port`；`.env` 与生成应用使用 `APP_URL="http://localhost:${APP_PORT}"`，旧端口别名不再读取。

- 认证示例及生成应用的密码重置确认字段改为 Laravel 式 `password`，保留密码首尾空白，与登录使用相同规范化规则。

- API/framework JSON 默认使用语义 HTTP 状态（如创建 201、校验失败 422、资源缺失 404）；`always_ok` 仅在配置中显式选择时启用，统一 envelope 与机器码保持不变。

### Fixed

- 配置来源错误仅显示可信诊断路径：未声明 schema 时保留已知父键，已声明字段才显示字段路径，动态名称与不可信数组片段不回显；来源值继续脱敏。

- 测试数据库仅在本次 CREATE 已成功后清理目标库；同名创建失败不再删除已有库。PostgreSQL 模板碰撞遵循同样的创建所有权边界；取消、连接中断或删除失败仍可能留下本次资源，不承诺必然回收。

- Cookie Session 认证校验真实受管理 Cookie 保护事实；plain、身份 Cookie 或合法数据 ID 被排除保护时拒绝认证，并在最终保存时防止更换名称/handler 降级保护。Cookie/Session 机制自身的失败响应统一禁止缓存，不泄出被替换响应的 Cookie 或正文。

- 本地存储回收 staging 时允许其他应用实例关闭并删除已列举的 run 目录；权限和其他 I/O 错误继续阻止初始化。

- 修复容器缓存完成通知早于构造帧结束的误报循环，以及生命周期关闭通知持锁时的同步 Waker 重入死锁。

- 配置目录显式错误不再静默退默认；已发现 TOML 精确读取，来源错误安全传播，Debug/启动日志不输出目录原值。

- 在 API 响应中保留类型化 `PageMeta`；明确 CORS 预检响应正文为空，同时普通跨域错误仍使用统一包装。

### Added

- 配置仓储新增 `get`、`get_or`、`has`、`get_many`、`get_many_with_defaults` 与 `all`；保留字面键优先、缺失与 null 的区别，批量读取固定同一快照。成功返回值可能含秘密，调用方须控制输出。

- Container 新增异步 `extend`，按顺序装饰原 factory 或实例并沿用既有发布/作用域机制；可信 Provider 可通过 `SessionStoreHandle::store()` 保留原 store 实现装饰。唯一模块与独立 consumer 提供公开用法，逃生口不替代请求 Session 校验。

- 请求公开真实匹配的 `Route`、路由名称、参数/default 与名称通配匹配，并提供可信应用可替换的 `RouteResolver`。上下文共享于同一请求，覆盖路由中间件与外层下游返回后的读取；不从响应状态、请求头或具名 URL 生成器推断命中。

- 请求新增 Laravel 风格 URL、路径分段/匹配与预取检查；`full_url`规范原始查询串，增删参数读取当前query bag，保留端口和可信传输边界。唯一模块和生成应用演示普通GET、JSON GET及POST来源差异；非法UTF-8与URL资源超限显式报错。

- Session 新增公开 driver `gc(lifetime_seconds)` 与请求级 `session.lottery`，默认 `[2,100]`，启动成功后、业务前对实际绑定驱动执行回收。数据库/内存清理已过绝对截止的记录，Cookie/Redis保留 no-op/TTL 职责；显式期限优先及取消不回滚已发生删除的边界明确记录。

- 新增公开 `CookieSessionStore` 与配置 `driver="cookie"`，复用请求绑定、Cookie 队列和密钥环实现双 Cookie；公开存储保证分类及认证完整性能力。客户端快照仅提供本请求与当前浏览器删除，不能承诺全局撤销；秒精度、异常弱类型及浏览器容量边界明确记录，唯一模块与生成应用提供使用参考。

- Session 新增可信 ID/驱动选择、来源与写入意图管理及请求内 driver 绑定；公开 `SessionRequestContext` 与默认请求 hook。`handler()` 改为返回 `Result`，同步外部实现与示例；显式转移、目标碰撞、撤销和失败 Cookie 清理分别定义，内置 Cookie driver 与客户端快照边界见本次新增能力。

- Session 新增 `remember`、`push`、`increment/decrement` 及显式步长、`remove`，复用同一属性和预算校验；`set_name` 接入真实出站 Cookie，`handler` 提供当前驱动引用。唯一模块和生成应用展示跨请求业务调用；PHP 数值差分与错误处理边界单独记录，完整 Session 扩展生命周期继续后继实现。

- 新增 Laravel 式 Cookie 读取、值工厂、请求隔离队列与响应组合；受保护模式复用现有密钥环，保留原始头的严格认证检查。唯一模块及生成应用展示 Cookie 往返，显式明文模式不提供加密保护。

- `Request` 新增 Laravel 式标量、字符串、集合、枚举、clamp 与条件读取；共用有界输入内核，`Fluent` 继承请求日历上下文。日期和区间使用 PHP 格式与语义，应用时区通过 `app.timezone` / `APP_TIMEZONE` 配置，可注入请求级时钟；唯一模块与生成应用提供公开 API 使用示例。

- 新增统一正文所有者 `Request`，提供 Laravel 式输入来源合并、dot/通配读取、存在性、筛选与输入修改；`Json` / `Query` 共享默认 trim/null 转换，公开 `InputNormalization` 支持路由或请求覆盖，原始正文保持不变。

- 新增只读 `RequestInfo` 提取器：检查路径、方法、严格 Host、头与 bearer，并按固定 Laravel/Symfony 行为执行内容协商；与 `Json<T>` 共存，限制元信息大小且 Debug 隐藏敏感内容。唯一模块示例展示 HTML/JSON 协商与正文读取。

- HTTP 路由支持 `RouteGroup::domain` 与 `DomainParams<T>`：域路由按声明顺序在普通路由前匹配，具名域 URL 由登记模板、单标签参数和 `app.url` scheme/port 生成；畸形 authority 失败关闭，转发 Host 不受信任，业务 404/405 不触发二次派发。

- HTTP 路由支持 `.name(...)`、按原样组合的组名 `name_prefix` 与 `RouteUrlGenerator`；路由参数安全编码，额外参数生成有序 query，URL 来源固定为 `app.url`。

- Provider 可登记可复用的 HTTP middleware alias 与递归 group，路由及嵌套组可按名称选择应用；未知名称、重名与 group 循环显式报错，无应用注册表时 `Routes::into_router()` 也会失败关闭。

- HTTP 路由新增 Laravel 式 `Routes::fallback`；支持静态 RouteGroup 前缀与 middleware，GET/HEAD 未命中时优先服务 public 静态文件（含 `/api`、`/ws`），原始 URI/body 传递给业务 handler，同 scope 重复 fallback 稳定报错。参数化/通配符组前缀暂报 `http_fallback_scope_unsupported`，避免 Axum catch-all panic。

- R1-E 在唯一 `modules/example` 中加入有所有者约束的 Article CRUD，并以实际业务路径展示租户、附件、缓存、队列、认证、实时通信、数据交换、国际化与观测等当前公开能力；具体真实服务范围和仍未关闭的边界见示例 README 与验收记录。

- `openrust new --grpc` 生成 typed Echo 服务与 Provider，并提供应用二进制 `serve all` 的 HTTP + TLS gRPC 联合运行测试。`--auth-mfa` 生成的 HTTP 验收现覆盖真实 TOTP 挑战、恢复码一次性与撤销；与 gRPC、认证、数据库组合时也生成可运行测试配置。

- R2-M 新增编译期 `ModuleDescriptor`、运行时 `ModuleManifest`/文件激活快照、模块配置与资源路径加载、命名空间视图和翻译，以及按全局名称/批次执行的统一迁移历史。根应用与生成项目从运行根读取模块状态；示例业务完整移至 `modules/example/`。管理写命令与完整发布流程仍属后继。

- R2-D 新增与生产共用 Provider/Router 启动路径的 `TestApp<A>`、可重放的 Provider 移除与注册后绑定覆盖、有界请求和安全响应断言；完整 `reset` 先关闭旧应用再重建。普通及认证项目生成模板包含对应测试，综合示例验证业务服务替换和路由移除。

- R2-C 为HTTP流式响应、命令、队列每次尝试和调度每次触发接入独立容器scope与受管清理；公开 `Scope::run`、`Scope::spawn`、`ScopeFactory` 和自定义队列driver可显式等待的 `JobAttempt`。

- 新增 R2-A 类型化容器基元及安全错误 Debug；生产 Provider/请求生命周期尚未切换。

- 新增共享固定窗口限流，以及使用 PostgreSQL/MySQL 存储、绑定用途的一次性验证码。

- 新增 OpenRust `Json` / `Path` / `Query` 提取器、`Created::with_location`，并为常见 HTTP 拒绝响应提供 JSON envelope 回退。

- 新增不发布的 `apps/server` 集成宿主，并设为 workspace 中 `cargo run` 的默认目标。
- 建立 Cargo workspace、共享 lint 策略、双许可证、CI 基线和框架架构决策。
- 新增应用核心，包括分层配置、类型化服务注册表、插件生命周期、运行时命令、结构化 HTTP 错误和可配置 JSON envelope。
- 新增 HTTP 路由、资源操作、类型化响应、校验与注入提取器、健康路由、中间件、CORS 和压缩。
- 新增 facade 应用运行入口、最小示例，以及可将运行时命令转发给项目二进制的 CLI。
- 新增贡献、安全和社区指南。

### Changed

- 应用关闭同步封闭容器的新解析与受管任务登记；`TestApp` 析构时即使没有运行时也先封闭，异步回收仍需显式 `shutdown` 或存活的 Tokio runtime。已持有的服务 `Arc` 由调用者管理。

- `App::run_command` 每次执行使用新scope，应用资源在显式关闭时回收；队列任务与调度触发不继承调用者的请求或租户身份。

- `ConfigLoader::figment()` 返回 `Result`，直接调用方须传播配置来源错误。

- 根 Cargo package openrust 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs；Laravel 应用职责目录回根，default-members=["."]、default-run="server"，根无参数 cargo run 启动唯一应用。

- 业务示例通过公开模块清单和统一 Provider 接入根唯一应用，专属迁移、资源、协议和测试位于 `modules/example/`；根不保留重复应用或 `examples/` 目录。CLI 生成项目也提供模块清单和文件状态入口。

- 根 CLI 运行命令按 Cargo metadata 默认应用转发，根 server 从根职责目录读取配置与静态资源。

- 错误响应中的 `timestamp_ms` Unix 毫秒数改为 `timestamp` RFC 3339 UTC 字符串（毫秒精度）。

- 将默认运行时配置命名空间切换为 `APP_`；应用可通过 `Application::config_namespace()` 选择一个自定义前缀。
- 将所有 OpenRust JSON 成功和错误响应统一为 `ApiResponse<T>`，并新增 `[api] http_status`（`always_ok` 或 `semantic`）、请求 ID 正文/响应头一致性及限流重试响应头。

- 将 facade prelude 切换为 OpenRust 提取器，并按照 ADR-008 统一校验、超时和未匹配路由响应。

- 按照 ADR-006 在已实现 API 中统一采用以下名称：`Plugin::build/finish`、`AppBuilder::plugin/insert/get`、`AppState::get/try_get`、`HttpPlugin`、`Routes::nest`，以及资源操作 `list/get/create/update/delete`。
- 将运行时配置切换为 `config/default.toml`、`OPENRUST_ENV` 和 `OPENRUST_<SECTION>__<KEY>`；`DATABASE_URL` 和 `PORT` 映射到对应配置区段，带前缀的变量优先。
- 将配置命令改为 `config show`，并记录未来的 `openrust generate` CLI 规划。

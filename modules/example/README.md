# 综合业务示例

> 当前源码位于 `modules/example/`，通过统一模块清单与 ServiceProvider 接入；隔离的 example_* 表 CRUD、当前已实现公开能力的业务调用与适用双库验收已由 R1-E 提交 `bc3fa01`。

`modules/example` 是根应用挂载的普通业务模块，供开发者学习如何在 `modules/<业务>/` 调用框架公开 API，以及通过公开 Provider/driver 契约扩展框架。框架、应用宿主和业务模块都在本仓 `main` 开发；这里没有独立 package、bin、启动模式或 example feature，也不复制完整应用骨架。

**综合示例已贯通当前已实现的公开框架能力。** 文章 HTTP CRUD、事务事件、审计、缓存、存储、账户安全及协议调用均有业务代码和分组验证；tenant 路由通过 `RouteGroup` 演示共享路径前缀与组级认证/授权 middleware。这不表示全部 Laravel 能力已完成。

`GET /api/example/route-url` 展示命名路由：`GET /api/example` 注册为 `example.index`，处理器从请求提取 `RouteUrlGenerator` 并生成该名称对应的绝对 URL。

`GET /api/example/representation` 使用公开 `RequestInfo` 按 Laravel Request 的内容协商方式调用 `prefers(&["html", "json"])`，将同一个 `ExampleMessage` 服务消息返回为转义后的 HTML 或统一封装 JSON，并发送 `Vary: Accept`。缺少或传入空 `Accept` 时应用明确选择 HTML；存在但无法匹配两种格式时返回 `not_acceptable` 错误。HTTP 状态遵循应用配置：`Semantic` 为 406，`AlwaysOk` 为 200 并在统一响应中保留错误码。`POST /api/example/request-body` 将只读 `RequestInfo` 放在 `Json<T>` 前提取，返回方法、路径、JSON 类型判断与输入标题，证明前者不读取或占用请求正文；正文仍由唯一的 `Json<T>` 提取器读取。这里仅演示 C04-A 的请求检查和协商，不代表完整 Laravel Request、表单、Session 或响应能力已经完成。

`GET http://acme.example.test/api/example/domain/orders/42` 展示 Laravel 式 domain group：在 `RouteGroup::domain("{account}.example.test")` 声明域参数，handler 用 `DomainParams<T>` 与 `Path<T>` 分别读取域和路径参数，再通过具名路由生成 URL。域名仅用于路由分派，不作为租户认证或授权依据；测试环境需用 `Host: acme.example.test` 请求。

`GET /api/example/middleware` 展示模块 Provider 如何通过 `BootContext::middleware_alias` 登记自有 Tower middleware、通过 `middleware_group` 组合，再由模块路由选择应用；集成测试验证只有该路由返回 `x-example-middleware: named-group`。

`GET /example/pages/about` 是可选页面路由；`/example/pages/*` 未命中页面时由同一 `RouteGroup` 的 `fallback` 返回页面 404，并继承 `example.response` middleware。其他路径仍使用框架 fallback；若 public 目录中存在对应静态资源，GET/HEAD 会先返回文件。应用也可以在生成项目 `routes/web.rs` 中直接参考 `Routes::fallback` 的写法。

模块扩展目标参考用户指定的第三方 [nWidart/laravel-modules](https://github.com/nWidart/laravel-modules)；当前依据为 [v13 文档](https://laravelmodules.com/docs/13/getting-started/introduction)，读取日期 2026-10-28，未宣称源码 revision 已对齐。借鉴自有路由/API、controller/model/resource、迁移、测试与 provider 装配；它不是 Laravel 核心，不扩张 Laravel 主基线，也不表示本框架已全量实现该包。按本项目约定采用 modules/<模块>、snake_case 和编译期静态注册。修改 modules/mod.rs 的编译声明和 Application::modules 注册清单是正常业务接入；为某个业务修改 src/ 核心或加入全局样例特判不属于接入方式。优先复用现有 ServiceProvider/Routes；Rust 模块编译期声明，无运行时源码扫描。

## 统一请求输入与路由规范化

`POST /api/example/input` 在 [`controllers/example.rs`](http/controllers/example.rs) 使用公开 `openrust::http::Request` 作为唯一读取正文的提取器。`request.info()` 提供方法和路径等元信息；不要在同一个 handler 中再并列 `Json<T>` 或原生 Request 消费正文。需要类型化数据时可调用 `deserialize_json`、`deserialize_query` 或 `deserialize_input`，示例用 `deserialize_query::<InputQuery>()` 把 `page=7` 转成 `Option<u32>` 并读取已去空白的 `label`。 `deserialize_json` 会经过动态 `serde_json::Value`；需要精确保留128位整数时直接使用 typed `Json<T>`，详见[HTTP 输入说明](../../src/http/README.md#统一请求输入)。

JSON 请求优先采用 JSON bag，表单请求采用 form bag，再与 query 做顶层左并集；正文中的显式 `null` 也遮住同名 query，不深合并嵌套对象。`input("profile.name")` 可读 dot 路径，`query("title")`、`post("title")` 只取直接键。处理器用 `only` 选择标题、姓名及演示数组，用 `except` 演示排除标题；`has` 计入 `null`，`filled` 对 `null` 为假。`merge` 可按 dot 写入，`merge_if_missing` 保留已有字段，`replace` 只替换当前输入 source 并保留 query fallback。代码分别记录修改后的少量业务字段；这些修改不会再次执行规范化。响应不回传密码或原始正文，只返回密码处理的布尔结果、原文长度与修改后原文是否保留。

```bash
curl -X POST 'http://127.0.0.1:3000/api/example/input?title=query&fallback=query-only&page=7' \
  -H 'Content-Type: application/json' \
  -d '{"title":"  body  ","profile":{"name":"  Alice  "},"nullable":""}'
```

默认先递归去首尾空白，再将空字符串转 `null`；仅完整顶层键 `current_password`、`password`、`password_confirmation` 排除 trim，嵌套 `user.password` 仍 trim，空密码仍可转 `null`。表单 `items[]=a&items[]=b` 形成连续数组；稀疏数字、负数与 `00` 等混合键保留为对象，删除数字项不会重编号。

[`routes/api.rs`](routes/api.rs) 用独立 `RouteGroup` 的 `.middleware(InputNormalization::disabled())` 为具名路由 `example.input-raw`（`POST /api/example/input-raw`）关闭两项转换，演示空白标题和空字符串原样保留；其他路由继续使用默认策略。应用可以在自己的组配置 `InputNormalization::default().trim_strings(true).convert_empty_strings_to_null(true).except_trim(["signature", "profile.token*"])`，或按请求向 extensions 插入策略进行条件覆盖。策略是公开 Tower Layer，不需要更改框架内核。

本簇没有解析 multipart 文本或文件、Cookie/Session 和 FormRequest。原生 `openrust::http::axum` 的 Request/Json/Query 是独立逃生口，保持 Axum 自身语义，不执行这里的受管理输入规范化。现有 `POST /api/example/request-body` 则继续展示框架 typed `Json<T>` 与只读 `RequestInfo` 的组合。

## 从真实调用链开始

根 [`src/main.rs`](../../src/main.rs) 声明应用宿主和已编译模块，然后运行 `bootstrap::Server`。[`bootstrap/app.rs`](../../bootstrap/app.rs) 的 `Application::modules` 返回模块清单，加入 [`descriptor`](mod.rs)；根 [`modules_statuses.json`](../../modules_statuses.json) 的 `example: true` 决定本次启动启用。descriptor 将 [`ExampleServiceProvider`](providers/example.rs)、自定义缓存 driver、以及适用数据库能力的任务/迁移/种子、文章事件和欢迎业务 Provider 纳入同一 register/boot 生命周期；禁用模块不会登记这些 Provider。默认模块根为进程工作目录，可由显式配置 `modules_root` 指定部署根；状态、配置、语言、视图和发布资源均从同一根解析。根 [`routes/web.rs`](../../routes/web.rs) 只保留框架通用路由，不硬编码业务 handler。

[`ExampleServiceProvider::register`](providers/example.rs) 登记 `example` 类型化配置/schema 和业务依赖工厂；其 `boot` 在正确配置时点调用 `ModuleConfigFiles::overlay`，读取模块 [`config/config.toml`](config/config.toml)，再贡献 [`routes::register`](routes/mod.rs)。后续业务工厂读取共享配置仓储，`greeting` 用于响应文字。模块配置覆盖既有同键值，与 Laravel 核心 defaults 的低优先级合并不同；不把整个模块的 overlay 提前到所有 Provider boot 之前。

模块 `GET /api/example` 由 `show` 返回 `Json<ExampleInfo>`，其中 `message` 来自 Provider 登记并经 `Inject<ExampleMessage>` 注入的业务依赖，`served_at` 直接调用第三方 `chrono::Utc::now()`；类型通过 serde 序列化，经过框架 HTTP 处理链。
从仓库根启动，在另一终端请求默认端口：

```bash
cargo run
```

```bash
curl --noproxy '*' http://127.0.0.1:3000/api/example
curl --noproxy '*' http://127.0.0.1:3000/health
```

默认基础示例不要求数据库或 Redis；端口与公开目录见根 [`config/server.toml`](../../config/server.toml)。文章及欢迎路由仅在配置数据库并编译认证能力后出现；无数据库启动不表示文章业务可用。

## 隔离的字段类型示例

`/api/example/records` 提供独立的列表、详情、创建、更新、删除接口，所有查询按当前登录用户隔离。它只操作 `example_records` 与 `example_record_categories`，不使用通用 `records`、`categories` 或开发者业务表。记录字段覆盖本仓生成器当前支持的可移植类型：`name`（string）、`description`（text）、`priority`（integer/i32）、`view_count`（big_integer/i64）、`enabled`（boolean）、`published_at`（nullable date_time/UTC）、`metadata`（nullable JSON）及 `category_id`（nullable references）。外键目标 `example_record_categories` 也属于示例命名空间。

示例 API 接受完整 JSON：`{"name":"样例","description":"说明","priority":1,"view_count":0,"enabled":true,"published_at":null,"metadata":{"source":"demo"},"category_id":null}`。该接口用于查看类型映射与 CRUD 写法；不会自动种入类别数据。综合示例的 API 路由统一置于 `/api/example/*`，网页置于 `/example/pages/*`，示例对象存储 key 统一以 `example/` 开头，避免覆盖应用自己的路由与对象路径。

## 文章与欢迎业务

数据库启用且编译 `auth` 时，`/api/example/articles` 使用公开 `ResourceActions` 注册列表、详情、创建、更新和删除。处理器用 `Validated` 校验标题与正文，页码限定 1–1,000,000、每页 1–100；列表通过公开 `Paginated` 返回 `data` 和 `meta`。`CurrentUser` 与 `QueryRule` 将读取限制为本人文章，更新和删除 SQL 同时筛选文章 ID 与 owner ID。缺失或他人文章均返回 `not_found`，写入在主库事务中完成。

文章创建、更新和删除在同一事务中调用 `Events::dispatch_in_transaction`，模块监听器写入 `example_article_events`，并把幂等的 `ArticleIndexJob` 插入同一持久队列事务；监听或入队失败会回滚文章写入。创建还从同一事务读取作者已验证邮箱，用公开 `Notifier::dispatch_in_transaction` 排入仅发给作者的确认邮件；入队失败同样回滚文章，测试 worker 使用捕获邮件驱动，不向真实供应商发送。显式 worker 在提交后按 owner/文章 ID 维护 `example-prefix` 命名 store 中 TTL 600 秒的派生标题，删除时移除对应键。`GET /api/example/articles/{id}/cached-title` 每次先在主库检查 owner，缓存 MISS 才从主库重建；默认 store 不受模块设置影响，应用可显式把该 named store 设为 Redis。标题是 eventual 派生值：异步 job 未消费或旧快照与新 job 交错时可能读到旧标题，单个写入的 TTL 为 600 秒，队列长期故障时不承诺端到端 600 秒内最新；授权与普通 CRUD 始终由主库决定。详情成功读取经公开 `Audit` 记录 actor、文章与结果，越权读取不会产生成功审计。

文章 HTTP 创建、更新和删除的完整事务结果使用既有 tracing span 记录固定操作与结果；启用 `metrics` 时产生 `example_article_operations_total` 与 `example_article_operation_duration_seconds`。标签仅有 `operation`、`outcome`，不包含账户、文章 ID、邮件地址、正文或错误内容。取消记为未完成的 `cancelled`，不推断数据库回滚；批量导入事务内的单次 `create_in_transaction` 不提前记作已提交。`/metrics` 仍须按框架配置显式开放并限制采集网络。

编译 `encryption` 时，所有者可通过 `PUT/GET /api/example/articles/{id}/private-note` 保存和读取私密备注。主库只保存 `Encryptor` 密文；认证上下文固定绑定 owner ID 与文章 ID，列表和公开文章资源均不包含备注或密文。私密备注输入最多 10,000 字符，越权与不存在返回 404；响应明确设置 `Cache-Control: private, no-store` 与 `nosniff`。双库真实 HTTP 回归还验证密文不能在另一文章上下文解密。密钥需由应用安全配置，测试密钥不适用于生产。

编译 `storage-local` 时，所有者可通过 `PUT/GET/DELETE /api/example/articles/{id}/attachment` 上传、下载和删除一份 UTF-8 文本附件。服务端构造 `example/` 前缀下 owner/文章专属对象 key，拒绝客户端文件名和其他 MIME；流大小 1 MiB、分块 16 KiB，逐块校验 UTF-8 与控制字符。下载强制固定文件名、`nosniff` 与 `private, no-store`。上传与文章删除使用同一文章行锁串行化；文章删除在数据库事务中写入持久清理意图，提交后才删除对象。上传在 App 资源所有者中受管运行并保留启动时的租户作用域；15 秒 HTTP 等待截止后发布任务继续收尾，文章删除需等该任务完成。45 秒期限约束请求体读取至 EOF；EOF 与期限由共享原子状态仲裁：期限先获胜时，流不能以正常 EOF 交给 storage，因此不会进入对象发布；EOF 先获胜时，发布任务继续持有数据库锁直到存储调用收敛，不因 HTTP 等待超时释放事务。真实 MinIO 代理暂停签名 CopyObject、确认其已向 MinIO 返回成功状态后，PostgreSQL 18 与 MySQL 8.4 各自的竞态回归验证 DELETE 等待复制完成，并在删除后确认最终 key 不存在且 staging 已清理。请求体挂起的 deadline/锁释放回归在 PostgreSQL/MySQL 也各通过 1/1。App shutdown 到达自身 deadline 后仍可能中止任务；进程被强杀、MinIO 永久无响应、网络请求永不收敛，或 CopyObject 已发送后返回 `storage_publication_uncertain` 且远端稍后完成时，不保证数据库与对象存储原子提交或自动消除孤儿。已提交文章删除的清理意图可由 `example attachments reconcile` 幂等重试；重用文章 ID 时，遗留意图会阻止新附件上传并拒绝清理，避免误删新对象。此机制不等于病毒或敏感信息扫描。本地附件回归使用生产 Router 的 `TestApp` 与真实 PostgreSQL/MySQL；S3 回归另外使用隔离 MinIO bucket，包含两独立 App 的真实 TCP 分块上传、下载与越权拒绝、挂起新对象不可见、挂起覆盖保留旧对象、复制在途时删除等待并清理、取消及持久清理意图恢复。S3 路径目前仍需同时编译 `storage-local` 与 `storage-s3`，并显式把默认 disk 配成 S3；发布与运维边界见 [存储说明](../../src/storage/README.md)。

编译 `data-exchange` 时，认证用户可通过 `POST /api/example/articles/import?format=csv|xlsx` 导入、`GET /api/example/articles/export?format=csv|xlsx` 导出自己的文章。输入最多 1 MiB、100 行，先完成有界解析，再逐行调用同一 `ArticleWriter`；合法行各自提交，报告只给行号、白名单字段和稳定错误码，前序成功行不会因后续坏行回滚。导出只含 `title`、`body`，最多 100 行、2 MiB，不含 owner、私密备注或密文；CSV 公式前缀按框架策略转义，不能承诺逐字导入往返。编译 `data-exchange-tasks` 和 `storage-local` 时，还可通过 `POST /api/example/articles/export-jobs` 排队导出，并经 `GET /api/example/articles/export-jobs/{job_id}/{attempt}` 按当前账户读取本地私有对象；框架清理任务在保留期后删除对象。标准应用迁移包含此能力所需共享锁表。

同样的 feature 组合提供 `POST /api/example/articles/import-jobs?format=csv|xlsx`，将服务端生成的 owner 专属导入来源写入私有本地存储并排队；所有者可读取 `/api/example/articles/import-jobs/{import_id}/status` 与 `/api/example/articles/import-jobs/{import_id}/{job_id}/{attempt}` 报告。每批最多两行，在同一数据库事务中复用文章写入、事件与通知规则；失败批次回滚，已提交批次保留，重试通过现有任务幂等记录避免重复提交。恢复窗口为 24 小时，超过窗口的未完成导入转为可见的 expired 终态，不把来源过期删除误报为成功；终态收据永久保存以拒绝旧 payload 重放，会持续占用数据库行，需要按业务保留政策人工规划容量。来源清理任务仅在终态或安全过期后删除对象；租约忙或未完成时有界延期。首次清理入队失败或延期耗尽时，可在迁移完成后运行 `example imports reconcile`，每次按导入 ID 升序最多检查 100 条未回收收据，输出安全计数与下一游标；下一页运行 `example imports reconcile <游标>`，直到输出 `next cursor none`。单条失败会计数并继续本页，命令返回失败；完成翻页后从首页重跑处理先前失败项。必须实际调度该维护命令。外部 worker 强制停止后租约最长需等待 5 分钟过期，24 小时后旧任务不能继续写入；存储与队列不是跨系统原子提交，报告和外部邮件交付也不宣称永久 exactly-once。

编译 `realtime` 时，当前文章所有者可经 JWT Bearer 请求 `GET /api/example/articles/{id}/events` 建立 SSE，或升级 `GET /api/example/articles/{id}/socket` 为只读 WebSocket；通过 `POST /api/example/articles/{id}/announce` 向自己在该文章频道的在线连接发布事件。两类连接要求 `[realtime].allowed_origins` 精确允许请求 Origin；缺省空列表拒绝浏览器来源。模块重新校验同一签名 token，并使用其实际 `exp` 截止连接，不自行延长凭据；文章权限在连接和发布时分别查询。`/socket` 握手采用 `Authorization: Bearer`，适用于能设置该头的原生或服务客户端。浏览器原生 `WebSocket` 使用独立的 `/browser-socket` 路由，以同请求已认证 Cookie Session 和精确 Origin 校验，不接受 Authorization 或 URL query 凭据；`SessionStoreHandle::authorized_until` 读取持久记录中的账户与原截止时间，不续期。两种入口都先查主库文章所有权；Session 登录和 CSRF 仍经账户公开入口。协议级真实 TCP 回归已覆盖 Session 路径；Chromium 153 原生浏览器的 PostgreSQL/MySQL 定向场景各1项已验证。浏览器用例与握手拒绝的精确 HTTP 状态分别验证，不能外推到所有浏览器。已建连接按握手时的截止时间结束，不承诺另一个请求登出后立即关闭已有 WebSocket。发布成功表示本机队列已接受，不能保证远端客户端收到；双库回归验证真实 TCP 升级和两个独立 App 经真实 Redis 传输，不等于两 OS 进程集群。

编译 `http-client` 且运营配置 `example.article_summary_origin` 时，所有者可通过 `GET /api/example/articles/{id}/summary` 请求固定上游的摘要。配置必须是无凭据、查询、片段和额外路径的 HTTP(S) 根地址；请求方不能提交 URL。调用先检查文章归属，再以 2 秒连接/5 秒总时限及 16 KiB 响应上限调用公开 `HttpClient`，非成功状态、无效内容和超限均返回不含上游地址的 502。双库真实 HTTP 回归使用本地 loopback 上游验证成功、越权、503 与超大响应；另有真实本地 TCP 上游的头前停滞、部分正文停滞与取消后 EOF 回归；RP 请求取消部分使用生产 `TestApp` oneshot，不能称整个取消用例为双端网络。服务没有因此获得商业上游可用性或远端业务取消保证。

`POST /api/example/welcome` 是显式 JWT 认证业务动作，按账户限流且只接受空 JSON 对象。服务从主库按当前账户 ID 读取未停用、已验证的邮箱，只向该邮箱发送；请求中的第三方邮箱或电话会被拒绝。邮件用公开 `LocalizedEmail` 与模块语言目录按请求语言生成主题/正文，真实 TCP 回归覆盖英文和简体中文。现有账户模型没有可验证归属的手机号，因此 HTTP 入口不发 SMS。仅测试编译的受控业务调用演示标准 `Mailer` 与 `SmsSender`：两个目的地发送前共同校验，两通道没有跨供应商原子事务。启动不会自动发送。[`tests/extension_flows.rs`](tests/extension_flows.rs) 用 `TestApp.configure_after_register` 安装仅测试环境可用的捕获 driver，验证内容、失败传播和生产拒绝，不向真实供应商发送。

编译 `signing` 时，所有者可通过 `POST /api/example/articles/{id}/share` 签发 5 分钟文章阅读 token，并通过 `/share/revoke` 撤销。公开 `GET /api/example/shared-articles/{token}` 先校验用途、签名、有效期和共享数据库撤销状态，再按签名内 owner 与文章 ID 读取；文章删除后旧 token 也不能再读。token 是 bearer 凭据，调用方须避免把完整 URL 写入访问日志或公开渠道。双库真实 HTTP 测试覆盖他人签发拒绝、公开读取、他人撤销拒绝与所有者撤销后拒绝；不把此链接当作账户登录。

上述 Article CRUD、审计与事务事件路径在 PostgreSQL 18 和 MySQL 8.4 的真实 HTTP 测试中通过；当前公开能力的综合示例和适用门禁已执行，Collector 双库 delivery/unavailable 均有真实服务证据。捕获证据不算 SMTP/Twilio 实发。

## 账户安全与外部身份

编译并配置 `auth-mfa` 时，`/api/example/account` 展示既有 `AuthFlows` 的 Session 偏好、同源登录与 CSRF 保护，以及 JWT 账户的 TOTP 启用、恢复码轮换和停用。管理操作重新验证密码；启用 MFA 后旧 Session 逐请求拒绝，登录必须完成一次性挑战。旧 access JWT 不会因此自动全局撤销，不能把此示例当作立即登出的保证。账户路由的成功与错误响应均由外层设置 `Cache-Control: no-store`；测试 Session cookie 名称来自同一 Provider 配置。

配置 `example.oidc` 且编译 `auth-oidc` 后，模块才挂载 `/api/example/oidc`。Provider 在启动时完成受限目标的 discovery，并要求同容器中的 `Db`、`AuthFlows` 与 `Encryptor` 可用；模块路由登记之后再覆盖 OIDC 配置会在 Ready 前拒绝。登录回调验证浏览器绑定 cookie、state 与授权码，不按相同邮箱自动合并账户。JWT 原生客户端可显式 bind/unbind；浏览器绑定从已认证 Session 的 `/bind/authorize` 发起，以同源 CSRF 和短时意图绑定账户与浏览器，回调仅准备一次性确认，再由 `/bind/confirm` 复验密码。确认页只在 HTTP 成功且统一响应 envelope 的 `success` 为真、`data.status` 为 `bound` 时显示成功；测试显式 `AlwaysOk` 模式下，错误密码或已消费 state 虽返回 HTTP 200，仍显示失败。显式运行 `browser_confirmation_reports_success_and_failures_from_actual_page` 会启动 Chromium 执行生产 HTML/JavaScript，覆盖成功、业务失败、HTTP 错误和非法 JSON；本地 mock 只验页面判断，不替代真实 Keycloak/数据库验收。意图终态保留 tombstone，以免旧 state 在过期后绕回普通登录或重新绑定；这会持续占用数据库行，当前没有自动压缩，运维需监控表容量，不能按本地 10 分钟 TTL 直接删终态。浏览器测试使用真实 Keycloak 授权网络和随机端口的真实 RP HTTP，但测试夹具截取 IdP 固定回调地址并转交相同路径；不宣称完整浏览器自动导航已验收。

登录回调经 `ExternalIdentityFlows::complete_with_handle` 返回外部登录关联 handle；启用 MFA 的账户只能得到挑战，不能先拿 token。一次性挑战完成后，refresh 家族沿既有接口关联；`/session/attach` 只允许把**已登录的同账户 Session**关联到该 handle，并重新验证密码、Origin 和 CSRF，不是 OIDC 直接创建 Session 的替代实现。`POST /logout` 先撤销本地关联 refresh/Session，再返回固定 IdP end-session URL；同 callback 的 state-only 分支消费退出 state，不循环重定向。公开 `/backchannel` 只接受框架验证通过的签名 logout token，按 issuer、audience、sid/sub 和 jti 处理撤销与重放；不能用未验证的请求字段选择账户。`tests/oidc_lifecycle_flows.rs` 在真实 Keycloak 与双库 RP TCP 中覆盖 MFA 门、一次性挑战、关联退出、真实签名 backchannel/重放、有效签名 wrong-audience 拒绝及伪签名无副作用。自包含旧 access JWT 仍可能有效至 TTL；本地 logout 不等于所有 IdP 会话均已退出，也不保证已建 WebSocket 即时断开。

## 租户业务：先授权，再读取派生数据

[`http/controllers/tenant.rs`](http/controllers/tenant.rs) 的路由链是 JWT `AuthLayer` → 主库 membership 校验 → 私有 `VerifiedTenant` extension → 公开 `TenantLayer` → handler。`X-Example-Tenant` 只是 selector，不能自行成为可信身份；模块没有允许用户自授权 membership 的 API。[`services/tenant.rs`](services/tenant.rs) 用 `QueryRule::for_tenant` 与 owner 条件读库，写 SQL 同时限定 tenant/owner；每次访问先查真实成员和记录，再读取 Cache/Storage。

`GET/PUT /api/example/tenant/record` 维护最小租户记录，`POST .../record/rebuild` 经持久 Queue 派发投影，`GET/PUT .../record/file` 演示对象存储。Cache/Storage 复用框架 `TenantContext` 分区，worker 从任务 metadata 恢复上下文并重新核对 membership、记录和 payload，不另造租户队列。相同逻辑 key 在两个 tenant 中物理隔离；同一 owner 也不能跨 tenant 读写。真实 worker 测试覆盖缺 metadata、篡改为无权限 tenant、入队后撤销 membership，验证无未授权投影；这不声称能防御拥有数据库完整写权限者任意伪造授权，也不宣称撤销能回滚已开始的 Storage IO。任务仍遵循 at-least-once，副作用须按业务幂等处理。

附件清理意图保存上传时的 tenant 快照；在另一 tenant 的 ambient context 执行会拒绝并保留意图，无 tenant 的维护入口按原快照恢复分区清理。[`tests/tenant_s3_flows.rs`](tests/tenant_s3_flows.rs) 使用真实 Redis、MinIO、双 App 与隔离数据库，检查同 tenant 跨 App 一致、不同 tenant 同逻辑 key 隔离、真实 worker 投影及 A 附件清理不删 B 对象；这仍是同进程实例测试，不是 R14 多进程集群验收。

## 其它公开能力的业务入口

- **默认 Redis 的 tag/SWR**：`GET /api/example/articles/{id}/cached-title/swr` 在主库 owner 授权后使用 `Cache::default_store`、tag 和 `get_stale_while_revalidate`；新鲜期 30 秒、陈旧期 120 秒，文章索引 job 在提交后消费并失效对应 tag。显式默认 Redis 的双库回归已覆盖该业务，不静默回退到 memory。它与 `example-prefix` named driver 是两个示例；两者都不是授权来源，队列未消费时允许旧派生值。
- **本地化预览**：`GET /api/example/reading-summary` 的 [`http/controllers/localization.rs`](http/controllers/localization.rs) 通过 `RequestLocale`、模块消息的 CLDR 复数及 `I18n::format_date/format_datetime` 返回阅读摘要预览；输入为受限 count、RFC3339 at 和 `utc`/`utc+08:00` 固定偏移，不伪装为真实阅读统计或 IANA/DST 时区实现。真实 TCP 测试覆盖英中差异、复数、长日期与跨日边界。
- **视图发布**：`GET /example/pages/about` 调用 `Views::render("example::pages/about.html", ...)`。生产模块优先使用 `resources/views/vendor/example` 发布副本，缺失时回退模块模板；[`tests/view_flows.rs`](tests/view_flows.rs) 经真实 TCP 验证源模板、发布覆盖、回退、转义和错误边界。
- **显式种子与重试**：`database/seeders` 实现公开 Seeder，由 `db seed` 或测试显式调用，boot 不写库。种子使用固定负 ID 命名空间、负数 owner ID 和碰撞 fail-closed；不在框架 `users` 表创建种子账户；重复执行保留用户后续编辑。`tests/queue_retry_flows.rs` 使用真实数据库/Redis worker 展示失败退避、恢复、终态与公开重试，不以直接调用 Job 冒充 worker。
- **gRPC TLS 与组合监听**：[`grpc.rs`](grpc.rs) 的 Echo 与 HTTP 读取同一个 `ExampleMessage`。[`tests/grpc_tls_flows.rs`](tests/grpc_tls_flows.rs) 使用同一生产 App 的公开组合函数启两个 listener/两个端口，真实客户端验证 CA、hostname、HTTP/Echo 响应和有界联合停机。不是同端口复用、根 `serve all` 自动装配或完整 R11 验收。
- **业务观测与真实出口**：[`services/telemetry.rs`](services/telemetry.rs) 的 `observe` 只记录固定操作/结果；[`tests/collector_flows.rs`](tests/collector_flows.rs) 由独立子进程连接受控 Collector，traces 走 OTLP/HTTP，metrics 由 Collector 抓取 Prometheus。PostgreSQL 18 与 MySQL 8.4 的 delivery/unavailable 均已通过真实 Collector 验收；PG 使用任务专属 Collector、独立活跃 sink 与唯一 run ID，保持全文件 16 MiB 上限及私密哨兵断言。详细证据见项目测试记录。不称 OTLP metrics，也不称入站 W3C parent 已接入。无商业出口授权时不向供应商发送。

外部 [`test-app consumer`](../../tests/consumers/test-app/) 使用公开 TestApp；其可选 `session` 编译契约确认 `SessionStoreHandle::authorized_until` 从独立 crate 可调用，**不代替真实认证 Session 的行为测试**。真实截止时间、owner/撤销与浏览器握手以相应双库业务测试为证据。

## 新建自己的业务模块

以 `modules/orders/mod.rs` 为例，通过公开描述和普通 Provider 贡献路由：

```rust
use openrust::prelude::*;
use openrust::core::{BootContext, BoxFuture, ModuleDescriptor, ServiceProvider};

struct OrdersServiceProvider;
impl ServiceProvider for OrdersServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.routes(register)?; Ok(()) })
    }
}
pub(crate) fn descriptor(root: &std::path::Path) -> ModuleDescriptor {
    ModuleDescriptor::new(
        "orders",
        root.join("modules/orders"),
        0,
    ).provider(OrdersServiceProvider)
}
fn register(routes: Routes) -> Routes { routes.get("/api/orders", index) }
async fn index() -> Json<Vec<&'static str>> { Json(vec!["first-order"]) }
```

在根 `modules/mod.rs` 声明 `pub(crate) mod orders;`，并在应用 `Application::modules` 中将 `modules::orders::descriptor(root)` 加入 `ModuleManifest::from_file(root.join("modules_statuses.json")).module(...)`；状态 JSON 设为 `{"orders":true}` 才启用。编译声明与运行时启用分开，只有已声明且启用的模块进入唯一 Provider 清单。部署时从应用根启动，或显式提供同一 `modules_root`；编译机路径不参与运行时查找。

若 `orders` 在启动时必须读取 `shared` 模块注册的服务，两个模块都要编译声明并启用，消费者用 `requires` 表达硬依赖：

```rust
ModuleManifest::from_file(root.join("modules_statuses.json"))
    .module(modules::shared::descriptor(root))
    .module(modules::orders::descriptor(root).requires("shared"));
```

依赖模块先于消费者完成 Provider 注册与 boot；该关系优先于 `priority`，可启动模块之间仍按升序 priority 和清单声明顺序稳定排序。未编译、未启用、自依赖、重复依赖和循环会使应用构建失败，框架不会自动改写启用状态。Cargo 管理 crate/features 依赖，`requires` 只处理应用模块间的运行装配依赖。

业务路由留在业务模块，静态列表只说明注册，不代替完整 CRUD。register 阶段绑定服务，所有 Provider 注册后依序 boot；第三方依赖仍由 Cargo 解析。禁用只影响后续应用启动，不对已运行进程热卸载；更完整的生成、发布和模块管理命令仍待后继实现。

## 模块里各类代码写在哪里

以下位置已存在，按实际职责拆分；没有业务实现的 models 等目录不预建：

| 实际位置 | 写什么 |
| --- | --- |
| mod.rs | 模块声明，保持薄入口 |
| providers/example.rs | register 登记配置与业务依赖，boot 合并模块配置并贡献路由 |
| config/config.toml | 在对应 Provider boot 时点覆盖 example 区段的模块配置 |
| services/message.rs | 实际供控制器调用、测试可替换的问候内容 |
| routes/api.rs、routes/web.rs | API/页面路由组合，调用 controller |
| http/controllers/example.rs、article.rs、attachments.rs、welcome.rs、auth.rs、jobs.rs、pages.rs | 实际请求处理 |
| http/requests/article.rs、attachments.rs、welcome.rs、auth.rs | 文章/附件校验、分页、欢迎空对象与认证输入 DTO |
| http/resources/article.rs、example.rs、credentials.rs、about.rs | 文章、认证及页面输出 |
| database/migrations/mod.rs、database/seeders/mod.rs | 模块业务迁移与种子 |
| resources/views/ | 业务模板 |
| jobs/ | 当前心跳任务和调度插件 |
| extensions.rs、services/welcome.rs、services/attachments.rs | 已挂载 named 缓存 driver、欢迎业务与仅测试的通信捕获；附件服务接公开 Storage |
| events/article.rs、models/article_event.rs | 事务内文章事件及模块事件表 |
| tests/auth_flows.rs、tests/attachment_flows.rs、tests/registration.rs、tests/extension_flows.rs | 双库 HTTP、附件流、模块迁移/启停与生产 TestApp 扩展测试 |
| proto/、generated/、grpc.rs | 实际 Echo 协议、显式生成脚本、类型与公开 gRPC 服务注册；TLS/双端口组合测试位于 tests/grpc_tls_flows.rs |

文章实体位于 models，校验/资源分别位于 http/requests 与 http/resources；事务事件已有实际模块监听器。其余当前公开能力仍按实际业务调用逐项补齐，不提前堆空层。

## 用 TestApp 测试自己的模块

[`tests/registration.rs`](tests/registration.rs) 直接使用根生产 `crate::bootstrap::Server`，复用相同 `Application::providers` 与 `Application::modules`，没有测试专用 Application 或第二容器。业务调用链是模块 descriptor → [`ExampleServiceProvider::register/boot`](providers/example.rs) → 配置驱动的 [`ExampleMessage`](services/message.rs) → [`show` 的 Inject](http/controllers/example.rs)。替换绑定不会改路由或控制器。

测试中的 `production_sources` 显式使用测试环境、内存缓存和测试加密密钥，不读取或修改进程环境，也不要求 Redis；这是测试夹具选择，生产缓存默认没有回退。使用公开 API 替换业务依赖的实际写法如下，完整可执行函数见同一测试文件：

```rust
use openrust::testing::TestApp;
use std::sync::Arc;
use super::services::ExampleMessage;

let mut fixture = TestApp::<crate::bootstrap::Server>::builder(production_sources(false));
fixture.configure_after_register(|bindings| {
    bindings.instance(Arc::new(ExampleMessage::new("A test-owned business message")));
    Ok(())
});
let mut app = fixture.build().await?;
let response = app.request(Request::get("/api/example").header("host", "localhost").body(Body::empty())?).await?;
response.assert_status(StatusCode::OK)?;
let json: serde_json::Value = response.json()?;
assert_eq!(json["data"]["message"], "A test-owned business message");
app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5)).await?;
```

替换在全体 Provider register 完成后、容器封口前生效。示例同时创建未替换与替换后的两个应用，检查内容互不污染；每个应用都显式 `shutdown` 等待框架拥有的异步资源关闭。完整 reset 会关闭再重建应用；闭包故意共享的外部 `Arc` 不会自动清空。

第二项测试用 `remove_module("example")` 从本次启动清单移除业务模块，确认 `/api/example` 不存在而 `/health` 保留。默认语义状态直接返回 HTTP 404，响应正文 `code = "not_found"`；另有显式 `always_ok` 模式回归保护可选覆盖。

```bash
cargo test -p openrust --bin server production_testapp_
```

这些请求经生产 Router 和 scope 的 Tower oneshot 执行，属于进程内行为测试，不是 TCP 网络证据。默认响应收集限制为 1 MiB/5 秒，可用 `request_with_limits` 指定；流式响应使用 `raw_response`，由调用方消费或丢弃 body。上述新增测试的实际通过情况以本簇验收记录为准，代码存在不代表已执行。

## 使用业务自己的第三方依赖

`modules/example` 是普通 Rust 模块，不是 Cargo package；在其子目录放 Cargo.toml 不会自动产生独立依赖边界。当前根 openrust package 同时构建框架 lib 和 server，业务依赖声明写在根应用 Cargo.toml，lib/server 的依赖解析与 feature 选择存在共享影响。“无需改框架源码”允许修改应用依赖与模块注册清单，不能解释成依赖图完全不变。

例如仓库已经登记并直接继承 serde、serde_json 和 chrono：

```toml
# 根 Cargo.toml 中的既有条目，不要重复追加
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }

[dependencies]
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
```

`http/controllers/example.rs::show` 已实际调用 `chrono::Utc::now()`，`http/resources/example.rs::ExampleInfo` 保存 `chrono::DateTime<chrono::Utc>` 并 derive `serde::Serialize`；认证输入也 derive `serde::Deserialize`。业务可直接导入这些已声明的包；下面是可用于新业务模块的序列化用法，不需要再添加依赖：

```rust
use serde::Serialize;

#[derive(Serialize)]
struct OrderLabel {
    name: String,
}

fn encode_label(label: &OrderLabel) -> Result<String, serde_json::Error> {
    serde_json::to_string(label)
}
```

真正新增业务包时，先确认包名、版本要求、许可证与所需 feature。若根 `[workspace.dependencies]` 已有同包，优先在根 `[dependencies]` 通过 `workspace = true` 继承；否则在前者登记核实后的明确版本约束，再在后者继承，不为教学随意引入生产依赖或编造版本。只用于测试的包写根 `[dev-dependencies]`；公共复用的版本政策同步 stack.md。声明位置和版本规则见 [Cargo 依赖说明](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)。

只启用业务需要的 feature；`default-features` 按实际需要设置，继承项的默认 feature 策略应从 workspace 声明协调。不要随意改框架依赖版本或默认 feature，不用无差别 `cargo update`，也不以 `[patch]` 强行覆盖核心依赖。package alias 解决导入名称或同时引用多版本的命名问题，不隔离同一依赖的 feature，也不会消除类型/解析冲突；Cargo 的 feature 合并规则仍生效。见 [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)。

审查根 Cargo.toml/Cargo.lock 的实际差异，先使锁文件精确反映获准的清单变动，再验证锁定结果：

```bash
git diff -- Cargo.toml Cargo.lock
cargo tree --locked -d
cargo tree --locked -e features
cargo check --locked -p openrust --all-targets
cargo test --locked -p openrust
cargo deny check
```

重复版本和 feature 来源可按 [cargo tree](https://doc.rust-lang.org/cargo/commands/cargo-tree.html) 继续定位。以上是验证步骤，不表示本轮已替开发者添加新包或执行过依赖演练。示例协议/生成脚本仍留模块内，不能因为第三方包需要代码生成就新增示例专用根 build.rs/build 依赖。

若将来确需更强的业务依赖隔离，同仓 workspace 的独立 package 可以另行设计；当前 root lib 与示例若相互依赖会产生环，普通目录也不是 feature 隔离。本轮不实施第二套 package 架构，不承诺完全独立的业务依赖图；任何此类调整须先收敛依赖方向及验收。

## 通用挂载与模块资源边界

除 modules/example 外都是框架结构与代码。模块专属代码、迁移、种子、模板、协议、生成脚本及测试 fixture 全放模块内；根职责目录只提供通用框架结构与公开加载/挂载，不存样例业务或示例专属分支。模块可按实际需要有资源目录，但不复制一套完整框架骨架。

| 职责 | 边界 |
| --- | --- |
| 通用启动、模块声明与公开挂载 | 根 modules/mod.rs、bootstrap 和 routes；核心库不依赖业务 |
| 框架通用配置与加载 | 根 config；示例专属配置留模块并显式通过公开 API 接入 |
| 示例业务迁移、种子与数据 | modules/example 内实际业务文件；根 database 仅通用结构/挂载 |
| 示例模板、语言和其他资源 | modules/example 内实际需要的资源；根 resources/public 仅通用结构/加载 |
| 示例协议与生成脚本 | modules/example 内实际需要的协议/脚本；根不因示例增加 build.rs 或 build 依赖 |
| 示例专属测试与 fixture | 模块内；根 tests 可保留通用验收并通过公开 API 挂载模块 |

示例实际迁移/种子在 database/migrations/mod.rs 与 database/seeders/mod.rs，模板在 resources/views，语言资源在 lang/，测试在 tests/ 下；这些均位于本模块内。协议输入为 proto/echo.proto，显式生成脚本为 proto/generate.py，工具清单与锁固定在 proto/generator.toml 和 proto/generator.lock，已生成类型为 generated/echo.rs；脚本在 /tmp 临时目录执行 cargo run --offline --locked，根没有为示例保留 build.rs/build 依赖。当前源码的默认/全特性 workspace、双库业务、生成项目及独立 consumer 门禁已执行并于 R1-E 提交 `bc3fa01`；各项真实证据见 [R1-E 验收记录](../../.agents/project/memory/testing.md#r1-e-业务示例定向验收证据整簇进行中)。

模块之间的硬依赖在应用清单通过 `ModuleDescriptor::requires("shared")` 声明。依赖模块必须同时编译并启用，否则应用构建会报错；依赖图决定必须先启动的模块，`priority` 只用于当前可启动模块之间排序。框架不会自动更改 `modules_statuses.json`，不会因启用业务模块而静默启用其依赖；本契约不提供包版本求解或热加载。
数据库、认证、队列等按现有 Cargo feature 和根配置启用。根应用检查显式数据库声明后装配数据库基础设施；没有声明表示未启用数据库业务，配置已声明但依赖不可用时必须报错，不回退到内存或空实现。模板通过 `view` 与应用显式视图目录启用；模块 Provider 在 boot 登记 `example::` 命名空间，优先读取 `resources/views/vendor/example` 的发布副本，缺失时回退模块 `resources/views`。`APP_VIEW_DIRECTORY=modules/example/resources/views` 仍可作为应用基础目录，但模块命名模板无需依赖该值；Protobuf 已生成类型按 `grpc` 编译，修改 proto 后显式运行 `python3 modules/example/proto/generate.py`，通过 `python3 modules/example/proto/generate.py --check` 比较重生字节；启用 feature 本身不等于已注册服务或实际监听端口。

## 当前能力覆盖与缺口

“代码存在”仅说明当前可见实现，不能替代本布局 check/test、真实请求与外部服务验收。根基础设施装配、文档链接或 consumer 测试也不自动算作 `modules/example` 的业务代码覆盖。

| 能力 | 本模块实际代码/公开扩展 | 根接入与条件 | 测试/当前缺口 |
| --- | --- | --- | --- |
| 完整业务实体 CRUD | `models/article.rs`、Article 控制器与 `ResourceActions` 覆盖列表、详情、创建、更新、删除 | 模块文章/事件迁移；`auth` 与数据库启用时挂载 | PG18/MySQL8.4 真实 HTTP 已覆盖两用户/匿名、校验/分页/404/越权、事务回滚；当前源码整组门禁已执行，本簇仍待独立审查和提交 |
| HTTP 路由与 JSON 响应 | `routes/api.rs`、Article `ResourceActions`、统一错误 envelope、业务限流与安全响应头 | ExampleServiceProvider 公开贡献模块 routes，包含 `/api/example` | 根 server、双库真实 HTTP CRUD 与各业务路由定向通过；当前源码默认/全特性整组门禁已执行 |
| 配置、应用生命周期、命令 | 模块类型配置与 boot 时 overlay；显式 `example attachments reconcile`、`example imports reconcile` 维护命令 | 根 bootstrap、config、唯一 server 与模块 Provider | TestApp 启停及配置边界已测；命令的生产运行/退出组合仍需最终门禁 |
| 数据库、迁移、种子、事务、查询 | Article 实体、owner 约束查询与主库事务；模块事件监听器同事务写入；显式种子命令创建不可登录隔离身份与两篇固定负 ID 文章 | 模块 database/migrations 与 database/seeders，descriptor 经公开 Provider 挂载；对应 db feature 与真实服务 | 双库 HTTP/迁移及真实 PG/MySQL `db seed` 各3项通过；boot 不写种子、并发重复幂等、碰撞整笔回滚、编辑保留 |
| JWT/PAT、Session、MFA 与 OIDC | 认证 HTTP、账户 Session/CSRF/MFA、OIDC 登录及浏览器/原生绑定调用公开 AuthFlows/ExternalIdentityFlows | 对应 auth feature、数据库迁移、Session/加密及可信 IdP 配置 | 账户与绑定回归及真实 Keycloak lifecycle 双库各5项定向通过，含 MFA/关联撤销/签名 backchannel；浏览器完整自动导航仍待验，旧 JWT 不提供即时全撤销 |
| 邮件及公开邮件实现 | `http/controllers/auth.rs::AuthCodeEmail` 实现 `Email`，`send_auth_code` 使用 `Mailer` | 根 mail 配置与 `MailerServiceProvider` | 认证测试有捕获 MailDriver；SMTP/真实发送须独立授权和条件，不能从 fake 推断 |
| 自定义缓存/邮件/SMS driver | `PrefixCacheServiceProvider` 已生产挂载 named driver；欢迎 HTTP 只向已验证账户邮箱发送，双通道 SMS 业务调用仅在受控测试编译中执行 | 标准 Cache/Mailer/SmsSender Provider 保留，捕获驱动仅 TestApp 安装 | 7项生产 TestApp 扩展回归；未验证手机号归属前不开放 HTTP SMS，SMTP/Twilio 实发仍待独立验收 |
| 队列与调度 | `JobsServiceProvider` 注册 heartbeat 与幂等 `ArticleIndexJob`，文章事件在事务内排入持久队列 | `queue`、数据库与模块迁移的公开挂载；API/worker/scheduler 为同一应用的进程角色 | 双库 HTTP 后 worker 消费索引；PG/MySQL 的数据库与 Redis backend 各1项真实故障→退避→恢复、终态记录→公开重试及 tenant 分区通过；Scheduler 真实数据库时钟→持久队列→worker 双库各1项通过，同进程双 App 不代表多进程调度集群 |
| 事件与通知 | Article 写入事务内经 `Events` 派发，监听器同事务记录文章事件并排索引任务；创建经 `Notifier` 将确认邮件排入 notifications 队列 | EventsServiceProvider 由模块清单安装，根注册 NotificationsServiceProvider | 双库真实 HTTP 后显式 worker 消费各3封作者确认邮件并核对捕获内容；无 SMTP 实发，事务失败不入队 |
| 缓存、tag/SWR、并发加载 | `ArticleIndexJob` 与授权后 `cached-title` 路由使用同一 named store，MISS 主库重建；模块默认 Prefix driver，可显式选择 Redis | 缓存不是授权来源，旧标题最多按单次写入 TTL 缓存且队列延迟另计 | PostgreSQL/MySQL 各 4 项业务回归含真实 Redis 双 App 共享、owner 拒绝、tenant 分区与失效；另默认 Redis tag/SWR 业务双库各6项通过；跨进程拓扑不由此验收 |
| 本地/S3 存储与流式上传下载 | `ArticleAttachments` 按 owner+文章固定 key 调用公开 `Storage` 流式上传/下载/删除；主库授权、UTF-8/大小限制与安全响应头 | `storage-local`、`auth`、数据库能力；S3 需额外 `storage-s3` 与显式默认 disk 配置 | 本地双库测试及真实 MinIO+PG/MySQL S3 回归通过，含真实 CopyObject 成功响应被暂停、并发 DELETE 等待及最终 key/staging 清理；版本化 S3 multipart/取消/确切版本另1项。跨 DB/对象存储不原子；进程强杀或远端永久无响应仍不保证自动收敛 |
| 服务端视图 | `http/controllers/pages.rs::about_page` 注入 `Views` | `view`；模块 `example::` 命名空间及发布优先/源回退 | 生产 TestApp 与真实 TCP 各自覆盖；真实 TCP 2项通过，含发布副本优先/缺失回退及转义/错误边界 |
| i18n 与地区格式化 | heartbeat-label 从 `RequestLocale` 读取请求语言，翻译模块消息并调用精确十进制格式化；欢迎邮件用 `LocalizedEmail` 和明确语言 key | 根 I18nServiceProvider 与模块 lang 目录 | 生产 TestApp 验证 en/zh-CN，真实 TCP 捕获邮件验证两种语言；复数与固定偏移长日期业务真实 TCP 2项通过；不包含 IANA/DST 时区 |
| 加密与签名链接 | 文章短期分享调用 `SignedLinks::issue/verify/revoke`；私密备注调用 `Encryptor` 以 owner+文章上下文加密入库、授权解密 | `signing`/`encryption` feature、根密钥及数据库 | 双库真实 HTTP 覆盖签发/撤销、越权备注拒绝、明文不落库及密文跨文章交换失效；生产密钥运维另行负责 |
| 审计、租户、权限边界 | Article 详情成功后经 `Audit` 落 actor/object/outcome；`QueryRule` 与写 SQL 同时限制 owner | 通用数据库配置、模块业务迁移，公开 API | 双库审计与租户各3项通过；真实 Redis+MinIO 租户跨 App/worker/附件快照清理双库各1项通过，非多进程证明 |
| HTTP 出站与上下文 | 文章摘要服务经公开 `HttpClient` 请求运营固定目标；按 owner 授权且限制连接/总时间与正文大小 | `http-client` feature 与显式 `example.article_summary_origin` | 双库真实 HTTP+本地上游覆盖成功、越权、503 和超大响应；另真实上游 TCP 头/体超时及取消 EOF 双库定向通过；取消的 RP 入口为 TestApp oneshot，metadata allowlist 不据此完整验收 |
| WebSocket/SSE 与跨节点 | 文章 owner 保护的 SSE/只读 WebSocket 订阅与定向发布使用公开 `Realtime`，JWT 路径按 token exp、Session 路径按持久截止结束 | `realtime` 与数据库；精确 Origin allowlist，Redis 跨实例需 `realtime-redis` 与显式配置 | PostgreSQL/MySQL 各3项真实 TCP WebSocket 回归含帧拒绝、expiry/停机与双 App Redis；Session WS 协议双库各3项、Chromium 153 原生浏览器双库各1项通过；不外推到 Safari/Firefox，两进程拓扑不在这些结果内 |
| gRPC 与 HTTP/gRPC 组合 | `grpc.rs` 将生成的 Echo 服务注册到同一 `GrpcServiceRegistry`，从应用容器读取 `ExampleMessage` 并限制输入长度 | 模块启用且编译 `grpc` 时贡献 `GrpcServiceProvider`；`proto/generate.py` 显式生成类型 | 真实 loopback Echo/坏输入与 TLS/同 App 双端口组合2项已通过；认证 RPC、同端口与完整部署拓扑不由此验收 |
| CSV/XLSX 与任务式数据交换 | `ArticleTransfer` 复用 `ArticleWriter` 同事务批次导入、按 owner/字段白名单导出；异步导入/导出经现有任务 API、私有 Storage 与持久收据 | `data-exchange`；异步另需 `data-exchange-tasks`、`storage-local`、数据库队列和共享锁迁移 | PG/MySQL 各10项定向与真实 TCP 上传→worker→owner 报告通过；24小时恢复窗口/永久收据及跨系统失败边界见上文，供应商存储未凭此验收 |
| 校验、分页与公开资源宏 | Article 输入用 `Validated`，列表用 `Paginated`，约定式路由用 `ResourceActions` | 公开 Validate/分页/资源接口，复用业务 CRUD | 双库真实 HTTP 已覆盖坏输入、页数界限与 `meta`，其它资源形式按实际业务再补 |
| 测试工具、替换与公开扩展 | `tests/registration.rs` 使用 TestApp、生产 Server、业务依赖替换与模块移除；认证、附件、导入导出等模块测试使用 TestDatabase 和显式捕获 driver | 模块测试与通用根验收挂载、隔离数据库；TestApp 请求为生产 Router 的进程内调用 | 当前 TestApp/TestResponse 用法与模块测试入口已展示；Laravel Factory/model factory states 属于 R6 待实现范围，不以 SeaORM Entity 替代 |
| 观测、就绪与故障行为 | 模块启用文章数据库时登记 `example.articles` 就绪探针；Article 写事务以固定操作/结果标签记录 span、计数与耗时 | 根 probes/metrics/otel 按现有配置启用 | 双库删表就绪与业务 HTTP metrics 已验证；真实 Collector PostgreSQL/MySQL 出口及不可达路径各 1/1 通过；无 OTLP metrics/业务 span 入站 W3C parent 结论 |

此表随当前源码更新，未列入的公开能力也须对照根 `src/lib.rs`、Cargo features 与项目 core 事实继续盘点。尚未实现的 Laravel 能力留在框架计划；每个后继功能簇同步实际框架 API、综合业务代码、通用根挂载、模块专属资源与测试，不能把目录迁移当作全框架等价。

## 验证与扩展

根框架测试使用 `cargo test -p openrust`，完整工具门禁使用 `cargo test --workspace`。认证 target 为 example_auth_flows，路径为模块 tests/auth_flows.rs；需要对应 feature 和有建库权限的隔离测试 URL，没有 URL 时跳过不算真实数据库通过。实际命令与结果统一记录于 [测试基线](../../.agents/project/memory/testing.md)。

编写业务时使用公开 `Routes`、`Inject`、`AuthFlows`、`Queue` 等入口；当前可参考 `JobsServiceProvider` 的 Provider 注册、`PrefixCacheServiceProvider` 的 driver 扩展与 `WelcomeService` 的标准通信调用。进程内 named 缓存不是跨节点共享缓存；捕获通信仅通过测试显式安装并拒绝 production。

自定义 driver 应实现框架已公开的 trait，通过应用 ServiceProvider 装配，不能修改框架核心来识别某个业务模块。其他 [公开 consumer](../../tests/consumers/README.md) 可辅助理解边界，但不能替代本模块的覆盖交付。

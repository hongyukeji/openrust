# 综合业务示例

`ExampleServiceProvider::boot` 通过公开 `BootContext::exceptions` 为示例业务异常登记类型化报告、409 渲染及最终响应头；`GET /api/example/exception` 经真实模块路由和异常链展示。`/api` 错误 envelope 策略由根应用 `AppServiceProvider` 显式登记；模块只贡献业务异常扩展，保留宿主的全局响应策略。此接线已通过本批限定 workspace、真实 MySQL 与公开 HTTP 验收；完整异常与 HTTP 能力继续逐项交付。

本模块的 `config/mod.rs` 在 Provider boot 时通过 `BootContext::code_config` 接入应用同一个配置仓储；`EXAMPLE_GREETING` 和 `EXAMPLE_FEATURE_ENABLED` 从启动时同次 dotenv 快照求值。`example.runtime_policy` 是进程本地 typed 配置工厂，仅在显式调用 `ExampleServiceProvider::with_runtime_policy()` 后登记；Provider 从同一仓储读取并挂载 `/api/example/config-policy`。默认模块仅登记可序列化区段，继续支持 `config:cache`；开启 typed 演示后，公开缓存契约会拒绝缓存读写。旧 TOML overlay 仅保留为显式选择来源的回归 fixture；修改 Rust 配置须重新编译，运行中的进程不会热加载。

> 当前源码位于 `modules/example/`，通过统一模块清单与 ServiceProvider 接入；隔离的 example_* 表 CRUD、当前已实现公开能力的业务调用与适用双库验收已由 R1-E 提交 `bc3fa01`。

`modules/example` 是根应用挂载的普通业务模块，供开发者学习如何在 `modules/<业务>/` 调用框架公开 API，以及通过公开 Provider/driver 契约扩展框架。框架、应用宿主和业务模块都在本仓 `main` 开发；这里没有独立 package、bin、启动模式或 example feature，也不复制完整应用骨架。

`ServiceProvider::register` 返回 `BoxFuture`，依赖绑定使用 `RegisterContext` 的异步 `instance`、`singleton` 等方法并在登记位置 `await?`。应用启动后可用 `App::register_provider` 登记新 Provider；相同类型普通登记保留首次实例，显式 `force_register_provider` 则重新执行登记并更新后续解析的服务。已持有的 `Arc` 保留原实例。现有模块回归以 `ExampleMessage` 和 `GET /api/example` 验证同一应用中的更新；已构建的 HTTP Router 是快照，业务应在构建 Router 前完成需要反映在路由表中的登记。

配置仓储的原始读写与严格类型读取语义见[配置说明](../../src/config/README.md)。模块自有的 [`config/mod.rs`](config/mod.rs) 通过统一清单写入 `example.feature`，`GET /api/example/config-feature` 通过公开配置仓储读取其固定 `enabled` 字段；模块未启用时该路由不存在。TOML 递归发现仅由显式附加的旧来源处理，不参与本模块默认引导。`GET /api/example/config-greeting` 由模块 Provider 在 boot 捕获共享 `ConfigRepository`，再经[业务服务](services/config_reading.rs)固定读取 `example.greeting`。`POST /api/example/config-greeting/review` 由同一服务将此业务键写为固定的“示例问候语已复核”，随后 GET 可读到新值；请求参数和正文均不能选择键或值。处理器只返回问候语，不输出 `all()` 或其他配置区段。写入仅作用于当前应用内存仓储，不保存到文件、数据库或其他实例，也不会自动重建已解析的类型化业务服务。[模块回归](tests/registration.rs)通过公开 `ConfigSources` 输入非字母顺序的 `example` 业务键，覆盖模块文件覆盖、运行期写入后位置保持、跨请求可见性、第二应用隔离和已解析服务的启动快照。生成器目前只建立空模块目录，不复制本模块；生成应用可显式安装独立 consumer 的 Provider fixture。

**综合示例已贯通当前已实现的公开框架能力。** 文章 HTTP CRUD、事务事件、审计、缓存、存储、账户安全及协议调用均有业务代码和分组验证；tenant 路由通过 `RouteGroup` 演示共享路径前缀与组级认证/授权 middleware。这不表示全部 Laravel 能力已完成。

启用 `queue` 时，模块始终通过同容器 `QueueContributions` 登记普通 heartbeat 任务，`POST /api/example/jobs/heartbeat` 经公开 `Queue` 派发；登记不要求默认数据库。标准构建包含数据库与 Redis 队列驱动，`QUEUE_CONNECTION=redis` 选择 Redis，`cargo run -- queue work redis --queue example` 执行该队列。Article 索引与数据库调度仍只在配置默认数据库时贡献；Redis worker 不因目录存在调度而被拒绝，实际事务型调度对 Redis 的限制在调度执行时报告。真实 Redis 无默认数据库的 HTTP→派发→handler→确认回归位于 `tests/database_selection_flows.rs`，必须显式提供服务运行，默认 ignored 不代表已验收。

`GET /api/example/route-url` 展示命名路由：`GET /api/example` 注册为 `example.index`，处理器从请求提取 `RouteUrlGenerator` 并生成该名称对应的绝对 URL。

`POST /api/example/message-bag` 是不依赖数据库或 Session 的业务错误聚合示例。请求可传 `{"title":"","email":"bad","record_id":0}`；[`controllers/example.rs`](http/controllers/example.rs) 自行判断创建表单字段和更新记录编号，再用公开 `MessageBag` 收集错误。重复调用 `add` 不增加同键同文案，`merge` 保留导入的重复消息；`ViewErrorBag` 将创建错误放在 `default`、更新错误放在 `update`，读取不存在的包只得到临时空包。响应展示键顺序、`first`、`all`、`unique`、格式化和 `to_json`；缺失键的 `first` 返回空字符串，格式参数 `"0"` 沿用当前格式，空键选择在非空包上为真、空包上为假。[模块 HTTP 回归](tests/message_bag_flows.rs)直接请求这条正式路由。开发者在自己的 `modules/<模块名>/http/controllers/` 编写业务判断，在该模块 `routes/api.rs` 挂载处理器，只需调用 `openrust::support` 的公开接口；此例是错误消息聚合参考，不替代真实 Validator、FormRequest 或自动重定向。

`GET /api/example/representation` 使用公开 `RequestInfo` 按 Laravel Request 的内容协商方式调用 `prefers(&["html", "json"])`，将同一个 `ExampleMessage` 服务消息返回为转义后的 HTML 或统一封装 JSON，并发送 `Vary: Accept`。缺少或传入空 `Accept` 时应用明确选择 HTML；存在但无法匹配两种格式时返回 `not_acceptable` 错误。HTTP 状态遵循应用配置：`Semantic` 为 406，`AlwaysOk` 为 200 并在统一响应中保留错误码。`POST /api/example/request-body` 将只读 `RequestInfo` 放在 `Json<T>` 前提取，返回方法、路径、JSON 类型判断与输入标题，证明前者不读取或占用请求正文；正文仍由唯一的 `Json<T>` 提取器读取。这里仅演示 C04-A 的请求检查和协商，不代表完整 Laravel Request、表单、Session 或响应能力已经完成。

`GET http://acme.example.test/api/example/domain/orders/42` 展示 Laravel 式 domain group：在 `RouteGroup::domain("{account}.example.test")` 声明域参数，handler 用 `DomainParams<T>` 与 `Path<T>` 分别读取域和路径参数，再通过具名路由生成 URL。域名仅用于路由分派，不作为租户认证或授权依据；测试环境需用 `Host: acme.example.test` 请求。

`GET /api/example/middleware` 展示模块 Provider 如何通过 `BootContext::middleware_alias` 登记自有 Tower middleware、通过 `middleware_group` 组合，再由模块路由选择应用；集成测试验证只有该路由返回 `x-example-middleware: named-group`。

`GET /example/pages/about` 是可选页面路由；`/example/pages/*` 未命中页面时由同一 `RouteGroup` 的 `fallback` 返回页面 404，并继承 `example.response` middleware。其他路径仍使用框架 fallback；若 public 目录中存在对应静态资源，GET/HEAD 会先返回文件。应用也可以在生成项目 `routes/web.rs` 中直接参考 `Routes::fallback` 的写法。

模块扩展目标参考用户指定的第三方 [nWidart/laravel-modules](https://github.com/nWidart/laravel-modules)；当前依据为 [v13 文档](https://laravelmodules.com/docs/13/getting-started/introduction)，固定模块源码为 59714840708a2aece19e810764473f53227c35fe，当前只按逐项证据交付子范围。借鉴自有路由/API、controller/model/resource、迁移、测试与 provider 装配；它不是 Laravel 核心，不扩张 Laravel 主基线，也不表示本框架已全量实现该包。按本项目约定采用 modules/<模块>、snake_case 和编译期静态注册。修改 modules/mod.rs 的编译声明和 Application::modules 注册清单是正常业务接入；为某个业务修改 src/ 核心或加入全局样例特判不属于接入方式。优先复用现有 ServiceProvider/Routes；Rust 模块编译期声明，无运行时源码扫描。

## SQLite 配置与迁移接入

模块的 reporting 路由通过标准 `Databases` 使用独立 SQLite 连接；对应测试在真实 Router 调用后检查同一池的 nullable/raw PRAGMA。迁移清单通过公开 Framework/模块 MigrationServiceProvider 接入，`PrivateNote` 回滚使用 `dialect::drop_column`；旧版 SQLite 在迁移所有者内重建 main 普通表，无需业务自行分支。

这些连接与既有迁移清单已在 SQLite 3.26/3.51.3 验证；无 auth 根宿主另有真实 up/status/down/up。独立清单 Provider 验收不等于 Root+Auth 或全部业务已支持；认证、DB缓存/队列/调度与完整 Migrator/Schema 仍开放。数据库 Session 的原生默认与显式 Guarded 语义见 [Session 指南](../../src/session/README.md#原生-database-与显式增强)。需要浏览器实时授权截止或远程注销时显式选择 Guarded，并在同一表登记增强列/索引迁移；示例包装器转发同一 Session driver 工厂和请求元数据，不保存第二套会话。

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

## Cookie 偏好与响应组合

[`controllers/cookie.rs`](http/controllers/cookie.rs) 与 [`tests/cookie_flows.rs`](tests/cookie_flows.rs) 展示业务如何使用框架 Cookie 能力。路由在模块自己的 [`routes/api.rs`](routes/api.rs) 定义，不修改框架源码：

- `GET /api/example/cookie` 用 `RequestInfo::cookie` / `has_cookie` 仅读取 `example_preference`，不返回其他 Cookie 或原始头。
- `POST /api/example/cookie` 接收 JSON `{"action":"queue","value":"dark"}`，通过注入的请求级 `CookieJar` 创建并排队；队列在响应阶段写出。
- 将 `action` 改为 `direct`，通过 `CookieResponseExt::with_cookie` 组合现有 JSON 响应；改为 `forget`，按相同 `/api/example` 路径发送到期删除标记。

浏览器随后向匹配路径回送 Cookie。测试将实际 `Set-Cookie` 的首个 name/value 对原样作为后续 `Cookie` 请求头，不假定 wire 值为明文，也不在业务中解密。Cookie 不经过普通输入的 trim/null 转换；读取使用直接键，数组叶子的解释由框架 wire 解析器负责。

宿主必须登记 `CookieServiceProvider`；示例 Cookie API 路由显式使用 `cookies` 中间件，普通 API 不自动启用 Cookie。默认构造要求现有 Encryptor，提供受保护 Cookie 与历史密钥解密。显式 `plain()` 仅提供无加密路径，不保护 Cookie 的机密性或完整性，不能用于默认 Web 组。Session API、账户 Session 与 OIDC 浏览器绑定路由逐组显式叠加 `cookies`、`session`；其余 API 不默认创建会话。未经过 Web CSRF 验证的 Session Auth API 保留独立认证 token 校验。

## Session 偏好与一次性提示

`GET /api/example/session/old-input` 读取上次请求闪存的输入；`POST` 接受 `{"action":"flash","name":"Ada","nickname":null,"password":"secret"}`，示例显式调用 `Request::flash_except(&["password", "action"])`，只保留非敏感的规范化输入。`action=only` 展示 `flash_only`，`action=flush` 仅清空旧输入，不删除整个 Session。显式 opt-in 的 `POST /api/example/session/old-input/preview` 带 `Precognition: true` 时只预检输入、返回 204，不执行闪存 handler，也不保存或老化当前 Session；普通路由上的同名请求头不会自动确认预检。读取保留显式 null 与缺失的区别；当前能力未合并上传文件，也未提供 redirect 自动闪存或通用错误 bag。

[`controllers/session.rs`](http/controllers/session.rs) 在模块自己的 [`routes/api.rs`](routes/api.rs) 注册 `GET /api/example/session` 与 `POST /api/example/session`。启用 `session` 编译能力、宿主登记 `SessionServiceProvider` 且最终驱动未禁用时，模块才挂载这两条路由；handler 直接提取公开 `Session`。根宿主配置数据库，或显式选择 cookie、array 等非数据库驱动时，会装配唯一的通用 Session，无需额外启用认证。默认 database 驱动且无数据库时不挂载路由，不静默使用 memory；未知显式驱动由框架最终配置校验拒绝。与 `RequestInfo::session()` 取得的句柄共享同一请求状态，无需业务再建 store 或修改框架源码。存储与 Cookie 配置由宿主统一管理，默认数据库驱动；测试显式选择 memory，不代表数据库不可用时自动回退。装配、迁移及自定义 driver 见 [Session 使用说明](../../src/session/README.md)。

POST 接收如 `{"action":"put","value":"blue"}` 的 JSON。示例仅操作 `example.session.preference`、`example.session.notice`、`example.session.memo`、`example.session.list`、`example.session.count`，不允许正文指定任意 Session 键，也不返回会话 ID、token 或认证属性：

- `put` / `forget` 保存或删除偏好；GET 查看当前示例字段。
- `flash` 创建下一请求可读的一次性提示；`now` 仅供当前请求使用。提示在保存时老化。
- `keep` 仅延长示例自己的提示，不调用可能影响其他业务提示的全局 `reflash`。
- `memo_null` 把示例 memo 置为 null，`remember` 对缺失或 null 计算固定的 `computed`，对已有非 null 值保留原值；`memo_seed` 接受有界字符串以展示命中。
- `push` 将有界字符串追加到示例列表；示例业务最多保留 8 项。`increment` / `decrement` 使用默认步长，`increment_by` / `decrement_by` 使用示例固定步长 2；这些限制只是业务输入范围，不是框架 Store 默认值。
- `remove` 返回本次移除的 memo 值，之后的 GET 仅显示固定业务字段中的 memo 已缺失。

正文先经过框架输入规范化，再交给业务校验；这里要求值为不超过256 UTF-8字节的字符串，因此全空白转换为 null 后会被拒绝。这不表示 Session 的 `put` 自动 trim 或将空字符串转 null。响应保存和 Cookie 往返由框架处理，客户端应保留实际 Cookie，不能假定其 wire 值为明文。

[`tests/session_flows.rs`](tests/session_flows.rs) 展示正式模块路由的偏好、flash 与 Store 便捷方法生命周期，分别显式选择 memory 和 Cookie 驱动；`tests/auth_flows.rs` 在配置数据库的根宿主通过真实 TCP 验证数据库 Session 的跨请求保存。它们不替代集群验收。

`extensions` 是受信任服务端选择的扩展示例：只重申当前合法 ID、生成随机 ID，并把实际 handler 赋回自身验证同绑定不变；正文不能指定任意 ID 或后端。返回值仅含身份是否改变、同 Arc 与请求依赖的布尔结果，不包含凭据。`probe` 只接受 `one`/`two`，通过现 Cookie 队列写入示例探针，不属于认证数据。

[`providers/session.rs`](providers/session.rs) 给出调用方显式安装的请求驱动与 Provider。默认模块不会安装它或覆盖宿主已有 store；示例 fixture 明确选择 memory 或当前公开 Db，委托底层原子 load/write/delete，不另建 Session 属性。hook 在每请求实例中读取解密 Cookie 快照并排回执 Cookie；两组 Cookie 的交错请求验证不串 scope。真实 TCP fixture 另启同根 Application、显式选择同数据库连接的请求驱动，主认证 App 保持默认数据库 driver。包装器将存储保证、认证支持能力及请求绑定委托给实际底层驱动；不能只委托读写而把客户端快照误报为服务器存储。此方案不具备通用 Container factory 装饰能力，不冒称可以保留并装饰任意既有 factory。

`POST /api/example/session` 的 `put_value` 操作用固定业务偏好键演示 `DataValue::from_json`、原始 `Session::put/get` 与显式 `to_json`。`object_native` 在同一请求用公开 `DataObject` 检查对象句柄身份和默认保存拒绝；`object_json` 为同一固定业务对象选择显式 JSON 持久投影，并由下一请求读取投影值。请求正文不能传入对象或自选 Session 键，响应只含受限业务状态与布尔验证结果。

### 请求级过期回收

Session 在成功 start 后、业务 handler 前，按最终 `session.lottery` 抽签调用实际请求驱动的 `gc(lifetime秒)`；默认 `[2,100]`，`[0,1]` 不触发，`[1,1]` 必触发。数据库和 memory 回收持久期限已到的记录；Cookie 与 Redis 保持 no-op/TTL 职责。显式 expiry 是授权期限上界，改变请求 lifetime 不会据此误删仍有效记录；这与 Laravel 按 last_activity 和当前 lifetime 回溯的方式有公开差异。错误会阻止业务执行，取消不启动后台补偿，已经完成的删除不回滚。

示例请求包装器转发真实底层 `gc`，仅显式安装它的 fixture 排出 `example_session_gc` 回执，内容是调用秒数和真实返回删除数，不包含 ID、属性或凭据；默认宿主不安装该包装器。模块的双客户端非空往返 fixture 使用全中 lottery 断言 GC 回执与业务隔离；实际数据库删除计数和竞争另由真实服务专项验证，不把回执当作数据库清扫证据。生成应用同样转发 GC 并使用最终配置，而不新增另一套清扫 driver。

### 不使用数据库的 Cookie Session

在宿主 `config/session.rs` 或环境变量显式选择下列驱动即可复用同一业务控制器，不修改模块或框架源码：

```toml
[session]
driver = "cookie"
lifetime = 120
expire_on_close = false
```

正式宿主与生成应用按配置装配 Session Provider。客户端须保留完整双 Cookie：一枚保存 ID，另一枚以 ID 为名称保存数据。不能只取配置名 Cookie 再请求；也不要在业务中自行解密或构造属性载荷。框架处理 Cookie 保护、历史密钥、请求内保存和响应队列。

此模式没有服务器 Session 记录，旧 Cookie 副本在嵌入期限前仍可重放；当前浏览器登出不等于全局撤销。认证必须启用加密并使用受保护 Cookie Provider；plain 或 Session Cookie 被排除保护时，认证入口会安全拒绝，普通匿名偏好业务仍可使用。需要服务器撤销保证的长连接授权和远程会话删除不能用它冒充数据库驱动。

[`tests/cookie_auth_flows.rs`](tests/cookie_auth_flows.rs) 参考现账户 Session 登录、受保护偏好读取/更新与登出，并包含 plain、ID 排除及数据 ID 排除的伪造拒绝场景；用户账户仍存真实数据库，Cookie driver 仅替代 Session 存储。它没有把 JWT 专用的 `/me` 路由改为 Cookie 认证，也不以测试准备服务替代 HTTP 登录。

[`tests/browser_cookie_session.py`](tests/browser_cookie_session.py) 可用已有 Playwright/Chromium 复跑浏览器自动保存、回送双 Cookie、双上下文隔离及 HttpOnly 边界。先启动显式 Cookie driver、已配置加密的宿主，再把下例 URL 与 Cookie 名改为实际配置；结果使用新的 `/tmp` 专用目录，旧结果不会覆盖：

```bash
python3 -B modules/example/tests/browser_cookie_session.py \
  --url http://localhost:3000/api/example/session \
  --cookie-name your_session_cookie \
  --result-directory /tmp/session-cookie-browser-check
```

脚本只接受本机 loopback HTTP，不安装浏览器、不使用用户 profile 或修改证书信任；其结果不代表浏览器 TLS 验收。业务字段保持小于浏览器实际 Cookie 容量，较大 Session 应选择服务器后端。Cookie/Session 机制自身的错误响应禁止缓存，不携带被替换的业务正文或 Cookie。

## 从真实调用链开始

根 [`src/main.rs`](../../src/main.rs) 运行同一 `bootstrap::Server`；`Application::modules` 调用模块 [`manifest`](mod.rs)，将 [`module.json`](module.json) 的 `ExampleModuleServiceProvider` 绑定到已编译实现。包 register 实际发现、选择并注册父/子 Provider，随后执行已编译 [`module_init.rs`](module_init.rs)，不是手调生命周期。业务 boot 从同一 ModuleRepository 读取实际清单目录；metadata.module_directory 仅覆盖文件包含路径，发布目录仍基于 application_root。状态文件路径独立配置；禁用只影响新执行，宿主 `/api` 保留。`GET /api/example/module-initialization` 只读返回初始化及业务注册观察值，沿真实 Inject 读取同一 Container。

`GET /api/example/module-state` 经公开 `Inject<ModuleRepository>` 与共享日志服务返回 `data` 中的 `name`、`enabled`、`loaded`、`events`。这是只读业务接口，持久写只通过核心命令或公开模块管理 API。`ModuleStateServiceProvider` 与父 Provider 的同一个 `EventsServiceProvider` 共享日志；命名监听 `modules.example.*` 追加生命周期事件，适用时再向该 Provider 追加原文章事务/队列监听，不另建事件 bus。

```bash
cargo run -- module:list
cargo run -- module:disable example --no-interaction
cargo run -- module:enable example --no-interaction
```

默认 file 在构造时读取并缓存，独立 CLI 命令改盘不会改变既有 App 的 enabled 视图；新 App 读取新状态。事件日志仅记录本 App 内的派发，不接收其他进程的事件广播。一个已加载 App 内执行 `app.run_command("module:disable", vec!["example".to_owned()]).await?` 后，状态端点仍可返回 `enabled=false, loaded=true`；新 App 不再挂示例路由，但宿主 `/api` 继续可用。再次启用不热挂先前未加载的 App，后续新 App 才恢复示例。需要实时查看外部状态时显式选择 MODULES_ACTIVATOR=file-live；编译代码直接使用 FileModuleActivator::new(path)?.with_live_reads()，仍不热挂卸已加载资源。对应[三实例回归](tests/module_activation_flows.rs)使用临时应用根、独立状态文件和真实 `bootstrap::Server`，源码用于验证上述流程，不在真实根状态文件上操作。

运行 catalog 来自 module.json，执行仍需显式编译绑定；class aliases、完整deferred加载与TTY搜索选择仍待实现。文件的 hardlink 等价及 Windows 原生行为尚未验收，启停与并发 `shutdown` 不提供原子提交屏障，详见[模块配置边界](../../src/config/README.md#模块激活与运行根)。

[`ExampleServiceProvider::register`](providers/example.rs) 登记 `example` 类型化配置/schema 和业务依赖工厂；其 `boot` 调用 `BootContext::code_config` 求值模块 [`config/mod.rs`](config/mod.rs)，按显式选项读取同仓储 typed 策略，再贡献 [`routes::register`](routes/mod.rs)。后续业务工厂读取共享配置仓储，`greeting` 用于响应文字。模块配置在 Provider 的 boot 时点接入；显式 TOML overlay 的旧回归 fixture 保留在 `tests/fixtures/config/`。

模块 `GET /api/example` 由 `show` 返回 `Json<ExampleInfo>`，其中 `message` 来自 Provider 登记并经 `Inject<ExampleMessage>` 注入的业务依赖，`served_at` 直接调用第三方 `chrono::Utc::now()`；类型通过 serde 序列化，经过框架 HTTP 处理链。
从仓库根启动，在另一终端请求默认端口：

```bash
cargo run
```

```bash
curl --noproxy '*' http://127.0.0.1:3000/api/example
curl --noproxy '*' http://127.0.0.1:3000/health
```

默认基础示例不要求数据库或 Redis；端口与公开目录见根 [`config/server.rs`](../../config/server.rs)。文章及欢迎路由仅在配置数据库并编译认证能力后出现；无数据库启动不表示文章业务可用。

## 隔离的字段类型示例

`/api/example/records` 提供独立的列表、详情、创建、更新、删除接口，所有查询按当前登录用户隔离。它只操作 `example_records` 与 `example_record_categories`，不使用通用 `records`、`categories` 或开发者业务表。记录字段覆盖本仓生成器当前支持的可移植类型：`name`（string）、`description`（text）、`priority`（integer/i32）、`view_count`（big_integer/i64）、`enabled`（boolean）、`published_at`（nullable date_time/UTC）、`metadata`（nullable JSON）及 `category_id`（nullable references）。外键目标 `example_record_categories` 也属于示例命名空间。

示例 API 接受完整 JSON：`{"name":"样例","description":"说明","priority":1,"view_count":0,"enabled":true,"published_at":null,"metadata":{"source":"demo"},"category_id":null}`。该接口用于查看类型映射与 CRUD 写法；不会自动种入类别数据。综合示例的 API 路由统一置于 `/api/example/*`，网页置于 `/example/pages/*`，示例对象存储 key 统一以 `example/` 开头，避免覆盖应用自己的路由与对象路径。

## 文章与欢迎业务

数据库启用且编译 `auth` 时，`/api/example/articles` 使用公开 `ResourceActions` 注册列表、详情、创建、更新和删除。处理器用 `Validated` 校验标题与正文，页码限定 1–1,000,000、每页 1–100；列表通过公开 `Paginated` 返回 `data` 和 `meta`。`CurrentUser` 与 `QueryRule` 将读取限制为本人文章，更新和删除 SQL 同时筛选文章 ID 与 owner ID。缺失或他人文章均返回 `not_found`，写入在主库事务中完成。

文章创建、更新和删除在同一事务中调用 `Events::dispatch_in_transaction`，模块监听器写入 `example_article_events`，并把幂等的 `ArticleIndexJob` 插入同一持久队列事务；监听或入队失败会回滚文章写入。创建还从同一事务读取作者已验证邮箱，用公开 `Notifier::dispatch_in_transaction` 排入仅发给作者的确认邮件；入队失败同样回滚文章，测试 worker 使用捕获邮件驱动，不向真实供应商发送。显式 worker 在提交后按 owner/文章 ID 维护 `example-prefix` 命名 store 中 TTL 600 秒的派生标题，删除时移除对应键。`GET /api/example/articles/{id}/cached-title` 每次先在主库检查 owner，缓存 MISS 才从主库重建；默认 store 不受模块设置影响，应用可显式把该 named store 设为 Redis。标题是 eventual 派生值：异步 job 未消费或旧快照与新 job 交错时可能读到旧标题，单个写入的 TTL 为 600 秒，队列长期故障时不承诺端到端 600 秒内最新；授权与普通 CRUD 始终由主库决定。详情成功读取经公开 `Audit` 记录 actor、文章与结果，越权读取不会产生成功审计。

文章 HTTP 创建、更新和删除的完整事务结果使用既有 tracing span 记录固定操作与结果；启用 `metrics` 时产生 `example_article_operations_total` 与 `example_article_operation_duration_seconds`。标签仅有 `operation`、`outcome`，不包含账户、文章 ID、邮件地址、正文或错误内容。取消记为未完成的 `cancelled`，不推断数据库回滚；批量导入事务内的单次 `create_in_transaction` 不提前记作已提交。`/metrics` 仍须按框架配置显式开放并限制采集网络。

编译 `encryption` 时，所有者可通过 `PUT/GET /api/example/articles/{id}/private-note` 保存和读取私密备注。主库只保存 `Encryptor` 密文；认证上下文固定绑定 owner ID 与文章 ID，列表和公开文章资源均不包含备注或密文。私密备注输入最多 10,000 字符，越权与不存在返回 404；响应明确设置 `Cache-Control: private, no-store` 与 `nosniff`。双库真实 HTTP 回归还验证密文不能在另一文章上下文解密。密钥需由应用安全配置，测试密钥不适用于生产。

编译 `storage-local` 时，所有者可通过 `PUT/GET/DELETE /api/example/articles/{id}/attachment` 上传、下载和删除一份 UTF-8 文本附件。所有者还可通过 `PUT /api/example/articles/{id}/attachment/visibility` 与 JSON `{"visibility":"public"}` 或 `{"visibility":"private"}` 修改当前租户附件的实际可见性；同一文章的非所有者收到 404，存储权限失败按框架错误返回。服务端构造 `example/` 前缀下 owner/文章专属对象 key，拒绝客户端文件名和其他 MIME；流大小 1 MiB、分块 16 KiB，逐块校验 UTF-8 与控制字符。下载强制固定文件名、`nosniff` 与 `private, no-store`。上传与文章删除使用同一文章行锁串行化；文章删除在数据库事务中写入持久清理意图，提交后才删除对象。上传在 App 资源所有者中受管运行并保留启动时的租户作用域；15 秒 HTTP 等待截止后发布任务继续收尾，文章删除需等该任务完成。45 秒期限约束请求体读取至 EOF；EOF 与期限由共享原子状态仲裁：期限先获胜时，流不能以正常 EOF 交给 storage，因此不会进入对象发布；EOF 先获胜时，发布任务继续持有数据库锁直到存储调用收敛，不因 HTTP 等待超时释放事务。真实 MinIO 代理暂停签名 CopyObject、确认其已向 MinIO 返回成功状态后，PostgreSQL 18 与 MySQL 8.4 各自的竞态回归验证 DELETE 等待复制完成，并在删除后确认最终 key 不存在且 staging 已清理。请求体挂起的 deadline/锁释放回归在 PostgreSQL/MySQL 也各通过 1/1。App shutdown 到达自身 deadline 后仍可能中止任务；进程被强杀、MinIO 永久无响应、网络请求永不收敛，或 CopyObject 已发送后返回 `storage_publication_uncertain` 且远端稍后完成时，不保证数据库与对象存储原子提交或自动消除孤儿。已提交文章删除的清理意图可由 `example attachments reconcile` 幂等重试；重用文章 ID 时，遗留意图会阻止新附件上传并拒绝清理，避免误删新对象。此机制不等于病毒或敏感信息扫描。本地附件回归使用生产 Router 的 `TestApp` 与真实 PostgreSQL/MySQL；S3 回归另外使用隔离 MinIO bucket，包含两独立 App 的真实 TCP 分块上传、下载与越权拒绝、挂起新对象不可见、挂起覆盖保留旧对象、复制在途时删除等待并清理、取消及持久清理意图恢复。S3 路径目前仍需同时编译 `storage-local` 与 `storage-s3`，并显式把默认 disk 配成 S3；发布与运维边界见 [存储说明](../../src/storage/README.md)。

编译 `data-exchange` 时，认证用户可通过 `POST /api/example/articles/import?format=csv|xlsx` 导入、`GET /api/example/articles/export?format=csv|xlsx` 导出自己的文章。输入最多 1 MiB、100 行，先完成有界解析，再逐行调用同一 `ArticleWriter`；合法行各自提交，报告只给行号、白名单字段和稳定错误码，前序成功行不会因后续坏行回滚。导出只含 `title`、`body`，最多 100 行、2 MiB，不含 owner、私密备注或密文；CSV 公式前缀按框架策略转义，不能承诺逐字导入往返。编译 `data-exchange-tasks` 和 `storage-local` 时，还可通过 `POST /api/example/articles/export-jobs` 排队导出，并经 `GET /api/example/articles/export-jobs/{job_id}/{attempt}` 按当前账户读取本地私有对象；框架清理任务在保留期后删除对象。标准应用迁移包含此能力所需共享锁表。

同样的 feature 组合提供 `POST /api/example/articles/import-jobs?format=csv|xlsx`，将服务端生成的 owner 专属导入来源写入私有本地存储并排队；所有者可读取 `/api/example/articles/import-jobs/{import_id}/status` 与 `/api/example/articles/import-jobs/{import_id}/{job_id}/{attempt}` 报告。每批最多两行，在同一数据库事务中复用文章写入、事件与通知规则；失败批次回滚，已提交批次保留，重试通过现有任务幂等记录避免重复提交。恢复窗口为 24 小时，超过窗口的未完成导入转为可见的 expired 终态，不把来源过期删除误报为成功；终态收据永久保存以拒绝旧 payload 重放，会持续占用数据库行，需要按业务保留政策人工规划容量。来源清理任务仅在终态或安全过期后删除对象；租约忙或未完成时有界延期。首次清理入队失败或延期耗尽时，可在迁移完成后运行 `example imports reconcile`，每次按导入 ID 升序最多检查 100 条未回收收据，输出安全计数与下一游标；下一页运行 `example imports reconcile <游标>`，直到输出 `next cursor none`。单条失败会计数并继续本页，命令返回失败；完成翻页后从首页重跑处理先前失败项。必须实际调度该维护命令。外部 worker 强制停止后租约最长需等待 5 分钟过期，24 小时后旧任务不能继续写入；存储与队列不是跨系统原子提交，报告和外部邮件交付也不宣称永久 exactly-once。

编译 `realtime` 时，当前文章所有者可经 JWT Bearer 请求 `GET /api/example/articles/{id}/events` 建立 SSE，或升级 `GET /api/example/articles/{id}/socket` 为只读 WebSocket；通过 `POST /api/example/articles/{id}/announce` 向自己在该文章频道的在线连接发布事件。两类连接要求 `[realtime].allowed_origins` 精确允许请求 Origin；缺省空列表拒绝浏览器来源。模块重新校验同一签名 token，并使用其实际 `exp` 截止连接，不自行延长凭据；文章权限在连接和发布时分别查询。`/socket` 握手采用 `Authorization: Bearer`，适用于能设置该头的原生或服务客户端。浏览器原生 `WebSocket` 使用独立的 `/browser-socket` 路由，以同请求已认证 Cookie Session 和精确 Origin 校验，不接受 Authorization 或 URL query 凭据；`SessionStoreHandle::authorized_until` 读取持久记录中的账户与原截止时间，不续期。两种入口都先查主库文章所有权；Session 登录和 CSRF 仍经账户公开入口。协议级真实 TCP 回归已覆盖 Session 路径；Chromium 153 原生浏览器的 PostgreSQL/MySQL 定向场景各1项已验证。浏览器用例与握手拒绝的精确 HTTP 状态分别验证，不能外推到所有浏览器。已建连接按握手时的截止时间结束，不承诺另一个请求登出后立即关闭已有 WebSocket。发布成功表示本机队列已接受，不能保证远端客户端收到；双库回归验证真实 TCP 升级和两个独立 App 经真实 Redis 传输，不等于两 OS 进程集群。

编译 `http-client` 且运营配置 `example.article_summary_origin` 时，所有者可通过 `GET /api/example/articles/{id}/summary` 请求固定上游的摘要。配置必须是无凭据、查询、片段和额外路径的 HTTP(S) 根地址；请求方不能提交 URL。调用先检查文章归属，再以 2 秒连接/5 秒总时限及 16 KiB 响应上限调用公开 `HttpClient`，非成功状态、无效内容和超限均返回不含上游地址的 502。双库真实 HTTP 回归使用本地 loopback 上游验证成功、越权、503 与超大响应；另有真实本地 TCP 上游的头前停滞、部分正文停滞与取消后 EOF 回归；RP 请求取消部分使用生产 `TestApp` oneshot，不能称整个取消用例为双端网络。服务没有因此获得商业上游可用性或远端业务取消保证。

`POST /api/example/welcome` 是显式 JWT 认证业务动作，按账户限流且只接受空 JSON 对象。服务从主库按当前账户 ID 读取未停用、已验证的邮箱，只向该邮箱发送；请求中的第三方邮箱或电话会被拒绝。邮件用公开 `LocalizedEmail` 与模块语言目录按请求语言生成主题/正文，真实 TCP 回归覆盖英文和简体中文。现有账户模型没有可验证归属的手机号，因此 HTTP 入口不发 SMS。仅测试编译的受控业务调用演示标准 `Mailer` 与 `SmsSender`：两个目的地发送前共同校验，两通道没有跨供应商原子事务。启动不会自动发送。[`tests/extension_flows.rs`](tests/extension_flows.rs) 用 `TestApp.configure_after_register` 安装仅测试环境可用的捕获 driver，验证内容、失败传播和生产拒绝，不向真实供应商发送。

编译 `signing` 时，所有者可通过 `POST /api/example/articles/{id}/share` 签发 5 分钟文章阅读 token，并通过 `/share/revoke` 撤销。公开 `GET /api/example/shared-articles/{token}` 先校验用途、签名、有效期和共享数据库撤销状态，再按签名内 owner 与文章 ID 读取；文章删除后旧 token 也不能再读。token 是 bearer 凭据，调用方须避免把完整 URL 写入访问日志或公开渠道。双库真实 HTTP 测试覆盖他人签发拒绝、公开读取、他人撤销拒绝与所有者撤销后拒绝；不把此链接当作账户登录。

上述 Article CRUD、审计与事务事件路径在 PostgreSQL 18 和 MySQL 8.4 的真实 HTTP 测试中通过；当前公开能力的综合示例和适用门禁已执行，Collector 双库 delivery/unavailable 均有真实服务证据。捕获证据不算 SMTP/Twilio 实发。

## 账户安全与外部身份

启用 `auth-fortify` 后，根应用默认采用官方 Web/Fortify 模式，沿具名 Session Guard、同源 Cookie/CSRF、同一账户的三字段和 `TwoFactorActions` 完成登录与二因素管理。用户自行选择绑定；本应用 `confirm=true` 时，生成密钥只是待确认，确认后后续登录才要求挑战。恢复码加密保存，成功使用后替换该码并保持八份，不维护另一套 MFA 服务或旧 `auth-mfa` 开关。

显式设置 `AUTH_PROFILE=jwt-api` 后，框架 `/api/auth` 提供密码登录、必要 TOTP/恢复码挑战、access/refresh、近期密码管理授权及二因素管理；这些流程不依赖 Session、Cookie 或 CSRF-cookie。模块的 `/api/example/me` 与 `/api/example/account/me` 使用该具名 API Guard，跳过旧账户 Session 登录与无作用域 JWT 入口。管理请求只使用受限的 `X-Fortify-Management` 授权，不能把它当资源 Bearer；遗失待确认绑定时可通过当前 JWT 和密码重开全新绑定，不能取回旧密钥或重置已确认因子。完整字段、装配与配置见[认证指南](../../docs/src/security/authentication.md#显式-jwt-api-profile)。

两种传输复用唯一 Provider、Fortify 状态和实际因子证明。受管资源入口核验当前账户、代际和凭据家族，不能用同步 JWT 签名校验推定当前授权。账户响应禁止缓存；Cookie 名由所选 Session Provider 配置决定。新认证组合的完整后端、Session 派生凭据、OIDC 关联及生成应用验收尚未闭合。通用模型保留原始 ID，但当前 JWT 持久适配的正整数 `users.id` 限制尚未消除。

配置 `example.oidc` 且编译 `auth-oidc` 后，模块才挂载 `/api/example/oidc`。Provider 在启动时完成受限目标的 discovery，并要求同容器中的 `Db`、`AuthFlows` 与 `Encryptor` 可用；模块路由登记之后再覆盖 OIDC 配置会在 Ready 前拒绝。登录回调验证浏览器绑定 cookie、state 与授权码，不按相同邮箱自动合并账户。JWT 原生客户端可显式 bind/unbind；浏览器绑定从已认证 Session 的 `/bind/authorize` 发起，以同源 CSRF 和短时意图绑定账户与浏览器，回调仅准备一次性确认，再由 `/bind/confirm` 复验密码。确认页只在 HTTP 成功且统一响应 envelope 的 `success` 为真、`data.status` 为 `bound` 时显示成功；测试显式 `AlwaysOk` 模式下，错误密码或已消费 state 虽返回 HTTP 200，仍显示失败。显式运行 `browser_confirmation_reports_success_and_failures_from_actual_page` 会启动 Chromium 执行生产 HTML/JavaScript，覆盖成功、业务失败、HTTP 错误和非法 JSON；本地 mock 只验页面判断，不替代真实 Keycloak/数据库验收。意图终态保留 tombstone，以免旧 state 在过期后绕回普通登录或重新绑定；这会持续占用数据库行，当前没有自动压缩，运维需监控表容量，不能按本地 10 分钟 TTL 直接删终态。浏览器测试使用真实 Keycloak 授权网络和随机端口的真实 RP HTTP，但测试夹具截取 IdP 固定回调地址并转交相同路径；不宣称完整浏览器自动导航已验收。

登录回调经 `ExternalIdentityFlows::complete_with_handle` 返回外部登录关联 handle；需要二因素的账户复用唯一 Fortify 挑战，完成前不能签发 token。一次性挑战完成后，refresh 家族沿既有接口关联；`/session/attach` 只允许把**已登录的同账户 Session**关联到该 handle，并重新验证密码、Origin 和 CSRF，不是 OIDC 直接创建 Session 的替代实现。`POST /logout` 先撤销本地关联 refresh/Session，再返回固定 IdP end-session URL；同 callback 的 state-only 分支消费退出 state，不循环重定向。公开 `/backchannel` 只接受框架验证通过的签名 logout token，按 issuer、audience、sid/sub 和 jti 处理撤销与重放；不能用未验证的请求字段选择账户。`tests/oidc_lifecycle_flows.rs` 与服务端 OIDC 测试保留真实 Keycloak、双库 RP TCP、关联退出、签名 backchannel/重放、wrong-audience 及伪签名负例；迁移后的当前 Fortify 调用与事务末检仍须重新执行，历史结果不作为本批通过证据。浏览器 RP 入口依赖其明确的浏览器绑定 Cookie，不属于所选 JWT API 无 Cookie 登录流程；它也不是自建 OAuth2 授权服务器或 OIDC Provider。受管 JWT/refresh 通过实际 Guard/authority 查询当前家族；只做同步签名的自包含 token 不因此获得撤销保证。本地 logout 不等于所有 IdP 会话均已退出，也不保证已建 WebSocket 即时断开。

## 租户业务：先授权，再读取派生数据

[`http/controllers/tenant.rs`](http/controllers/tenant.rs) 的路由链是 JWT `AuthLayer` → 主库 membership 校验 → 私有 `VerifiedTenant` extension → 公开 `TenantLayer` → handler。`X-Example-Tenant` 只是 selector，不能自行成为可信身份；模块没有允许用户自授权 membership 的 API。[`services/tenant.rs`](services/tenant.rs) 用 `QueryRule::for_tenant` 与 owner 条件读库，写 SQL 同时限定 tenant/owner；每次访问先查真实成员和记录，再读取 Cache/Storage。

`GET/PUT /api/example/tenant/record` 维护最小租户记录，`POST .../record/rebuild` 经持久 Queue 派发投影，`GET/PUT .../record/file` 演示对象存储。Cache/Storage 复用框架 `TenantContext` 分区，worker 从任务 metadata 恢复上下文并重新核对 membership、记录和 payload，不另造租户队列。相同逻辑 key 在两个 tenant 中物理隔离；同一 owner 也不能跨 tenant 读写。真实 worker 测试覆盖缺 metadata、篡改为无权限 tenant、入队后撤销 membership，验证无未授权投影；这不声称能防御拥有数据库完整写权限者任意伪造授权，也不宣称撤销能回滚已开始的 Storage IO。任务仍遵循 at-least-once，副作用须按业务幂等处理。

附件清理意图保存上传时的 tenant 快照；在另一 tenant 的 ambient context 执行会拒绝并保留意图，无 tenant 的维护入口按原快照恢复分区清理。[`tests/tenant_s3_flows.rs`](tests/tenant_s3_flows.rs) 使用真实 Redis、MinIO、双 App 与隔离数据库，检查同 tenant 跨 App 一致、不同 tenant 同逻辑 key 隔离、真实 worker 投影及 A 附件清理不删 B 对象；这仍是同进程实例测试，不是 R14 多进程集群验收。

## 其它公开能力的业务入口

- **默认 Redis 的 tag/SWR**：`GET /api/example/articles/{id}/cached-title/swr` 在主库 owner 授权后使用 `Cache::default_store`、tag 和 `get_stale_while_revalidate`；新鲜期 30 秒、陈旧期 120 秒，文章索引 job 在提交后消费并失效对应 tag。具名 `redis` store 是框架缺省缓存选择，`CACHE_STORE` 映射到 `cache.default`；显式默认 Redis 的双库回归已覆盖该业务，不静默回退到 memory。它与 `example-prefix` named driver 是两个示例；两者都不是授权来源，队列未消费时允许旧派生值。
- **本地化预览**：`GET /api/example/reading-summary` 的 [`http/controllers/localization.rs`](http/controllers/localization.rs) 通过 `RequestLocale`、模块消息的 CLDR 复数及 `I18n::format_date/format_datetime` 返回阅读摘要预览；输入为受限 count、RFC3339 at 和 `utc`/`utc+08:00` 固定偏移，不伪装为真实阅读统计或 IANA/DST 时区实现。真实 TCP 测试覆盖英中差异、复数、长日期与跨日边界。
- **视图发布**：`GET /example/pages/about` 从当前 `RequestInfo` 调用公开的 `Views::render_for_request("example::pages/about.html", &info, ...)`。模板按请求读取默认 `errors.first("title")`、`errors.any()` 和 `other` 命名包的 `first()`/`all()`；无 Session 时为空，Session 中自定义 `DraftErrors` 恢复后仍保留业务首条消息。生产模块优先使用 `resources/views/vendor/example` 发布副本，缺失时回退模块模板；[`tests/view_flows.rs`](tests/view_flows.rs) 的既有真实 TCP 验证覆盖源模板、发布覆盖、回退、转义和错误边界。页面从当前请求共享包读取默认、命名和自定义消息，后续请求按原有 flash 规则老化。
- **显式种子与重试**：`database/seeders` 实现公开 Seeder，由 `db seed` 或测试显式调用，boot 不写库。种子使用固定负 ID 命名空间、负数 owner ID 和碰撞 fail-closed；不在框架 `users` 表创建种子账户；重复执行保留用户后续编辑。`tests/queue_retry_flows.rs` 使用真实数据库/Redis worker 展示失败退避、恢复、终态与公开重试，不以直接调用 Job 冒充 worker。
- **gRPC TLS 与组合监听**：[`grpc.rs`](grpc.rs) 的 Echo 与 HTTP 读取同一个 `ExampleMessage`。[`tests/grpc_tls_flows.rs`](tests/grpc_tls_flows.rs) 使用同一生产 App 的公开组合函数启两个 listener/两个端口，真实客户端验证 CA、hostname、HTTP/Echo 响应和有界联合停机。不是同端口复用、根 `serve all` 自动装配或完整 R11 验收。
- **业务观测与真实出口**：[`services/telemetry.rs`](services/telemetry.rs) 的 `observe` 只记录固定操作/结果；[`tests/collector_flows.rs`](tests/collector_flows.rs) 由独立子进程连接受控 Collector，traces 走 OTLP/HTTP，metrics 由 Collector 抓取 Prometheus。PostgreSQL 18 与 MySQL 8.4 的 delivery/unavailable 均已通过真实 Collector 验收；PG 使用任务专属 Collector、独立活跃 sink 与唯一 run ID，保持全文件 16 MiB 上限及私密哨兵断言。详细证据见项目测试记录。不称 OTLP metrics，也不称入站 W3C parent 已接入。无商业出口授权时不向供应商发送。

外部 [`test-app consumer`](../../tests/consumers/test-app/) 使用公开 TestApp；其可选 `session` 编译契约确认 `SessionStoreHandle::authorized_until` 从独立 crate 可调用，**不代替真实认证 Session 的行为测试**。真实截止时间、owner/撤销与浏览器握手以相应双库业务测试为证据。

## 新建自己的业务模块

默认在 `modules/mod.rs` 声明编译模块，并登记 `registered_descriptor` 到同一 `ModuleManifest`；Provider、资源仓储与初始化仍经过唯一管线。Rust `mod` 本身不启用模块；移除清单登记则不挂载。默认不需要 module.json 或状态 JSON。需要运行元数据发现及持久启停时，显式选择 `file`/`file-live`，按 [CLI 模块接入](../../tools/cli/README.md#接入自己的业务模块) 登记编译工厂和文件回调。`ModuleLoadContext::directory()` 取模块仓储目录，`module_directory` 只覆盖 files 包含目录，不复制应用或容器。

显式 ModuleDescriptor 与 requires 是已授权的程序化增强，仍可独立使用；不与运行发现清单混用，不把 requires 拓扑伪装成上游 module.json 排序。禁用不热卸载，完整生成/发布和 TTY 管理仍待后继实现。

## 模块里各类代码写在哪里

以下位置已存在，按实际职责拆分；没有业务实现的 models 等目录不预建：

| 实际位置 | 写什么 |
| --- | --- |
| mod.rs | 模块声明，保持薄入口 |
| providers/example.rs | register 登记配置与业务依赖，boot 合并模块配置并贡献路由 |
| config/mod.rs | 在对应 Provider boot 时点求值 example 区段；typed 策略须显式开启 |
| tests/fixtures/config/*.toml | 仅用于显式可选 TOML 来源回归 |
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
    Box::pin(async move {
        bindings.instance(Arc::new(ExampleMessage::new("A test-owned business message"))).await?;
        Ok(())
    })
});
let mut app = fixture.build().await?;
let response = app.request(Request::get("/api/example").header("host", "localhost").body(Body::empty())?).await?;
response.assert_status(StatusCode::OK)?;
let json: serde_json::Value = response.json()?;
assert_eq!(json["data"]["message"], "A test-owned business message");
app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5)).await?;
```

替换在全体 Provider register 完成后、基础配置校验及 boot 前生效。示例同时创建未替换与替换后的两个应用，检查内容互不污染；每个应用都显式 `shutdown` 等待框架拥有的异步资源关闭。完整 reset 会关闭再重建应用；闭包故意共享的外部 `Arc` 不会自动清空。

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

### 草稿重定向与一次性表单状态

`GET /api/example/draft/plain` 与 `/back` 展示从当前 `RequestInfo` 生成普通 302；前者不需要 Session。`GET /api/example/draft/named` 使用 `Redirector::from_info(&info).route("example.index", &BTreeMap::new())` 按公开名称跳转至示例首页，同样无需 Session；目标 URL 由当前请求绑定的路由生成器决定。

`POST /api/example/draft/submit` 使用真实 `Request`，正文 `mode` 可选 `all`、`only`、`except`、`errors`、`custom`。前三种分别调用 `RedirectResponse::with_input`、`only_input`、`except_input`，将当前规范化输入闪存至下一次请求；业务使用 `only`/`except` 决定哪些字段可以带回表单。`errors` 与 `custom` 通过 `Redirector::from_request(&request).route("example.draft.review", &BTreeMap::new())` 生成审核页跳转；`errors` 沿原响应闪存链仅保存标题输入与错误，`custom` 仅保存错误；审核页在模块路由中用 `.name("example.draft.review")` 登记；该审核页仅在启用 Session 时挂载。

`errors` 将默认标题错误与 `other` 命名错误一起写入 Session，`custom` 使用模块自己的 `DraftErrors` 和公开 `MessageProvider`/`ErrorBagCodecs` 扩展恢复业务化的首条消息。页面在原 `errors.any()` 流程内使用公开 `errors.hasAny(["title", "summary"])` 查询多个表单字段，有任一默认包字段错误才显示重新提交提示；自定义 `DraftErrors` 的实际 `title` 键与 `has` 行为仍参与查询，`other` 命名摘要错误照常独立展示。Session 已启用时，`GET /api/example/draft/review` 显式读取旧输入与错误包；`GET /example/pages/about` 通过同一请求的公开视图入口展示这些错误，缺失字段显示空字符串。再次请求后 flash 老化，多字段提示随错误一起消失；现有双客户端用例同时核对初始空包、两种错误包的提示和老化后的隐藏。未启用 Session 时普通与首页命名重定向可用；提交的 `only` 模式仍在写入 flash 时报 `session_layer_missing`，指向未挂载审核页的命名跳转则按未知路由报错。

`GET /api/example/draft/action` 用公开 `Redirector::action` 的二元组身份跳转到匿名的 `/api/example/draft/actions/42?section=review`；`/api/example/draft/action-preview` 使用绝对字符串查询裸 invokable 身份，目标为 `/api/example/draft/actions/42/preview?section=review`。目标在 `RouteGroup::namespace("Example\\Draft")` 内通过 `get_action(path, action, handler)` 与真实处理器同次登记，不依赖路由名称，也不设置整个应用的根 namespace。页面展示 `Route::action()` 和实际参数；这些身份不触发动态控制器实例化。

`GET /api/example/draft/actions/summary` 是静态草稿概览：模块先登记默认概览，再用同方法同路径的真实业务动作覆盖；静态概览在 `/{id}` 前声明，返回 `source=模块草稿概览`、真实动作身份与空 `id`，不会被误当作草稿编号。文章 `/articles/export` 等静态业务入口也先于含 `/articles/{id}` 的资源路由登记；路由按声明首匹配，不依赖框架替晚声明静态路径提权。生成 helper 采用相同公开登记方式，验证默认实现被覆盖与静态路径正确命中。

`POST /api/example/draft/action-submit` 用固定二元组调用 `action_with`，返回303、`x-example-draft-action: review`，不读取请求中的任意目标地址。无 Session 时普通动作跳转可用；存在 Session 时仅闪存标题输入并保存复核错误，目标页展示一次后老化。生成 helper 同时展示 tuple、绝对 invokable 和完整字符串查询，沿同一公开 API 验证匿名身份、参数、响应选项和 flash；原命名路由示例仍保留。

`GET /api/example/draft/secure` 对相对首页目标调用公开 `secure`，返回 `https://` 的 Location；示例只验证重定向响应，不跟随请求或宣称当前 HTTP 监听器已启用 TLS。显式状态、头与协议选择可使用 `secure_with` 或 `to_with_scheme`。

`GET /api/example/draft/signed-links` 使用公开 `RouteUrlGenerator` 为固定草稿 `42` 生成 5 分钟的绝对及相对签名链接，并展示 `Redirector::signed_route` 的目标；生产使用必须设置不可预测的 `APP_KEY`。`/api/example/draft/signed/{id}` 与 `/api/example/draft/signed-relative/{id}` 分别用框架 `signed`、`signed:relative` middleware 验签后才执行 handler，handler 再通过 `RequestInfo` 公共入口确认。更改草稿编号或查询内容返回 403；不使用现有 `SignedLinks` 的 JWT/撤销协议代替 URL 签名。示例仅供开发者理解路由签名，实际私密草稿仍需独立认证和授权。

`POST /api/example/draft/refresh?discard=once` 通过 `Redirector::from_request(&request).refresh()` 返回同路径的 302，目标不保留 query；客户端随后 GET `/api/example/draft/refresh` 会显示真实草稿内容，不再次刷新。无 Session 时同样可以完成 POST→GET；存在 Session 时，响应继续使用 `with_input` 和 `with_errors` 保存一次性的规范化标题输入与复核提示，展示一次后老化。该示例提交仅含公开草稿字段；`with_input` 闪存完整规范化输入，实际业务筛除敏感字段可沿既有 `only_input`/`except_input`。生成 helper 展示 `refresh_with` 的显式 303 和自定义头，同样验证 query 丢弃、GET 内容及闪存老化。

`GET /api/example/draft/guide` 通过 `away_with` 返回固定官方文档外链、303 和业务响应头；目标中的 query 与 fragment 保持原值，不经过 URL 生成器。地址由业务常量提供，请求中的 `url` 参数不会改变跳转目标。生成 helper 使用默认 302 的 `away`；测试只检查响应，不访问外部站点。`refresh/away` 与显式选项方法均保留原请求的响应闪存借用链，普通跳转无需 Session。

草稿预览使用真实 Session 展示确认后的导航回跳：以 `Accept: text/html` 访问 `GET /api/example/draft/preview?section=summary`，未确认时通过 `guest` 跳至固定 `/api/example/draft/confirm`；框架把本次真实 GET 路由的完整 URL 写入普通 `url.intended`。确认页使用 `get_intended_url` 仅读取，连续刷新不会像 flash 一样老化。`POST /api/example/draft/confirm` 只记录本模块的非敏感预览确认状态，再调用 `intended_or("/api/example/draft/review")` 一次消费地址；返回原预览后展示草稿内容，不再次跳确认页。再次提交确认时使用固定审核页回退。请求正文不接受任意跳转 URL，两份 Cookie 的意图互相隔离。

此草稿步骤不登录账户、不授予权限，也不替代既有真实 Session 登录、CSRF 或 CRUD Policy。当前账户登录仍使用原凭据/Origin/Session 流程及 JSON 响应。`guest` 只有 GET、已匹配真实路由且非 `expects_json` 时保存当前完整 URL，其他请求使用 previous；`intended` 的缺失默认是 `/`，显式默认使用 `intended_or`。生成应用的公开测试 helper 保持同一入口，验证 HTTPS Location、双 Cookie 的普通意图持久性、一次消费与默认 `/` 回退，并继续执行原输入、命名错误及视图 flash 老化。

codec 在模块 `ExampleServiceProvider::register` 中通过 `RegisterContext.extend` 追加到 Session Provider 的注册表，保持已有登记。`DraftErrors` 的类型、编解码与业务规则均位于本模块。命名参数当前使用 `BTreeMap<String, String>`；默认状态为 302，自定义状态及响应头使用 `route_with`。示例覆盖手动普通、返回、命名、动作、scheme、刷新、固定外链、签名 URL 与 Session 导航回跳；URL 参数的其余扩展与自动校验失败重定向仍由后继能力验收。请求局部视图使用同一请求的 `RequestInfo` 与公开 `Views::render_for_request` 入口。

“代码存在”仅说明当前可见实现，不能替代本布局 check/test、真实请求与外部服务验收。根基础设施装配、文档链接或 consumer 测试也不自动算作 `modules/example` 的业务代码覆盖。

| 能力 | 本模块实际代码/公开扩展 | 根接入与条件 | 测试/当前缺口 |
| --- | --- | --- | --- |
| 完整业务实体 CRUD | `models/article.rs`、Article 控制器与 `ResourceActions` 覆盖列表、详情、创建、更新、删除 | 模块文章/事件迁移；`auth` 与数据库启用时挂载 | PG18/MySQL8.4 真实 HTTP 已覆盖两用户/匿名、校验/分页/404/越权、事务回滚；当前源码整组门禁已执行，本簇仍待独立审查和提交 |
| HTTP 路由与 JSON 响应 | `routes/api.rs`、Article `ResourceActions`、统一错误 envelope、业务限流与安全响应头 | ExampleServiceProvider 公开贡献模块 routes，包含 `/api/example` | 根 server、双库真实 HTTP CRUD 与各业务路由定向通过；当前源码默认/全特性整组门禁已执行 |
| 草稿重定向与 Session 错误包 | `http/controllers/draft_redirect.rs` 通过公开 `Redirector` 实现普通跳转、输入 flash 与命名错误；`services/draft_errors.rs` 实现自定义错误包及 codec；`pages.rs` 使用请求局部视图入口 | 模块 Provider 用公开容器扩展追加 codec；有 Session 时挂载审核页，无 Session 可渲染空错误包 | 模块 TestApp 覆盖两客户端并发视图、默认/命名/自定义包隔离及 flash 老化；生成应用通过公开 helper 验证请求重定向与视图接线。PG 兼容性保留在发布矩阵 |
| 配置、应用生命周期、命令 | 模块类型配置与 boot 时 overlay；显式 `example attachments reconcile`、`example imports reconcile` 维护命令 | 根 bootstrap、config、唯一 server 与模块 Provider | TestApp 启停及配置边界已测；命令的生产运行/退出组合仍需最终门禁 |
| 数据库、迁移、种子、事务、查询 | Article 实体、owner 约束查询与主库事务；模块事件监听器同事务写入；显式种子命令创建不可登录隔离身份与两篇固定负 ID 文章 | 模块 database/migrations 与 database/seeders，descriptor 经公开 Provider 挂载；对应 db feature 与真实服务 | 双库 HTTP/迁移及真实 PG/MySQL `db seed` 各3项通过；boot 不写种子、并发重复幂等、碰撞整笔回滚、编辑保留 |
| JWT/PAT、Session、Fortify 与 OIDC | Web/Session 与显式 JWT API 共用 Fortify 状态；具名 Guard 保护模块账户资源；OIDC 调用公开 ExternalIdentityFlows | `auth-fortify`、显式 profile、数据库/加密及可信 IdP 配置；浏览器 RP 另需 Session/Cookie | 当前组合的派生凭据、关联末检、双库/Keycloak 与生成应用验收尚未闭合；UUID JWT 持久链和完整浏览器自动导航仍为缺口 |
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

### 入站请求地址

`GET /api/example/request-url?label=public&page=1` 与同地址的 POST 经普通模块路由调用 `Request` 的 `root/url/full_url`、`scheme/http_host/port`、`decoded_path/segments/segment`、`is/full_url_is`、`prefetch` 及查询增删方法。演示仅接受无查询或上述固定公开查询，其它查询返回 400；不回显 Cookie、Authorization 或正文。

`address` 也通过公开 `client_ip/client_ips/base_url` 展示同一请求的客户端链与代理前缀；无 socket 对端的进程内请求可能没有客户端地址。代理信任范围和头集合由宿主配置，模块只消费框架已确定的上下文，不自行解析转发头或信任客户端声明。

`address` 展示原请求；`merged` 和 `replaced_with_query` 展示当前输入修改后的查询投影。非 JSON GET 的 `merge/replace` 修改当前 query bag，因此影响 `full_url_with_query/full_url_without_query`；`full_url` 始终来自原始 URI。输入来源优先判断 JSON：带 JSON Content-Type 的 GET 与 POST 均修改 JSON 正文输入，不混入地址查询。`/api/example/route-url` 则展示按命名路由生成出站链接，与读取当前入站请求地址分属不同职责。进程内测试不代表真实 TCP/TLS 已验收。


### 真实请求地址验收工具

[`tests/tools/verify-request-url.py`](tests/tools/verify-request-url.py)以`--binary`、`--binary-sha256`和必需的`--source-snapshot`运行；快照为构建前采集的Git tracked/untracked相对路径到SHA256的JSON对象，构建及运行前后都须一致。工具从仓库根运行，或显式传`--root`；只启动两个任务隔离的临时配置后端，复用框架公开模块装配，不操作应用数据库。

工具验证实际TCP和显式CA/hostname TLS代理链、可信/不可信proxy、三种query来源与错误/隐私边界。TLS由工具持有的代理终止，不能代替Rust原生TLS适配器验收；未监听端口在不同平台可拒绝或超时，报告记录实际类别。父supervisor控制执行和清理预算，结束后回收自身后端/线程与临时私钥、配置，只保留脱敏结果；失败结果不会标记accepted。

### 请求的路由上下文

`GET` / `POST /api/example/route-context/named/{id}` 由公开 `http::routing::get(...).post(...)` 受管理声明注册，路由名为 `example.route-context.show`；`GET /api/example/route-context/anonymous/{id}` 是匿名路由。响应展示 `RequestInfo::route()` 的最终 URI、方法、路径参数、`route_name()`、`route_is()` 和缺参数默认值。查询或 JSON 正文中的 `id` 不覆盖路径参数，匿名路由的名称为空。所有第一方受管理 `Routes::route` 调用须传 `http::routing` 的 `RouteMethods`；原生 Axum Router 是独立逃生口，不提供此匹配上下文契约。

将 `{id}` 换为 `missing` 可触发业务 404，换为 `blocked` 可触发路由 middleware 的 403 短路；具名响应仍带 `x-example-route-before` / `x-example-route-after`，证明上下文在路由 middleware 之前发布并跨错误保留。参数当前是 JSON 字符串值，不表示模型或枚举绑定已经实现。出站 `RouteUrlGenerator` 继续负责生成链接，不提供当前请求的命中事实。

[`tests/tools/verify-route-context.py`](tests/tools/verify-route-context.py)接收与 D6 工具相同的 `--binary`、`--binary-sha256`、`--source-snapshot` 和可选 `--root`。它复用 D6 的配置隔离、来源快照、资源门限、有界请求、进程组守护和任务专有清理，真实启动一个加载唯一模块的 server，检查具名/匿名、参数/default、输入伪造、业务 404、middleware 403、HEAD 与方法 miss。PG/MySQL feature 的二进制分别运行，但工具不连接数据库，不能据此声称数据库或 TLS 等价验收；临时目录沿用共享 supervisor 的 D6 前缀。

D7 工具使用有界原始 TCP 读取 HEAD 响应直到 EOF，完整解析响应头并确认没有额外正文，且路由 before/after 两个标记均正确；方法 miss 只证明路由 middleware 未执行，不据此推断私有共享 slot。来源快照与二进制 SHA256 分别证明检查对象未变化，二者本身不能证明该二进制由该快照构建；验收须附记录构建命令、来源快照摘要及产物摘要的实际构建记录，将构建与后续网络结果关联。

账户示例的 `/account/session/*` 路由也只在宿主实际登记 Session 且最终驱动启用时接入 `cookies`、`session` 别名；仅启用数据库或认证编译能力不会强制 Session。JWT 及普通模块 API 仍可在无 Session 的宿主中运行。

### 模块父 Provider 与独立子项

默认编译 descriptor 依次登记 `ExampleModuleServiceProvider` 和复用原 `ModuleFile` 的初始化 Provider。它在 register 中按原条件和顺序 await 框架的 `register_provider`，状态日志、真实事件监听、业务路由及适用数据库/队列等子项仍各自保有 ProviderId 与真正的 register/boot；父 Provider 不手工转调它们的生命周期。禁用模块不构造这些运行状态；测试移除业务子项仍生效，不能被父重新装回。此接线为后续 module.json 的普通 ModuleServiceProvider 工作流提供前置，不代表运行发现已实现。

## 数据库能力选择

普通模型、迁移与业务代码按其通用能力编译，不把 MySQL/PG 连接器 feature 当作 ORM 开关。活跃模块总注册迁移与 seed 命令贡献；有无连接不影响注册，命令实际执行才检查连接。生产模块在真实 register 读取同一配置仓储的默认 URL，决定默认库业务子 Provider；具名连接不冒充默认库，未配置默认库时宿主与基础示例仍运行。具体官方 URL 的驱动必须已编入，非法配置继续明确拒绝；这不代表示例已支持 SQLite 方言或完整 Laravel 懒连接。

模块的附件维护命令由普通 Provider 贡献到同一命令目录。公开 `App::commands()` 快照持有命令实例；重新引导生成新的目录与实例，先前取得的快照不会改指向新实例。实际维护执行仍通过 `App::run_command` 进入当前应用的命令 scope，不把快照本身当作执行授权或后台 worker。

## 独立数据库连接

配置 `database.instances.reporting` 后，`GET /api/example/reporting-connection` 从同一 Container 注入 `Databases`，探测固定 reporting 连接；响应只包含 backend 与 reachable。此路由可使用内置 SQLite，不依赖默认库业务，也不允许请求指定连接名或暴露 URL。回归在 `tests/sqlite_connection_flows.rs`；完整 SQLite 认证、迁移和队列仍待后续。

## SQLite本地账户流程

内置SQLite连接仍由根标准Provider和同一迁移命令装配；唯一模块的account路由复用公开AuthFlows、Session、PAT和MFA服务，不另建SQLite账户系统。模块测试覆盖原生Session登录/跨请求/CSRF、令牌撤销和密码重置，以及TOTP确认、一次性恢复码、审计失败回滚。完整R7、外部OIDC、SQLite队列/缓存锁等不能由本地账户流程推导完成；机制、错误码及单机限制见[认证边界](../../src/auth/README.md#sqlite-本地认证边界)。

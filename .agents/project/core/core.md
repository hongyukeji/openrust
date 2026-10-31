# `openrust::core`

> 受众：AI Agent
> 更新：2026-10-30
> 摘要：应用内核当前事实；模块清单支持硬依赖和确定性拓扑启动。

## 边界

只提供装配、配置、错误、命令与生命周期；不含路由、数据库或任何 driver。依赖：`axum-core`（`IntoResponse`）、`http`、`figment`、`serde`、`tokio`、`tracing`、`toml`。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `app` | `Application { type Config; fn config_namespace(); fn providers(&Config) }`，默认命名空间 `APP_`；`App` 提供 `state`、`config`、`figment`、`commands`、`extension`、`run_command`、显式 `shutdown`；`AppState` 的 `resolve` 异步构造工厂，`get` 仅同步读取已就绪实例。每次命令建立独立 scope，正常返回显式等待清理，调用 future 取消时关闭 guard 交由 App 受管回收；命令可复用同一 App，顶层 CLI 退出时关闭资源。section 解码错误保留安全键路径，不回显值 |
| `container` | 生产唯一 `ContainerBuilder`/`Container`/`Resolver`/`Scope`：bind/singleton/scoped/instance、dyn 接口、单次构造、环检测、取消和根 reset。HTTP 请求 scope 贯穿 Inject/Auth/RateLimit/响应 body 轮询；命令、Job attempt 与 scheduler trigger 各用独立 scope。`Scope::run` 仅在当前 future 安装同容器 task-local，`Scope::spawn` 在同 scope/显式 ExecutionMetadata 下构造并执行受管任务；普通 `tokio::spawn` 不继承。关闭 App 封闭根生命周期并等待受管清理 |
| `provider` | `ProviderBuilder` 先完成所有 `ServiceProvider::register`，合并基础配置，顺序执行 `boot`，最后校验配置和贡献并解析 required 服务；Provider 依赖、同类型去重与 boot 顺序明确。资源由 `ResourceOwner` 管理，启动失败或取消后在存活 Tokio runtime 内有界回收；旧 Plugin 启动循环已删除 |
| `module` | `ModuleDescriptor::requires` 声明已编译模块间硬依赖；启用模块的依赖必须同时编译、启用，重复/自身/循环依赖拒绝。依赖拓扑优先于升序 `priority`，相同优先级保持应用编译清单顺序；模块 Provider register/boot 共享该结果，不自动改写运行启用状态 |
| `command` | `Command { name, about, accepts_args, run }`；额外参数默认拒绝；内置 `help`、`config show`（密钥遮蔽） |
| `config` | `src/config/` 已有 app/api/auth/cache/database/logger/mailer/server/signing/sms，queue 与 security 随对应 feature 开启；单一 `ConfigLoader` 合并类型默认值 → 应用根 `config/*.toml` 领域文件（按文件名排序）→ 应用工作目录 `.env` → 进程环境变量；不再加载 `default.toml` 与环境名 TOML。默认 `APP_`；Laravel 风格 `DB_*` / `REDIS_*` 分项连接变量、直接 URL 简写、`APP_PORT` → `server.port`、`APP_NAME` / `APP_URL` / `APP_TIMEZONE` 均映射到领域配置（`app.timezone`默认UTC，启动时校验；HTTP日历使用最终Provider配置覆盖后的时区），应用命名空间变量优先；`FrameworkConfig { app, server, api, log, i18n }`，其他能力通过独立 section 读取；数据库、认证、邮件旧模块路径由 re-export 保持；`NoConfig`、`load_dotenv`、`redact_secrets` |
| `error` | `Error` 枚举（`BadRequest` / `Unauthorized` / `Forbidden` / `NotFound` / `Conflict` / `Validation` / `ValidationDetail` / `RateLimited` / `RateLimitedWithRetryAfter` / `Config` / `Custom` / `Internal` / `ServiceInitialization`），`status()` / `code()` / `to_api_response()`；`validation_with_detail` 用于 JSON 结构错误，`custom_with_status` 承载 405/408/413/415 等稳定码，`IntoResponse` 输出 `application/json`；`ApiResponse<T>`、请求作用域 `ResponseContext`、`ValidationErrors` |
| `execution` | `ExecutionContext` 为 HTTP、命令或一次队列尝试提供 task-local metadata、单调时钟 deadline 与合作取消；子调用 budget 不超过父 deadline，子取消不向父级传播；`ExecutionMetadata` 仅可跨队列保存有界 request ID 与 `Locale` |
| `validation` | `Validate` 与 `validate(value)` 公开复用现有 validator，嵌套错误映射到统一 `Error::Validation` 字段路径；HTTP `Validated<Json/Query>` 使用同一入口 |
| `rate_limit` | `RateLimiter` 与 `RateLimitStore`；按稳定 key 原子消费固定窗口，服务统一策略校验和 429/Retry-After 语义，store 故障传播；core 不决定 HTTP 身份来源或中间件 |
| `lifecycle` | `bootstrap` / `init_tracing` / `shutdown_signal`；bootstrap 使用唯一 Provider 启动引擎。Scope 关闭先封闭解析并取消子任务，再由父 ResourceOwner 的单一持久清理 driver 排队执行有界 shutdown；最多 4096 个待清理子 scope，超过则返回 `scope_capacity_exceeded`，不丢弃已登记回调 |

分页 `meta` 使用 `Option<PageMeta>`，`with_meta` 直接保留强类型值。

### 控制台迁移与启动校验（D10-B 限定子范围已验收，待提交）

顶层 CLI 在引导前持有候选命令及参数；`BootContext::console_command` / `is_console_command` 只描述候选首词，不能证明最终命令身份。设置上下文的入口限制在框架内部，普通 `bootstrap`、公开 Builder 与 TestApp 保持原严格启动校验。

MFA 在候选为迁移时仍解析服务与配置，仅登记延后 schema 检查；派发复用现有最长命令匹配，以实际框架 `MigrationCommand` 类型决定是否免于运行态五表预查。其他实际命令（包括同首词多词自定义命令及后续 `serve`）先执行检查；迁移按原 up/down/status/命名数据库语义返回，不附加会破坏合法回滚的五表后置条件。内部一次性 CLI 保留关闭与 telemetry 收尾，不对外返回可复用免检应用。

固定 Laravel Console Kernel 先持有 InputInterface 再 bootstrap 的流程是对应依据；Rust 用明确候选上下文与最终命令类型绑定保护既有启动检查，非全局 argv/env 开关。真实 MySQL 8.4 私有库已验证空库 status、up 1、down 1、命名库 status、全量 up，以及删自有 MFA 表后普通 serve 缺表拒绝；库已精确清理。原迁移失败历史/重启恢复测试本轮没有实际数据库连接，不在上述证据内。见[限定验收](../memory/testing.md#c04-d10-b-请求绑定重定向与-session-错误包限定子范围已验收)。

## 公共 Support 错误包

`openrust::support` 的 `MessageBag`、`ViewErrorBag` 不依赖 Session 或应用容器；核心装配不持有请求错误。`MessageBagContract` 与 `MessageProvider` 为公开扩展契约，命名包以原对象借用派发；`ViewErrorBag` 的默认代理只读取 `default`，缺失包是临时空值，不插入共享状态。

消息构造接受 String；`Collection`/`ArrayKey` 保留顺序与稀疏键。`MessageValue::Nested` 表达递归合并派生数组；外层 merge 重编号整数键，内层按固定 PHP 实际规则保留原稀疏键并追加。`add` 的数值字符串宽松去重与构造/unique 的字符串去重分别实现。嵌套值的非默认格式错误对应 Laravel `HandleExceptions` 将警告转异常的路径，裸 PHP 的警告后继续行为另列参考，不能混称相同。

`MessageValue` 序列化先转为具体值类型，避免递归叠加泛型有限数值检查器；通用 Collection 的非有限数拦截和整树预算仍保留。受控写入先校验候选完整树再发布，失败不替换原值或自定义包实例。用户入口见根 README 的消息包章节。D10-B 已把 Session `errors` 接入公开 codec 和请求绑定重定向并通过限定验收；请求局部视图自动共享仍按 todo 后继实施。

自定义包使用原 `MessageBagContract` 与 `MessageProvider`，不新设平行错误体系。Session Provider 发布默认 `ErrorBagCodecs`，内置 `MessageBag` 无需注册；可信业务 Provider 可通过 `RegisterContext::extend::<ErrorBagCodecs, _>` 装饰既有绑定，再以稳定 type ID、版本及编码/解码函数登记自定义包。编码和恢复回调在 Session 属性锁外运行；失败、未知版本或预算超限不能部分替换已存错误。固定 Laravel Store 对照中，PHP 序列化保留自定义对象，JSON 保存后恢复为基础消息包；Rust 为持久化自定义行为使用显式类型化 codec，非 PHP 字节格式兼容。

限定验收入口见[testing](../memory/testing.md#c04-d10-a-错误包限定子范围已验收)；不据此关闭完整校验器、重定向或视图错误共享。

## R2-R02 有序来源入口（限定验收通过）

`ConfigSources` 是不可变有序原值与Figment投影的同次输入，`ConfigLoader::sources()`供默认引导一次捕获后同时读取应用类型配置和启动Provider；仓储仍只有同一个State与版本。既有`ConfigRepository::new`、`ProviderBuilder::start`和`TestApp::builder`接收该输入，不增加平行启动API。旧Figment输入已丢失的声明顺序不能恢复；运行期set不重建已经解析的资源。

本簇已通过首轮142项、默认workspace853/全feature1642、双严格Clippy/docs、features/MSRV/deny、consumer5项及fresh生成原7+fixture1/MySQL2、真实TCP与资源收尾；命令、忽略项、失败历史及SHA见[本簇验收](../memory/testing.md#r2-r02-config-sources)。来源自然排序不等于完整Laravel配置发现：递归目录、文件名到配置namespace映射及类型表示差异仍为完整Config待办。当前实现不得据此宣称全量Config对应完成。契约见[ADR-086](../decisions/adr-086-config-repository-reading.md)。

## R2-Q/X 已验收子范围

R2-Q 分支 `850e1a4` 与 R2-X 分支 `d299ca4` 已完成各自限定验收；main 已完成实际组合验收，含双库、TCP和五类fresh生成应用；本地集成提交见Git，证据见[testing](../memory/testing.md#r2-qx-main-组合验收通过)。不代表完整配置/Container/Facade已对应。

- 配置读取复用唯一 `ConfigRepository`：`get` / `get_or` / `has` / `get_many` / `get_many_with_defaults` / `all`。完整字面键优先，缺失与显式 null 分开；批量读取固定同一快照。原始返回值可能含秘密，调用方负责控制输出。无 schema 的来源错误仅保留可信父键；有 schema 时仅保留声明路径，动态 Map 名和来源数组段不回显。见 [ADR-086](../decisions/adr-086-config-repository-reading.md)。

- R2-R01 限定交付：同一仓储的 `set` / 有序 `set_many` 将原始值与 Figment 投影成对发布，`App` / `AppState` / Provider 共享句柄，`App::section` 读取当前值；`App::config` / `figment` 和既解析服务保留启动快照。启动 schema 屏障不等于禁止可信应用代码的运行期原始写入。已通过配置仓储默认/无默认各39项、App句柄1项及模块/CLI/consumer定向，双严格Clippy、工作区845/1632项、docs/features/MSRV/deny、fresh生成/MySQL8.4及真实TCP均有本簇证据；资源收尾通过，限定本地提交见Git；证据见测试基线。R2-R01的Figment入口不能恢复此前丢失的来源初始键序；R2-R02已对有序来源入口限定验收，既有Figment有损入口仍不能恢复丢失顺序，完整发现/命名映射继续见唯一todo。
- Container `extend` 复用现有工厂、作用域与发布流程，按顺序装饰原实例；示例 Provider 在首次解析前安装装饰器，并核 boot 与 HTTP 同一实例。可信 Session driver 装饰器通过 `SessionStoreHandle::store()` 取得原 store；该逃生口不能代替请求 Session 状态及其容量/删除检查。见 [ADR-085](../decisions/adr-085-container-binding-extension.md)。
- `modules/example` 的固定配置键路由与 Container 装饰器是公开调用示范；任意 query 不扩张允许读取的配置键。完整配置修改/动态设置、Provider 自动发现及完整 Container 对应范围继续按唯一 todo 领取，不能因本子范围关闭。

## 约束

- Provider 清单由应用在启动前一次确定，后续 Provider 不能动态增删启动清单。
- `Application::Config` 不能是 `()`，用 `NoConfig`。
- 5xx 错误只输出通用 `message`。

## 后续边界

- `/ready` 聚合已注册依赖探针；外部服务和完整集群健康语义继续按专项验收。
- `tokio::spawn` 不继承 task-local；跨任务只显式传递所需 metadata/cancellation，不捕获身份和任意请求数据。
- 生产启动已使用 Laravel 风格 register/boot；公共业务流程钩子和事务前后扩展的 Laravel 逐项等价仍需后续验收。

## 验证

`cargo test -p openrust`：配置分层、APP_ 默认和自定义命名空间、`.env` 引导、前缀优先级、错误键名、密钥遮蔽、错误渲染，以及 Provider 全注册/顺序 boot、容器解析与资源关闭。

限流单测验证 quota 边界、固定窗口重试语义、非法策略和存储故障关闭。

R2-A：新增服务工厂错误保留内部source，公开Error Debug仅code/status，HTTP仍按5xx统一脱敏；容器基元尚未接入生产启动，不代表完整ServiceProvider或异步资源生命周期完成。

R2-A2：ConfigLoader::figment现为fallible；显式目录或已发现文件无效即失败，默认未声明且缺目录仍可用内置默认。ConfigLoader Debug/引导日志只暴露来源key；未知字段schema、Provider模块配置合并与单次配置快照仍待对应R2子簇。

R2-A3：缓存工厂结束帧/出向等待边先于结果发布；完成slot不建等待边。scope/root reset先锁内提交状态，锁外通知/释放用户值；同步Wake重入不持State/outcome锁。生产Provider迁移和异步清理边界仍不变。


## R2-R03 递归配置发现与晚到 Provider 默认值（任务分支限定验收完成，待提交）

`ConfigLoader` 递归发现领域 TOML，使用真实 basename 建立文件命名空间；隐藏文件和目录 symlink 不作为配置来源，同一命名空间在实际遍历中后项覆盖，再按完整名称自然序加载，不承诺跨文件系统的碰撞胜者。根领域 TOML 一次迁移为相对内容；模块配置沿用既有独立 overlay，新增 feature 文件展示公开读取，CLI 从既有模板拆分并保留格式化键与注释。配置仍由同一 `ConfigSources` / `ConfigRepository` 保存点路径与来源顺序；模块 overlay 保持独立。

晚到 Provider 默认值按浅层合并；`mergeableOptions` 中声明的数组按固定 PHP 边界浅合并。仅发现子文件时，文件前 base 加晚到 section defaults 重建父区段，再重放文件后环境层，保留环境优先、metadata/profile 与 raw/typed 投影；真实 PHP `after_final` 的剩余父 base 根覆盖不被误当缺键。无文件的程序来源沿原 join 路径，不新增第二个公开仓储。当前实证只覆盖指定 Laravel/PHP 场景，不代表完整 Config bootstrap 或全部值类型。

本分支最终定向配置 76、CLI 21、唯一模块 2、公开 consumer 6，严格 Clippy、Rustdoc 和 fmt 均通过；普通 fresh 生成 7 项及安装 consumer 1 项通过。无 auth MySQL 生成只编译、测试 0 项；另以 fresh MySQL 8.4 认证项目实际通过 2 项且无 skip，测试库集合前后一致。真实根 HTTP 单独验证 `GET /api/example/config-feature` 为 200、`enabled=true`、服务 exit 0；URL/TLS runner 另有 10/10 场景、两服务正常退出与来源/二进制稳定，不把后者冒充配置功能 HTTP 证明。执行者终端回报三个唯一生成包清理命令 exit 0，原始 stdout 未留存，不能独立复算逐命令日志 SHA；登记 16 路径 54.48 GB、磁盘空闲 8.782%，触软线未触硬线。本簇本地提交见Git，未合入 ENV main，main 完整组合门禁、完整 V1 Config、typed helpers、配置缓存、动态 Settings 与发布全后端矩阵仍开放。详见[本簇记录](../memory/testing.md#r2-r03-递归配置发现任务分支限定验收记录)。

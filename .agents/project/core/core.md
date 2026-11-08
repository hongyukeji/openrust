# `openrust::core`

> 受众：AI Agent
> 更新：2026-11-02
> 摘要：应用内核当前事实；模块清单支持硬依赖和确定性拓扑启动。


## 模块扩展与通用装配边界

AR-M01已由`6c21bab0`完成[限定验收](../memory/testing.md#模块扩展独立包限定验收)。模块类型、配置、发现、启停、资源和命令现由`openrust_modules`提供，下文模块行为事实归该扩展包；framework只持通用Container、Provider及装配hook。`ProviderRegistration`在用户register前准备初始绑定，保留插入位置、尾部子Provider、条件fallback及bootstrap命令顺序；根解析拒绝构造帧内绕过作用域。应用和TestApp通过公开hook安装扩展，未安装扩展时remove_module不隐式启用模块机制。完整deferred/aliases与模块生成等后继保持开放。

当前宿主：显式模块注册与宿主API前缀已完成[限定验收](../memory/testing.md#显式模块注册与宿主-api-前缀限定验收)：10项主门禁、6个真实server进程、fresh minimal9/MFA真实MySQL3、生成模块持久启停1及只读空目录负例通过；首批8页mdBook构建/链接、112生成源码归档、不同模型独审和target清零完成，本地提交以Git为准。registered为应用约定，库file默认保留；完整Modules/Routing与全量文档未关闭，下一SQLite及CLI后继按唯一todo。 ModuleActivator::ensure_writable为可写驱动的公开前置；or_api_envelope_when按逻辑或组合应用/模块错误范围。普通参数25上限是待消除的后端差距，不作Rust必要适配关闭。

当前R2共享生命周期的命令所有权前置已完成[限定验收](../memory/testing.md#共享命令所有权前置限定验收)：命令目录共享，App/ProviderRuntime返回不借用宿主的Arc命令快照，动态同名替换保原槽，回调/析构/await不持目录锁；12门禁、MySQL模块1、fresh9/3及实际help通过，本地提交以Git为准。完整deferred/manifest/when、扩展及ProviderRegistry共享仍按唯一todo，不据前置关闭。

## FileActivator 默认快照（限定验收）

默认file构造读取并返回Result，缓存与本实例后续修改独立于其他writer。显式with_live_reads/file-live保实时扩展；文件driver以惰性工厂在实际包register/legacy准备处构造，自定义driver仍同trait共享。set/delete先改缓存、失败pending不丢，reset清缓存；锁内物理合并/原子替换与严格IO错误属已授权质量增强，不改变默认缓存视图。PHP数字键/list与空表codec、同目标同字节的自身提交恢复及外来坏文件保护已配套，new/reset_file/公开调用方已同步，限定适用验收已通过；完整Modules仍开放。

固定真实FileActivator/Filesystem/ConfigRepository已在PHP8.5隔离执行，Container寻址与Module.getName仅作测试替身，不是整套Laravel应用；构造缓存、写失败后的memory/retry、cached noop、delete缺文件重建、数字list及空path事实保全tmp/file-activator-cache-20261102/reference/。751输入十门禁、fresh12/MySQL3/MFA2/root1及四TCP已通过，独审/归档与target清零收齐，见[限定验收](../memory/testing.md#fileactivator-实例缓存限定验收)；本地提交以Git为准，skill独立owner仍独占。

## module.json 运行发现（限定验收）

包Provider在register读取同仓储路径/状态并发现元数据（仓储/Provider资源取manifest实际目录，仅files使用module_directory覆盖），命名编译工厂注册真正Provider，随后编译ModuleFile初始化；根/生成模板及唯一example已接线。条件候选仅参与可达性，实际依赖仍核Registry。Module::priority改为原始JSON值，Manifest/Repository排序与CLI显示分别按固定PHP契约实现；2068真实PHP向量已在最终默认/全特性对照通过。显式ModuleExecution保存同执行的Manifest/文件状态，初始化并发等待、错误/取消封闭、同链重入报错；跨tokio::spawn不自动传播。完整适用门禁、fresh/真实MySQL/TCP及资源收尾已通过[限定验收](../memory/testing.md#模块运行发现与编译文件限定验收)，本地提交以Git为准。

class aliases与完整deferred加载、非有限JSON值、非UTF8 wildcard组件仍缺口；本机两原始字节目录fixture被OS errno92拒绝，只标平台无法执行，不冒称scanner通过。原失败与修复回执位于tmp/module-discovery-20261102/，仅本地提交事实以Git为准；FileActivator实例快照/顶层数值键已按限定范围纠正并验收，DB能力门控为下一整改。skill仍独立owner独占。

## 嵌套 Provider 注册（限定验收）

同一内部ProviderRegistry用于启动和已启动App，RegisterContext新增即时register_provider/force/boxed入口及公开config_repository句柄。成功子项先入有效序列；startup统一register屏障后boot，live子项在父下一语句前boot。force首槽、失败贡献、原实例及进行中RAII取消标记由同一执行器处理；启动mask覆盖嵌套force，运行期显式注册保原接口。默认空Events改用不同ProviderId补位，真实嵌套Events不被吞。唯一模块和生成consumer实际调用、完整适用门禁、生成应用及真实MySQL/三进程均已通过[限定验收](../memory/testing.md#同内核嵌套-provider-注册限定验收)，完成产物清理；本地提交以Git为准。nested_providers纯声明给原静态预检补足外部引用的子身份，执行后校验真实登记；未声明的未来类型无法反射，完整依赖/发现仍开放。

## 模块持久管理（限定验收）

同一次 ModuleManifest 解析生成已编译 catalog 与加载标记，ModuleRepository 复用该 seed 和所选 ModuleActivator，不复制 Provider。文件驱动保留原始键和值，状态按 canonical 精确键读取；管理 find 支持原有大小写/Studly 查找。写操作稳定侧锁内重读并原子替换，显式 reset 可删除坏状态；无自动恢复。启停具名事件沿同一 Events，弱 Resolver 每次从当前根容器取事件服务，兼容重绑定与根 scope 更新；前事件失败/取消不写，后事件失败保留已提交状态，Stop 仅停止传播。并发 shutdown 与文件写入没有全局原子屏障。

核心命令完整启动后执行，多名称预检、顺序及重复保留，--all 覆盖名单，异常保前项并中断后项。激活状态变化不热卸载/挂载。根/生成配置支持独立状态文件及 application_root；唯一模块真实调用、生成consumer与适用门禁/真实服务已限定验收，见[证据](../memory/testing.md#模块持久启停与同容器管理限定验收)。运行 module.json 发现、Provider/files 接线、TTY 选择、硬链接别名和Windows原生证据仍开放；本段仅关闭上述持久管理子范围，不作完整Modules声明。

## R2-P Provider 注册生命周期（与 S-DV 的 main 限定组合已验收）

`Container::flush` 清空运行期绑定、别名、解析历史和所有作用域的共享槽。保留编译类、环境、持久装饰器和注册规则，旧实例及已取得的原工厂快照继续可用。旧构造无法回填已清除的缓存，用户析构在锁外运行。[限定验收](../memory/testing.md#容器清空绑定限定验收)

`Application::flush` 在同一次容器清空中撤销普通解析与重绑回调，并清空应用的有效 Provider、命令和宿主扩展。显式重新登记 Provider 后立即启动。保留配置快照及原受管资源，关闭仍沿 `Application::shutdown` 执行。[限定验收](../memory/testing.md#应用清空注册限定验收)

应用 `booting` 在全部 Provider 注册之后执行，`booted` 在 Provider 启动完成后及最终配置验证前执行。两阶段按实际队列读取后续登记。已启动应用即时执行新 `booted`，`flush` 清除两个队列但保留已启动状态。[限定验收](../memory/testing.md#应用启动回调限定验收)

Provider 实例可在 `RegisterContext` 或该实例的 `BootContext` 登记自己的 `provider_booting` 和 `provider_booted`。实例队列随注册条目保存，启动与运行时共用执行器，强制替换保留槽位并使用新实例队列。[限定验收](../memory/testing.md#provider-实例回调限定验收)

应用 `terminating` 共用一份异步队列。控制台退出和 HTTP 响应正常完成后使用原入口作用域执行。`flush` 清空队列并隔离旧遍历，进程资源仍由 `shutdown` 回收。[限定验收](../memory/testing.md#应用终止回调限定验收)

HTTP 与控制台在原入口作用域内解析同一 Events 服务，并在应用回调前派发 `foundation::events::Terminating`。派发失败中止后续回调，停止传播只终止本次监听器链。HTTP 在事件与应用回调之间执行[终止中间件](http.md#http-终止中间件)。生命周期时长处理器与通用 `Container::call` 仍按待办推进。[限定验收](../memory/testing.md#内核终止事件限定验收)

同一 `Container` 在首个 Provider 的异步 `register` 前建立；`RegisterContext` 的绑定、实例、`extend` 与 `rebinding` 在声明位置 `await`，`configure_after_register` 仍在全部初始 register 后、boot 前操作共享容器。普通重复 Provider ID 保留首个；force 重新执行 register，新有效项沿首槽 boot 顺序；App 启动后仍可登记 Provider。register 失败不替换旧索引，boot 失败保留新索引，已发布贡献不伪装成全局事务回滚；失败或取消依受管 deadline 回收资源。

已绑定服务登记 `rebinding` 时立即解析并返回当前实例；登记本身不调用新回调，后续真实重绑在原位执行回调，错误阻止后续 register 语句。绑定 generation 与缓存槽同步准入；旧 `Arc` 可继续使用，后续解析不让旧工厂或等待者填入新槽。配置登记即时可读，来源值优先于默认值；后到 Provider 默认值不回滚先前成功发布的 `merge_defaults` 数组贡献和显式 `set`，启动仍保留校验与缓存屏障。Cookie 已解析单例保持首次快照，晚首次解析可读取新配置。当前只证明重建 Router 可见新贡献；已服务 Router 热更新、Provider 自动发现、完整 Facade/Container 仍开放。来源 `4ac8099`、S-DV来源 `6ffb5c8` 与三父main提交 `9e5a94198b421987851b3580b27fa90370feeb77` 的实际门禁见[组合记录](../memory/testing.md#r2-p-s-dv-main-limited)。

## C04-F MIME 注册（任务分支限定验收）

任务分支的 `MimeTypes` 通过同一 `Container` 解析，`Application`/`ApplicationState` 与 `RegisterContext` 的异步入口共用应用作用域注册状态；Provider工厂覆盖、晚重绑及两App隔离由外部consumer验证。默认Unix `FileBinary` 和固定Symfony v8.0.0有序扩展表在该分支经代码与真实生成/模块流程限定验收；本次任务分支限定交付的源码现已叠入未提交main候选，组合门禁待执行，不得据此声称main新API已验收。原生Fileinfo、Windows默认检测/清理和完整MIME仍开放。见[分支证据](../memory/testing.md#c04-f-request文件家族分支限定验收)。

## 边界

只提供装配、配置、错误、命令与生命周期；不含路由、数据库或任何 driver。依赖：`axum-core`（`IntoResponse`）、`http`、`figment`、`serde`、`tokio`、`tracing`、`toml`。

## 当前实现

| 模块 | 内容 |
| --- | --- |
| `app` | `ApplicationDefinition { type Config; fn config_namespace(); fn providers(&Config) }`，默认命名空间 `APP_`；`Application` 提供 `state`、`config`、`figment`、`commands`、`extension`、`run_command`、显式 `shutdown`；`ApplicationState` 的 `resolve` 异步构造工厂，`get` 仅同步读取已就绪实例。每次命令建立独立 scope，正常返回显式等待清理，调用 future 取消时关闭 guard 交由 App 受管回收；命令可复用同一 App，顶层 CLI 退出时关闭资源。section 解码错误保留安全键路径，不回显值 |
| `container` | 生产唯一 `ContainerBuilder`/`Container`/`Resolver`/`Scope`：bind/singleton/scoped/instance、dyn 接口、单次构造、环检测、取消和根 reset。HTTP 请求 scope 贯穿 Inject/Auth/RateLimit/响应 body 轮询；命令、Job attempt 与 scheduler trigger 各用独立 scope。`Scope::run` 仅在当前 future 安装同容器 task-local，`Scope::spawn` 在同 scope/显式 ExecutionMetadata 下构造并执行受管任务；普通 `tokio::spawn` 不继承。关闭 App 封闭根生命周期并等待受管清理 |
| `provider` | `ProviderBuilder` 在同一容器中异步顺序执行 `ServiceProvider::register`、启动校验与 `boot`；App 后续公开 register/force 复用同一容器、原位贡献及资源 owner。Provider 依赖、普通同 ID 去重与 boot 顺序明确。资源由 `ResourceOwner` 管理，启动失败或取消后在存活 Tokio runtime 内有界回收；旧 Plugin 启动循环已删除 |
| `module` | `ModuleDescriptor::requires` 声明已编译模块间硬依赖；启用模块的依赖必须同时编译、启用，重复/自身/循环依赖拒绝。依赖拓扑优先于升序 `priority`，相同优先级保持应用编译清单顺序；模块 Provider register/boot 共享该结果，不自动改写运行启用状态 |
| `command` | `Command { name, about, accepts_args, run }`；额外参数默认拒绝；内置 `help`、`config show`（密钥遮蔽） |
| `config` | `src/config/` 已有 app/api/auth/cache/database/logger/mailer/server/signing/sms，queue 与 security 随对应 feature 开启；单一 `ConfigLoader` 合并类型默认值 → 应用根 `config/*.toml` 领域文件（按文件名排序）→ 应用工作目录 `.env` → 进程环境变量；不再加载 `default.toml` 与环境名 TOML。默认 `APP_`；Laravel 风格 `DB_*` / `REDIS_*` 分项连接变量、直接 URL 简写、中性 `SERVER_PORT` → `server.port`、`CACHE_STORE` → `cache.default`、`APP_NAME` / `APP_URL` / `APP_TIMEZONE` 均映射到领域配置（`app.timezone`默认UTC，启动时校验；HTTP日历使用最终Provider配置覆盖后的时区），应用命名空间变量优先（不重命名中性SERVER_PORT/CACHE_STORE，旧端口/缓存别名不回流）；`FrameworkConfig { app, server, api, log, i18n }`，其他能力通过独立 section 读取；数据库、认证、邮件旧模块路径由 re-export 保持；`NoConfig`、`load_dotenv`、`redact_secrets` |
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

## 具体类型构造

应用的 `configure_providers` 可向 `ProviderBuilder` 追加编译类描述。完整类目录在 Provider 准备前验证，普通 typed 解析可按声明构造实例及依赖，显式绑定优先。根示例与生成应用的 HTTP 注入均复用同一容器。公开入口支持类依赖和严格类型默认值。

具名服务、类符号和别名由原绑定目录解析。显式工厂与描述符在构造前验证声明输出，trait 投影复用具体对象和现有缓存。显式绑定在准入时复核别名边与绑定世代。

容器原子检查和安装条件绑定。非空调用帧按精确参数名覆盖声明、可由显式工厂读取，独立于普通共享缓存与历史。帧在失败、取消或成功后关闭读取，锁外逐值退役，再复核作用域后交付。

构造依赖使用独立消费类帧。上下文支持服务重定向、闭包、具名值、配置与有序变参，命中时跳过原抽象的共享缓存及普通解析历史。显式绑定快照保留原闭包，可按调用者作用域与显式参数执行，绕过原键的解析装饰与发布。

类名绑定沿唯一目录登记临时、应用共享和作用域共享工厂。同键包装直接构造编译类并读取 ambient 覆盖，异键包装使用本次工厂实参解析调用容器的具体键，并保留具体键的装饰、缓存和历史。两者的原工厂调用均跳过原抽象的解析发布。

编译类描述可声明 `Singleton` 或 `Scoped`，分别使用同一应用或作用域缓存。直接构造的共享实例不新增显式工厂条目，声明命中的 `Bind` 与 `BindWhen` 则登记对应共享工厂。显式共享登记优先，普通类工厂可采用具体类或抽象类上的声明。调用级参数和上下文覆盖绕过当前键的共享缓存。

弱标量在整批选参后按声明顺序转换。诊断、Stringable 与构造回调之间复核来源和作用域。实际 Provider 应用按错误掩码升级警告，并按实际环境、测试设置和 tracing 通道处理弃用，Provider 注册期即可报告。

容器解析回调支持全局与有序类型分组。共享对象在后置回调前可读取，回调成功才写普通历史。异键包装静默本层具体解析，依赖按正常流程派发。已编译同对象视图跨包装和保留对象的装饰器保全。

构造参数和类支持编译属性。普通构造保留显式覆盖与属性 null 回退，属性后置先于类装饰与缓存发布。SelfBuilding 使用独立方法实参，方法绑定可替换注入。同类重入退到普通构造，祖先共享尝试继续负责发布。

通用内置属性 `Config`、`Give`、`Tag` 复用现有配置解码、调用级参数帧和标签目录。`Give` 保留解析对象的编译视图，`Tagged` 固定取得时计数并在每轮首 poll 读取当前标签列表。

数据库与文件系统属性在当前 Resolver 上使用 `Databases` 和 `FilesystemManager`。`Database` 与 `DB` 的类型身份独立，返回的句柄共享现有池和磁盘 backend。数据库路由限定连接仍随管理器能力推进。

## 服务初始化诊断

容器工厂的初始化错误保留完整 Rust 服务类型名及原始错误链。日志的顶层和依赖服务名都限制为 128 字节，并转义控制字符。HTTP 仍输出稳定的 `service_initialization_failed` 与通用消息。

## 公共 Support 错误包

`openrust::support` 的 `MessageBag`、`ViewErrorBag` 不依赖 Session 或应用容器；核心装配不持有请求错误。`MessageBagContract` 与 `MessageProvider` 为公开扩展契约，命名包以原对象借用派发；`ViewErrorBag` 的默认代理只读取 `default`，缺失包是临时空值，不插入共享状态。

消息构造接受 String；`Collection`/`ArrayKey` 保留顺序与稀疏键。`MessageValue::Nested` 表达递归合并派生数组；外层 merge 重编号整数键，内层按固定 PHP 实际规则保留原稀疏键并追加。`add` 的数值字符串宽松去重与构造/unique 的字符串去重分别实现。嵌套值的非默认格式错误对应 Laravel `HandleExceptions` 将警告转异常的路径，裸 PHP 的警告后继续行为另列参考，不能混称相同。

`MessageValue` 序列化先转为具体值类型，避免递归叠加泛型有限数值检查器；通用 Collection 的非有限数拦截和整树预算仍保留。受控写入先校验候选完整树再发布，失败不替换原值或自定义包实例。用户入口见根 README 的消息包章节。D10-B 已把 Session `errors` 接入公开 codec 和请求绑定重定向并通过限定验收；请求局部视图自动共享仍按 todo 后继实施。

自定义包使用原 `MessageBagContract` 与 `MessageProvider`，不新设平行错误体系。Session Provider 发布默认 `ErrorBagCodecs`，内置 `MessageBag` 无需注册；可信业务 Provider 可通过 `RegisterContext::extend::<ErrorBagCodecs, _>` 装饰既有绑定，再以稳定 type ID、版本及编码/解码函数登记自定义包。编码和恢复回调在 Session 属性锁外运行；失败、未知版本或预算超限不能部分替换已存错误。固定 Laravel Store 对照中，PHP 序列化保留自定义对象，JSON 保存后恢复为基础消息包；Rust 为持久化自定义行为使用显式类型化 codec，非 PHP 字节格式兼容。

限定验收入口见[testing](../memory/testing.md#c04-d10-a-错误包限定子范围已验收)；不据此关闭完整校验器、重定向或视图错误共享。

## R2-T 严格类型配置读取（分支限定证据；main组合见下）

同一 `ConfigRepository` 增加 `string`、`integer`、`float`、`boolean`、`array`、`collection`，各有值默认 `_or` 与惰性默认 `_or_else`。仅缺失时回退，显式 null 与类型错误不回退；整数仅接 i64 范围的整型，浮点仅接 f64。错误说明路径/预期/实际类型或范围，不回显配置值。数组保留现有混合/稀疏键序；配置集合包装已存在的数组/对象，不重施请求输入 bag 的1024字段/64层预算，但 `Collection` 其他受限操作仍遵现有边界。唯一模块固定业务问候语与公开 consumer/生成 fixture 使用公开入口。仓储43、Collection4、模块1、consumer6及严格Clippy3/Rustdoc1已通过；fresh普通生成7+fixture1、真实HTTP与唯一生成包清理已通过；分支限定证据保留；同树main限定提交见下，不据此关闭完整Collection或L005.03。

## R2-R02 有序来源入口（限定验收通过）

`ConfigSources` 是不可变有序原值与Figment投影的同次输入，`ConfigLoader::sources()`供默认引导一次捕获后同时读取应用类型配置和启动Provider；仓储仍只有同一个State与版本。既有`ConfigRepository::new`、`ProviderBuilder::start`和`TestApp::builder`接收该输入，不增加平行启动API。旧Figment输入已丢失的声明顺序不能恢复；运行期set不重建已经解析的资源。

本簇已通过首轮142项、默认workspace853/全feature1642、双严格Clippy/docs、features/MSRV/deny、consumer5项及fresh生成原7+fixture1/MySQL2、真实TCP与资源收尾；命令、忽略项、失败历史及SHA见[本簇验收](../memory/testing.md#r2-r02-config-sources)。来源自然排序不等于完整Laravel配置发现：递归目录、文件名到配置namespace映射及类型表示差异仍为完整Config待办。当前实现不得据此宣称全量Config对应完成。契约见历史决策 086。

## R2-CC 配置缓存（分支限定证据；main组合见下）

- 固定 Laravel `ConfigCacheCommand` / `ConfigClearCommand` 与引导顺序，复用同一 Application、Provider、ConfigSources 和 ConfigRepository；公开命令为 `config:cache` / `config:clear`。默认 `bootstrap/cache/config.json` 是 Rust 数据表示适配，不执行 PHP 配置代码；默认 namespace 使用 `APP_CONFIG_CACHE` 选择相对或绝对文件路径。
- 首次控制台引导与普通应用一致，先加载缓存并执行 Provider register/boot，再派发维护命令；有效缓存屏蔽损坏来源，损坏缓存先报错，不借命令候选提前删除。`config:clear` 仅在实际命令执行时清除选定文件。`config:cache` 在实际命令执行时先清除本次选定文件，经同 Application fresh register/boot 取得最终有序配置；fresh 资源关闭成功后以同目录临时文件写入、同步、原子替换并回读核对。Unix 临时文件权限为0600；配置可能含秘密，不进错误或日志。失败不得保留半文件，其他路径缓存不因探测被删除。
- 引导先以外部环境快照检查缓存；未命中才独立解析 dotenv，再按选中路径检查一次；仅双重未命中才解析运行环境并读普通配置。dotenv 不写进程全局环境。命中缓存仍执行 Provider 其余职责与类型校验，同仓储 cached 标记在通用模块配置文件发现之前阻止重新读取/覆盖。
- 命令影响后续启动，不热改已运行 App 或已解析客户端；运行期仓储写入、持久业务 Settings、跨节点激活与完整配置发现仍按各自计划，不能由缓存子簇关闭。R03与R2-P现已分别经main限定组合交付；本句仅说明缓存子簇自身的行为边界，不将分支数字冒称main结果。
- 修后当前输入config77、lifecycle18、模块缓存屏障1、CLI模板1、双严格Clippy、默认workspace lib/tests851项、新server/两项模块真实脚本通过；强制重编CLI后的生成8项及真实重启通过。全特性lib/tests1638通过/68ignored、默认doctest22/34ignored与全特性24/40ignored、双warnings-denied docs及no-default/MSRV通过，离线deny按相同依赖图输入复用。两任务临时目录已回收；Cargo选择范围过宽的同名包缓存未删，归属复核缺口公开保留。限定分支提交见Git，main组合与完整Configuration仍未关闭。

## R2四簇 main 组合（限定验收）

R03、typed、CC 与 Signed 已在同一main提交整合；默认与全特性严格Clippy、workspace tests已实际通过（907/0/34 ignored、1698/0/108 ignored）。双docs/MSRV、no-default、deny及生成/MySQL证据以[组合记录](../memory/testing.md#r2-四簇-main-组合限定验收)最终receipt为准；五父本地提交 `e105dbfc58afc330b50e53b75f0b9e1424fa387a` 已创建。完整Config、动态Settings与签名URL复杂参数等缺口不因本次集成关闭。

## R2-Q/X 已验收子范围

R2-Q 分支 `850e1a4` 与 R2-X 分支 `d299ca4` 已完成各自限定验收；main 已完成实际组合验收，含双库、TCP和五类fresh生成应用；本地集成提交见Git，证据见[testing](../memory/testing.md#r2-qx-main-组合验收通过)。不代表完整配置/Container/Facade已对应。

- 配置读取复用唯一 `ConfigRepository`：`get` / `get_or` / `has` / `get_many` / `get_many_with_defaults` / `all`。完整字面键优先，缺失与显式 null 分开；批量读取固定同一快照。原始返回值可能含秘密，调用方负责控制输出。无 schema 的来源错误仅保留可信父键；有 schema 时仅保留声明路径，动态 Map 名和来源数组段不回显。见 历史决策 086。

- R2-R01 限定交付：同一仓储的 `set` / 有序 `set_many` 将原始值与 Figment 投影成对发布，`Application` / `ApplicationState` / Provider 共享句柄，`Application::section` 读取当前值；`Application::config` / `figment` 和既解析服务保留启动快照。启动 schema 屏障不等于禁止可信应用代码的运行期原始写入。已通过配置仓储默认/无默认各39项、App句柄1项及模块/CLI/consumer定向，双严格Clippy、工作区845/1632项、docs/features/MSRV/deny、fresh生成/MySQL8.4及真实TCP均有本簇证据；资源收尾通过，限定本地提交见Git；证据见测试基线。R2-R01的Figment入口不能恢复此前丢失的来源初始键序；R2-R02已对有序来源入口限定验收，既有Figment有损入口仍不能恢复丢失顺序，完整发现/命名映射继续见唯一todo。
- Container `extend` 复用现有工厂、作用域与发布流程，按顺序装饰原实例；示例 Provider 在首次解析前安装装饰器，并核 boot 与 HTTP 同一实例。可信 Session driver 装饰器通过 `SessionStoreHandle::store()` 取得原 store；该逃生口不能代替请求 Session 状态及其容量/删除检查。见 历史决策 085。
- `modules/example` 的固定配置键路由与 Container 装饰器是公开调用示范；任意 query 不扩张允许读取的配置键。完整配置修改/动态设置、Provider 自动发现及完整 Container 对应范围继续按唯一 todo 领取，不能因本子范围关闭。

## 约束

- 初始 Provider 清单由应用装配；启动后可经公开注册入口追加或 force 重注册 Provider。已服务 Router 不承诺热更新，需显式重建路由才观察新贡献。
- `ApplicationDefinition::Config` 不能是 `()`，用 `NoConfig`。
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


## R2-R03 递归配置发现与晚到 Provider 默认值（分支历史；main限定组合已交付）

`ConfigLoader` 递归发现领域 TOML，使用真实 basename 建立文件命名空间；隐藏文件和目录 symlink 不作为配置来源，同一命名空间在实际遍历中后项覆盖，再按完整名称自然序加载，不承诺跨文件系统的碰撞胜者。根领域 TOML 一次迁移为相对内容；模块配置沿用既有独立 overlay，新增 feature 文件展示公开读取，CLI 从既有模板拆分并保留格式化键与注释。配置仍由同一 `ConfigSources` / `ConfigRepository` 保存点路径与来源顺序；模块 overlay 保持独立。

晚到 Provider 默认值按浅层合并；`mergeableOptions` 中声明的数组按固定 PHP 边界浅合并。仅发现子文件时，文件前 base 加晚到 section defaults 重建父区段，再重放文件后环境层，保留环境优先、metadata/profile 与 raw/typed 投影；真实 PHP `after_final` 的剩余父 base 根覆盖不被误当缺键。无文件的程序来源沿原 join 路径，不新增第二个公开仓储。当前实证只覆盖指定 Laravel/PHP 场景，不代表完整 Config bootstrap 或全部值类型。

本分支最终定向配置 76、CLI 21、唯一模块 2、公开 consumer 6，严格 Clippy、Rustdoc 和 fmt 均通过；普通 fresh 生成 7 项及安装 consumer 1 项通过。无 auth MySQL 生成只编译、测试 0 项；另以 fresh MySQL 8.4 认证项目实际通过 2 项且无 skip，测试库集合前后一致。真实根 HTTP 单独验证 `GET /api/example/config-feature` 为 200、`enabled=true`、服务 exit 0；URL/TLS runner 另有 10/10 场景、两服务正常退出与来源/二进制稳定，不把后者冒充配置功能 HTTP 证明。执行者终端回报三个唯一生成包清理命令 exit 0，原始 stdout 未留存，不能独立复算逐命令日志 SHA；登记 16 路径 54.48 GB、磁盘空闲 8.782%，触软线未触硬线。本簇分支提交 `6482a38` 已通过五父提交 `e105dbf` 合入 main，并与 typed helpers、配置缓存及 Signed 完成限定组合门禁；分支工作区/分支已正常退役。完整 V1 Config、动态 Settings 与发布全后端矩阵仍开放。详见[本簇记录](../memory/testing.md#r2-r03-递归配置发现任务分支限定验收记录)。

当前配置依赖簇已限定验收：ConfigContext有序get/require、根键求值前覆盖、默认字段/单一来源及模块递归候选/CAS沿同一仓储。每个函数只求值一次，前向/自依赖拒绝，过期/跨线程上下文安全失败；typed/factory与缓存拒绝保留。根/生成清单server→app→auth/security，所属默认改变沿最终配置传播，运行期写入/晚merge仍为快照。配置119、模块投影2、公开consumer11、CLI27及完整适用门禁、fresh/缓存/MySQL/TCP通过，见[配置证据](../memory/testing.md#配置可编辑默认值与有序依赖限定验收)。完整配置、动态Settings和默认driver体验仍按唯一todo继续。

Calendar 公开纯数值格式化与 `U.u` UTC 解析复用同一内核，保留负秒字段及边界错误。见[数值日期限定验收](../memory/testing.md#数值日期格式化与解析限定验收)。

日期的隐式字符串转换由 `InputDate::to_string_value` 统一，局部设置优先于所属工厂。显式格式、JSON 与 SQL 使用各自规则。

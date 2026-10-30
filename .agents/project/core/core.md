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

## R2-Q/X 已验收子范围

R2-Q 分支 `850e1a4` 与 R2-X 分支 `d299ca4` 已完成各自限定验收；main 已完成实际组合验收，含双库、TCP和五类fresh生成应用；本地集成提交见Git，证据见[testing](../memory/testing.md#r2-qx-main-组合验收通过)。不代表完整配置/Container/Facade已对应。

- 配置读取复用唯一 `ConfigRepository`：`get` / `get_or` / `has` / `get_many` / `get_many_with_defaults` / `all`。完整字面键优先，缺失与显式 null 分开；批量读取固定同一快照。原始返回值可能含秘密，调用方负责控制输出。无 schema 的来源错误仅保留可信父键；有 schema 时仅保留声明路径，动态 Map 名和来源数组段不回显。见 [ADR-086](../decisions/adr-086-config-repository-reading.md)。
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

# OpenRust — Laravel + laravel-modules 的 Rust 实现

> Laravel + laravel-modules = OpenRust。以 Rust 语言 1:1 实现 Laravel 的框架能力与 laravel-modules 的模块化开发机制。

OpenRust 面向熟悉 Laravel 的开发者：目标是让开发者沿用 Laravel 官方文档中的概念和操作流程，用 Rust 编写应用。以固定版本的 Laravel 文档、源码和测试为实现基准，逐项对应服务容器、Service Provider、路由、数据库、认证、队列等能力；Goravel 作为跨语言实现参考。

模块化是框架正式能力：以 [nWidart/laravel-modules](https://github.com/nWidart/laravel-modules) 为对应上游，通过统一的 Service Provider 与服务容器接入 `modules/<模块名>/` 中的业务代码、路由、配置、迁移和资源。模块创建、注册、管理与测试纳入实施范围；`modules/example/` 是完整 CRUD、框架调用及扩展方式的唯一综合参考。以上为实现目标，当前模块能力与验收缺口按实施计划逐项记录。

**当前状态：全面重构进行中，尚未完成 Laravel 1:1 等价验收。** 每项能力须经过实现、行为测试、综合示例和适用的真实服务验收；已实现代码、目标能力及未验收项分别记录，详见[实施计划](.agents/project/plans/laravel-rebuild.md)。

1:1 指功能、可观察行为和使用流程的对应。保留 Rust 类型、所有权、trait、async 和 Cargo；必要语言适配须说明具体约束、用户可见差异与验证证据。用户明确选定的工程约定和框架增强单独记录，不能用目录相似、名称对应或编译成功代替行为等价，也不承诺直接执行 PHP 源码。

本仓库同时维护框架与应用入口，统一在 `main` 开发。根目录采用 Laravel 职责布局；`src/lib.rs` 提供框架库，`src/main.rs` 提供唯一默认 server。`modules/example/` 是目标唯一综合示例业务模块，目标包含完整 CRUD、框架能力使用及公开扩展参考，示例专用代码和资源保留在模块内。

## 当前实现与目标结构

最终业务目录为 `modules/<模块名>/`，唯一综合示例为 `modules/example/`；根 `app/` 保留通用应用级职责。根目录继续保留 Laravel 的 `bootstrap/`、`config/`、`database/`、`public/`、`resources/`、`routes/`、`storage/` 和 `tests/`，并保留 Rust 的 `src/`、`tools/` 和 Cargo。框架与应用同仓、同 workspace、只在 main 开发。

业务示例代码现位于 `modules/example/`，通过模块清单和文件激活状态接入统一 Provider 内核。模块管理命令、完整 CRUD 与全部能力示例仍在实施，当前闭环不代表 laravel-modules 全矩阵已完成。目标与边界见 [ADR-074](.agents/project/decisions/adr-074-laravel-modules-directory.md)。

## 设计目标

缓存使用 [`openrust::cache`](src/cache/README.md) 的统一 API，标准生成应用通过 `cache-redis` 使用 Redis；memory/file 与自定义 driver 均需显式选择。开发期 `openrust dev` 会复用或启动本地 Compose Redis。

- **约定清晰，不依赖魔法。** 目录、命名、路由、迁移和配置行为按 Laravel 对应契约实现，并以 Rust 类型表达依赖。应用通过 `Container` 与 `ServiceProvider` 统一装配，Facade 对应能力仍按实施计划逐项验收。
- **每项能力都有明确入口。** 例如，输入校验使用 `Validated<T>`，授权使用 `Policy`，后台任务使用 `Job`。
- **常用能力开箱可用，驱动可替换。** v1 计划提供数据库队列、Redis 缓存、本地存储、SMTP 邮件和可扩展短信（含 Twilio 适配器），并支持显式选择 memory/file 缓存及可选的 S3 兼容存储。框架计划支持单机和集群部署；驱动通过 Cargo feature 与配置选择。
- **生成代码就是普通代码。** `openrust generate` 生成的内容均为可读、可编辑、可测试的 Rust 代码。
- **默认可测试。** 外部服务能力提供 fake 或可控本地服务验收路径；Compose 中的 Mailpit 服务支持真实本地 SMTP 验证。业务测试无需付费 SMTP/SMS 账号。

OpenRust 基于 `tokio`、`axum`、`tower` 和 `SeaORM` 构建，采用明确的 Rust 类型与常规项目结构。

## 从根目录运行

```bash
cargo run
# 可选覆盖监听端口
APP_SERVER_PORT=4000 cargo run
```

根 `default-members = ["."]`、`default-run = "server"`，默认启动唯一 server。`app/` 保留应用级装配，`modules/` 放业务模块，`bootstrap/app.rs` 装配应用，`routes/` 注册路由；`config/`、`database/`、`public/`、`resources/`、`storage/` 和 `tests/` 都在根目录。

当前综合示例 [`modules/example`](modules/example/README.md) 是普通业务模块，展示如何通过公开 `openrust::...` API 接入业务；新业务统一位于 `modules/<业务>/`，由根 `modules/mod.rs`、bootstrap 和 routes 显式接入。它没有独立应用 manifest、启动入口或完整子应用骨架，也不需要额外 example feature。数据库、授权、队列等通过已有 Cargo feature 和根应用配置启用真实服务，缺必需依赖明确失败。

标准应用默认启用 Redis 缓存，需可用的 Redis 配置与服务；`/health` 与 `/ready` 提供探针。R2-M 模块清单、资源与迁移已有真实服务验收；管理写命令、完整发布和 CRUD 仍按当前计划推进。

```bash
cargo test -p openrust
cargo test --workspace
```

公开扩展及拓扑验收见 [consumers](tests/consumers/README.md)，完整门禁见 [贡献说明](CONTRIBUTING.md)。

## 编写业务与扩展

真实参考链是 `src/main.rs` → `bootstrap::Server::modules` → 文件激活清单 → `ExampleServiceProvider` → 模块 `routes::register` → `/api/example` handler；根 routes 只处理通用框架路由。当前实现由 `modules/mod.rs` 声明已编译模块，`bootstrap::Server::modules` 读取 `modules_statuses.json` 决定本次启动启用。先声明业务描述和 Provider，再用公开 `Routes` 定义模块路由；设置状态文件后运行根 `cargo run` 请求验证，无需修改框架 `src/` 核心。完整代码与操作见 [模块开发示例](modules/example/README.md#新建自己的业务模块)。

综合示例的硬交付包括完整实体 CRUD，以及当前所有已实现公开能力的实际使用和 Provider/driver 扩展参考；目标业务边界为 modules/，模块之外保留框架通用结构和代码；示例专有迁移/种子/资源/协议/生成脚本及测试 fixture 留模块内，根只提供通用结构和公开挂载，不因示例保留 build.rs 或 build 依赖。当前正在补齐，基础常量路由不是 CRUD，文档或其他 consumer 的代码不代表模块已覆盖；实际代码、接入条件与测试缺口见 [能力覆盖表](modules/example/README.md#当前能力覆盖与缺口)。

业务可以使用或新增第三方包，声明位置是根应用清单；普通 Rust 业务模块没有独立 Cargo 依赖边界，当前 lib/server 共享解析影响。已有 `serde`/`serde_json` 的实际导入、workspace 继承、新包位置和锁图检查见 [业务依赖教程](modules/example/README.md#使用业务自己的第三方依赖)。

## 生成用户应用

CLI 保留用户应用生成功能，模板唯一维护于 [CLI](tools/cli/README.md)。例如在临时目录验证：

```bash
cargo run -p cli -- new /tmp/my-application
cd /tmp/my-application
cargo check
cargo test
cargo run
```

生成应用可使用自己的名称、workspace 和 Cargo.lock，本地框架/tool 依赖须保持可达；不修改本仓 workspace，也不要求维护额外源码仓库。标准生成应用默认使用 Redis，运行前准备服务或显式选择其他 cache store。

未启用数据库、加密或 gRPC 的普通生成项目包含使用 [`TestApp`](src/core/README.md#测试应用) 的进程内 HTTP 测试；启用数据库认证后另生成隔离数据库的真实 TCP 认证测试与可替换 Mailer 的 `TestApp` 请求。其他生成选项当前不自动生成通用 `TestApp` 测试，应用仍可自行使用该公开 API 并显式提供所需配置。测试以同一 `Application` Provider 清单启动，不修改全局环境；进程内请求不替代真实服务验收。综合示例还演示业务依赖替换、两应用隔离和移除业务 Provider 后保留健康路由，见 [示例测试](modules/example/tests/registration.rs)。默认 API 错误状态为 `always_ok` envelope，需要断言实际 HTTP 404 时应在测试配置中选 `semantic`。

业务 JSON API 根路径为 `/api`，所有子路由统一以 `/api/` 开头并使用 JSON API 响应封装；`/health`、`/ready` 和可选 `/metrics` 保留为运维端点。详细约定见 [HTTP 指南](src/http/README.md)。

应用默认从当前工作目录的 `config/` 读取配置。`APP_SERVER_PORT=4000 cargo run` 或 `PORT=4000 cargo run` 可覆盖监听端口；若两者同时设置，`APP_SERVER_PORT` 优先。应用可通过 `Application::config_namespace()` 选择如 `SHOP_` 的变量前缀。框架代码位于根 openrust package，能力模块位于 `src/audit.rs`、`src/config/`、`src/core/`、`src/db/`、`src/http/`、`src/i18n.rs`、`src/mail/`、`src/sms.rs`，以及按 feature 启用的 auth、encryption、signing、queue 和 schedule。每个应用的 TOML/YAML 配置保存在其根目录的 `config/` 中；根唯一 server 同时承担综合示例与集成验收。

网站静态文件位于应用的公开目录，默认是工作目录下的 `public/`，目录中的文件按相对 URL 路径提供。生成项目默认启用 `view` feature 并用 `views/index.html` 渲染 `/`；`public/` 仍用于 CSS、图片等静态资源。`[server].public_dir` 可更换静态目录；`[server].root_target` 可设为 `view:index.html`、`file:/index.html`、`forward:/api` 或 `redirect:https://example.com/`。环境变量 `APP_SERVER_PUBLIC_DIR` 和 `APP_SERVER_ROOT_TARGET` 覆盖 TOML；`/api/*` 保留 JSON API，`/ws/*` 用于 WebSocket。视图配置与 API 示例见[视图指南](src/view/README.md)。
应用数据加密通过可选的 `encryption` feature 启用。注册 `openrust::security::EncryptionServiceProvider`，在 `[security.encryption]` 下配置 32 字节密钥环，并使用 `openrust key generate` 生成本地密钥材料。当前密钥用于加密新值；受控轮换期间，历史密钥用于解密旧值。密钥应保存在版本控制之外。

文件存储由可选 `storage-local` 与 `storage-s3` feature 提供，使用 `StorageServiceProvider` 装配 `Storage` 的 default/named disks。上传与下载都使用有界流，应用须先执行身份和资源授权；本地 disk 仅在当前节点可见。配置、安全边界与 `StorageBackend` 扩展方法见[文件存储指南](src/storage/README.md)。

可选 `data-exchange` feature 提供有界 CSV/XLSX 逐行解析、显式字段映射与类型转换，以及流式导出；解析器与业务持久化、事务和任务调度分离。CSV 默认防护电子表格公式注入，XLSX 拒绝公式与宏。使用示例和资源边界见[数据交换指南](src/data/exchange/README.md)。

`signing` feature 和 `openrust::signing::SignedLinks` 提供绑定用途并会过期的链接。它们使用 `[security.signed_links]` 下独立的密钥环；若需跨实例撤销单条链接，请添加 `SignedLinkRevocationMigration` 并配置共享数据库存储。详见[签名链接指南](src/signing/README.md)。

可选 `grpc` feature 使用 tonic 提供 gRPC server/client、具名共享 channel、标准 health、显式 reflection 与 Protobuf 运行时。`GrpcServiceProvider` 注册 `grpc serve` 命令但不会自动监听；服务端默认要求 TLS，明文必须显式设置 `allow_insecure_server = true`，只适用于本机开发或可信 TLS 终止代理。调用预算覆盖流式响应，连接、metadata、消息和全局并发均有上限，停机先报告 `NOT_SERVING` 并限时 drain。reflection 默认关闭，开启时必须显式注册 descriptor set 和服务白名单。具名 HTTPS 客户端绑定显式 CA 与可选客户端身份。CLI 可通过 `openrust new <name> --grpc` 为应用生成独立 proto、build.rs、typed Echo 服务及 Provider；生成编译使用 vendored protoc，不依赖开发机全局安装。生成应用先配置 TLS 证书和私钥，再用 `cargo run -- serve all` 启动真实 HTTP 与 TLS gRPC 两个监听端口，生成测试通过实际二进制验证 Echo、证书拒绝和信号停机。普通无参数启动仍只运行 HTTP。启用 `auth` feature 后，gRPC server 可验证 JWT/PAT Bearer；方法使用 `GrpcAuth::authorize` 复用 Policy 与 PAT abilities，Cookie Session 不用于 RPC。应用同时安装 `HttpServiceProvider`、`GrpcServiceProvider`、`HttpGrpcServiceProvider` 后可运行 `serve all`；两个 listener 先绑定，仅全部就绪时报告 ready，并共享信号、停机期限和后台任务收尾。该入口不代表多节点路由或集群验收已通过。真实 loopback 四类调用与运行治理见 [`tests/consumers/grpc/`](tests/consumers/grpc/README.md)。

显式审计记录通过 `openrust::audit::Audit` 与数据库 sink 或自定义 `AuditSink` 写入。请将 `DatabaseAuditMigration` 加入应用迁移器；既有数据库另加入 `DatabaseAuditTenantMigration`。默认不接收详情；如需记录，必须配置允许列表，敏感字段名会被脱敏。关键数据库变更可在同一事务中写入经过校验的审计记录，详见[审计模块指南](src/audit/README.md)。

`queue` feature 为 PostgreSQL 和 MySQL 提供持久化数据库任务派发、有界 worker 并发和由数据库协调的调度。将 `QueueMigration` 注册到应用迁移器，并在 `DatabaseServiceProvider` 后安装 `QueueServiceProvider`；使用 `register_job` 显式注册处理器。可通过 `schedule_interval`、`schedule_daily_at` 或 `schedule_cron` 注册任务。cron 使用五字段语法，默认时区为 `UTC`，也可指定 IANA 时区；春季不存在的本地时间跳过，秋季重复的本地分钟只派发一次。默认只派发当前分钟匹配项；`MissedRunPolicy::CatchUp { max_slots }` 可设置每次扫描最多补跑 1 到 100 项，后续 tick 继续处理剩余积压。表达式或时区变化会开启新的计划版本，不追溯补发该版本启用前的历史触发。使用集群部署时，各 scheduler 节点必须运行相同构建产物，以保持内置 IANA 时区数据一致。job 派发和时段记录在同一数据库事务提交，两个 scheduler 节点对同一时段只创建一个 job；任务本身仍至少执行一次，外部副作用须保持幂等。`schedule run --once` 扫描一次，`schedule run` 持续运行，`schedule list` 显示已注册项。`Queue::dispatch_unique` 根据队列范围内的键防止任务重叠重复，并返回活动任务 ID；任务完成或移入 `failed_jobs` 后释放该键。`dispatch_chain` 按顺序执行已注册任务，`dispatch_batch` 限制每组任务的并发数。每组是一个持久化父任务；重试可能再次执行已成功子任务的副作用，因此子任务处理器必须保持幂等。每组最多 128 项，批次并发数为 1 到 64。若任务创建需与业务数据共同提交或回滚，请在 `Db` 事务中派发。当前 worker 支持重试、失败任务保留与手动重试、进度、协作取消、单次尝试超时，以及进程故障后的租约恢复。事件支持有序进程内监听器和显式队列监听器；完整集群验收仍待完成。

可选 `queue-redis` feature 提供 Redis job driver，默认仍为数据库队列。设置 `QUEUE_DRIVER=redis` 与 `redis_namespace` 即可启用；Redis 队列与缓存共用 `REDIS_HOST`、`REDIS_USERNAME`、`REDIS_PASSWORD`、`REDIS_PORT`、`REDIS_DB` 连接项，无需单独配置队列 URL。缺少 feature、配置错误或 Redis 不可达都会失败，不会切换为数据库或内存 driver。Redis-only `QueueServiceProvider` 无需注册 `DatabaseServiceProvider`，但当前数据库事务型 scheduler 与 Redis 派发无法原子提交，因此启用 Redis 时不能注册 schedule。Redis 支持普通/延迟/唯一派发、lease、进度、退避重试和失败任务人工重试；事务派发与任务组明确不支持。语义为至少一次，handler 必须幂等；Redis 磁盘持久性和故障切换取决于运维配置，不承诺 exactly-once 或故障切换零丢失。详见 [`modules/example/`](modules/example/README.md)。
外部 driver 可通过 `JobRegistry` 复用 typed Job 执行，并用 `JobContext::with_progress_reporter` 声明自己的进度存储；独立的 [`QueueDriver` 示例](tests/consumers/queue-custom-driver/README.md)展示派发、执行和不支持事务时的显式错误。

统一 Provider 装配使用 `QueueServiceProvider` 与容器中的 `QueueContributions`：所有 register 完成后读取最终绑定，任务目录在克隆间共享，后续 boot 注册的任务对已经构造的 driver 可见。`QueueContributions::extend_validated` 将 creator 与最终配置校验器作为同一条目替换；仅 `extend` 时，自定义选项的合法性由实现者负责，框架不会推断第三方配置。最终校验同时覆盖顶层回退与所有具名连接；内置 Redis URL/命名空间校验不建立网络连接。worker 停机先封闭整个 manager，所有已构造连接收到停止请求，尚未构造的名称不能在停止后首次初始化；显式重启使用新代缓存，旧代在途结果不能重新发布。


邮件、短信也支持应用实现自定义 driver，并由应用自己的 `ServiceProvider` 注册标准 `Mailer` / `SmsSender` 服务；请看[独立通信 driver consumer](tests/consumers/communication-drivers/README.md)。

```rust
#[cfg(feature = "queue")]
fn main() -> openrust::Result<()> {
use openrust::queue::QueueServiceProvider;
use openrust::schedule::{CronOptions, MissedRunPolicy};

let queue = QueueServiceProvider::new();
queue.contributions().schedule_cron(
    "reports.daily",
    "reports",
    "reports.daily.v1",
    &serde_json::json!({}),
    "30 9 * * 1-5",
    CronOptions {
        time_zone: "Asia/Shanghai".to_owned(),
        missed_runs: MissedRunPolicy::CatchUp { max_slots: 3 },
    },
)?;
Ok(())
}
#[cfg(not(feature = "queue"))]
fn main() {}
```

可选 `realtime` feature 提供在线 WebSocket 与 SSE 会话；跨节点路由须显式启用 `realtime-redis` 并配置 Redis URL，未配置共享 transport 时只支持本进程连接，Redis 故障会返回错误且不会本地静默回退。应用路由负责认证、Origin 校验和频道授权，并为连接提供授权有效期；Session/PAT 撤销需显式调用 `Realtime::revoke_user`。SSE 不提供离线回放，Redis Pub/Sub 不保证远端确认或 exactly-once。连接、消息和缓冲区有配置上限，停机时关闭在线连接。真实双进程示例与验收见 [`tests/consumers/realtime/`](tests/consumers/realtime/README.md)。

## 类型化事件

`EventsServiceProvider` 按声明顺序注册应用事件监听器。`Events::dispatch` 按顺序等待匹配的监听器；遇到首个错误时立即返回，不再调用后续监听器。该派发仅在当前进程内执行，不会持久化或广播。启用 `queue` feature 后，`listen_queued` 可将事件映射为已注册的 `Job`；在业务事务中调用 `dispatch_in_transaction`，可将任务与业务变更原子地持久化。worker 仍需显式启动，任务处理器必须能够应对至少一次重试。

```rust,ignore
let mut events = openrust::events::EventsServiceProvider::new();
events.listen_queued::<UserRegistered, WelcomeEmailJob, _>("mail", |event| {
    Ok(WelcomeEmailJob { user_id: event.user_id })
});
let mut builder = openrust::core::ProviderBuilder::new();
builder.provider(openrust::queue::QueueServiceProvider::new()).provider(events);
let app = openrust::App::from_provider_runtime(builder.start(config).await?)?;
let events = app.state().resolve::<openrust::events::Events>().await?;

db.transaction(|transaction| {
    let events = std::sync::Arc::clone(&events);
    Box::pin(async move {
        create_user(transaction).await?;
        events
            .dispatch_in_transaction(transaction, &UserRegistered { user_id: 42 })
            .await?;
        Ok(())
    })
})
.await?;
```

同时还需使用 `QueueServiceProvider` 注册 `WelcomeEmailJob`。如需在事务提交后尽力执行进程内观察，请仅在事务成功返回后调用 `Events::dispatch`；监听器错误无法撤销已提交的事务。不得将密码、token、验证码或原始请求放入事件/任务载荷。

## 跨协议校验与执行作用域

`openrust::validation::validate(value)` 把命令、Job 和其他入口的 DTO 校验转换为与 HTTP `Validated<Json<T>>` 相同的字段路径和 `Error::Validation`；校验 trait 与 derive 宏可由 `openrust::validation::Validate` 使用。

`ExecutionContext::current()` 可读取当前 request ID、locale、剩余时间预算和协作取消状态。HTTP 中间件、`App::run_command` 及 typed Job 执行入口会创建各自的作用域；`JobRegistry::execute` 与数据库 worker 会强制执行 job deadline 和合作取消。直接调用 `JobRegistry::execute` 会在当前异步调用中等待 handler 完成，但不派发、不持久化，也不提供自动重试、事务派发或任务组。存在 `I18n` 服务时，Job 使用派发时的 locale 或应用默认 locale。`tokio::spawn` 不会继承 task-local，需显式传递取消句柄或建立子作用域；子 deadline 不超过父级，子任务超时只取消自身。命令和 Job 可显式调用 `validate` 并在副作用前检查取消/预算。

队列只持久化白名单 `ExecutionMetadata`：最多 128 字节的安全 ASCII request ID 和规范化 locale。身份、token、Cookie、header、deadline 与取消句柄不会进入 payload。自定义 `QueueDriver` 若要接受当前调用中的关联 metadata，须实现 metadata-aware 派发方法；默认实现会在不支持时返回 `queue_driver_metadata_unsupported`。request ID 只用于日志关联，不构成 W3C trace parent 或 exactly-once 保证。

HTTP 的 `/health` 用于存活检查，`/ready` 在 listener 已绑定且未停机时返回 200，停机或未启动时返回 503。收到 SIGINT/SIGTERM 后，服务器先切换为 not-ready，再按 `server.shutdown_timeout_secs` 等待在途请求；到期会合作取消请求并返回 `http_shutdown_timeout`，进程以失败退出。取消不会撤销已提交的副作用。

## v1 源码应用使用路径

生成器创建拥有自己 Cargo.lock 的独立应用 workspace，使用可解析的本地框架和工具依赖；不修改框架源 workspace：

```bash
git clone https://github.com/hongyukeji/openrust.git
cd openrust
cargo run -p cli -- new blog --db postgres
cd blog
cargo run
```

生成的应用应直接依赖仓库内框架源码，通过 `cargo check` 和 `cargo test`，无需发布内部 package 或添加手工 registry patch。依赖编排、自动迁移与 OpenAPI 等保证仍按 v1 计划逐项实现和验收。
## 请求语言

生成的应用默认安装 `I18nServiceProvider`。`[i18n] default_locale` 和 `fallback_locale` 用于选择默认语言和最终资源回退；`Accept-Language` 用于选择受支持的请求语言。框架内置英文和简体中文 HTTP 消息。应用可将 JSON 资源合并至 `MessageCatalog`，在处理器中使用 `RequestLocale`，或调用 `I18n::with_locale` 显式设置任务作用域。普通 `{name}` 文本保持兼容，并支持有限的 CLDR `plural` 类别与 `select` 分支。`I18n::format_number` 接受精确十进制字符串；日期时间格式化接收调用方已转换到明确业务时区的 Chrono 值。详见 [`I18n` API](src/i18n.rs)。

```rust
use openrust::http::{Inject, Json};
use openrust::i18n::{I18n, MessageArgs, RequestLocale};

async fn greeting(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "app.greeting", &args))
}
```

启动时可将应用 JSON 对象加载至 `MessageCatalog`，例如在指定语言下配置 `{"app.greeting":"Hello, {name}!"}`，或用 `plural` / `select` 对象声明有限分支；两者都要求 `other` 回退项。在请求之外编写邮件或 SMS 时，也应显式使用同一语言设置。数字、日期结果仅用于展示，不应写回 API 机器字段或替换 UTC 存储时间。

## 文档

当前用户指南位于各模块和 package 的 README 中。[Agent 计划](.agents/project/plans/README.md)记录剩余工作；M6 计划建立独立的公开文档站。规划记录与用户文档分开维护。

开发与验证请阅读[贡献指南](CONTRIBUTING.md)，并遵守其中的[构建空间管理规范](CONTRIBUTING.md#构建空间管理)，在任务收尾时检查并清理临时构建产物。

## 许可证

您可自行选择采用 [Apache License, Version 2.0](LICENSE-APACHE) 或 [MIT license](LICENSE-MIT) 授权。

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

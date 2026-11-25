# Horizon

## 安装 {#installation}

启用根应用的 `horizon` feature，即可使用可选 Redis 队列监督扩展 `openrust-horizon` 与授权运维面板。`HorizonServiceProvider` 通过包发现自动注册；独立使用包的应用显式注册此 Provider。加载 Provider 不启动 Worker，也不连接 Redis。

在应用 `Cargo.toml` 的 `[features].default` 列表中加入 `horizon`，保留已有功能，然后使用源码入口：

```sh
./artisan vendor:publish --tag=horizon-config
./artisan horizon
```

发布配置后重新编译应用使其生效。重复发布保留已有文件，`--force` 才覆盖。配置缓存见[配置](../getting-started/configuration.md)。

## 配置 {#configuration}

复用 `database.redis` 中的单机 Redis 连接和 `queue.connections` 中的 Redis 队列连接。`horizon.use` 选择元数据连接，默认 `default`；`horizon.prefix` 读取 `HORIZON_PREFIX`，默认将 `APP_NAME` 转为下划线 slug 后追加 `_horizon:`；共享 Redis 的不同环境应使用独立前缀。不支持 Redis Cluster。原生[队列](../digging-deeper/queues.md)仍按自身契约支持其他传输。

已有部署若使用此前的 `horizon:` 元数据前缀，应显式设置 `HORIZON_PREFIX=horizon:`，让相关 Worker 继续使用同一前缀。排空旧 Master 后再切换前缀；新前缀拥有独立的控制状态与历史记录。

`horizon.defaults` 按监督器名称配置；`horizon.environments` 选择第一条匹配 `app.env` 的模式，支持 `*`。未匹配时不启动监督器，`max_processes: 0` 禁用对应监督器。环境选项与默认值递归合并，数组按索引覆盖。默认模板在 `production` 使用 10 个进程预算，在 `local` 使用 3 个。

监督器支持 `connection`、`queue`（字符串或字符串数组）、`balance`、`auto_scaling_strategy`、`min_processes`、`max_processes`、`balance_max_shift`、`balance_cooldown`、`tries`、`timeout`、`sleep`、`memory`、`max_jobs`、`max_time`、`workers_name`、`backoff`、`force`、`rest`、`nice` 和 `directory`。`backoff` 接受整数、逗号分隔字符串或整数数组。`tries: 0` 表示不限尝试次数，`sleep: 0` 不等待，`timeout: 0` 关闭任务超时。`nice` 非零时使用 Unix 进程优先级接口，工作进程继承监督器优先级；其他平台明确拒绝此选项。配置支持 snake_case 与 `processes` 别名；Rust 的选项结构使用 `max_tries`，配置仍可使用 `tries`。

`balance: "auto"` 按预计清空耗时（`time`）、待处理数量（`size`）或 `log1p` 数量比例（`log`）计算整个 `max_processes` 预算中的浮点目标。按目标从小到大调整进程池，目标向上取整，并应用 `min_processes`、剩余预算和 `balance_max_shift`。未知耗时按零处理，非空但总耗时为零的队列以最大预算为目标。`simple` 均分预算；`false` 或 `"off"` 使用一个优先队列池，按待处理数量在最小与最大值间缩放。`balance_cooldown` 控制间隔；计算时包含退出中的进程，各 Master 独立管理预算。

Worker 的 `timeout` 必须短于队列的 `retry_after`，默认分别为 60 和 90 秒。Worker 使用应用自身可执行文件、基础目录、环境、处理器和配置。`max_jobs` 与 `max_time` 触发进程轮换，异常退出后自动补位。使用操作系统进程管理器托管 Master。SIGTERM／SIGINT 及终止请求等待当前任务完成，宽限期为配置的超时加五秒，之后强制结束。

进程监督器面向 Unix 部署。基于信号的监督器控制和孤儿进程检查需要 Unix 进程工具。在其他平台上使用受支持的元数据命令，并在运行 Worker 前验证部署行为。

## 面板授权 {#authorization}

包 Provider 自动注册面板路由。在应用 Provider 或路由装配中注册授权回调。每次请求（包括指标接口）都须通过回调，否则拒绝访问。回调使用应用已有的登录会话与授权策略。

```rust
use openrust::http::{HttpError, HttpFuture, Request};
use openrust_horizon::Horizon;

fn view_horizon(request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    // 替换为应用已认证运维人员的授权策略。
    let _ = request;
    Box::pin(async { Ok(false) })
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::auth(view_horizon).expect("Horizon provider registered");
    Horizon::routes()
}
```

`HORIZON_PATH` 控制 `horizon.path`（默认 `horizon`），`HORIZON_DOMAIN` 控制可选的 `horizon.domain`；`horizon.middleware` 默认 `["web"]`。设置 `horizon.routes=false` 关闭自动注册。`Horizon::routes()` 使用这些配置；显式 `routes_at("/operations/horizon")` 优先于自动注册，每个应用只注册一次。现有路由组继续应用自身前缀与中间件。面板每五秒刷新，设置严格的内容安全策略并禁用响应缓存，支持队列负载、Master 与监督器、待处理／完成／静默／失败任务、任务载荷与异常、监控标签、指标快照和批次。失败任务及批次失败任务通过已配置的原生失败任务提供器重试。

设置 `HORIZON_NAME` 可在面板的页面标题和主标题显示部署名称。

从 `openrust_horizon` 导入事件类型，为 `JobPending::NAME`、`JobPushed::NAME` 和 `JobReserved::NAME` 注册同步监听器。三个事件分别在 Redis 写入前、成功写入后和保留任务后派发，提供连接、队列、投递 ID 和原生载荷。`JobReserved` 的 `job` 快照还提供尝试次数。使用载荷 UUID 关联同一任务的不同投递。

同一 Redis 连接上的任务链推进时，先为后继任务派发 `JobPending`。Redis 在一次原子操作中入队后继任务并删除前一个保留，确认后依次派发后继任务的 `JobPushed` 和前一任务的 `JobDeleted`。保留失效可能在 `JobPending` 后阻止推进，使用 `JobPushed` 确认新的投递。

Redis 将保留任务放回延迟队列后派发 `JobReleased::NAME`。`delay` 是 Rust 的 `Duration`，任务详情以 `release_delay_ms` 提供该值。Redis 确认删除保留任务后派发 `JobDeleted::NAME`，任务链推进时也会派发。它提供任务快照、失败标记和保留后的实际时长。

每批 Redis 迁移完成后、Worker 保留任务前，Horizon 派发 `JobsMigrated::NAME`。每次轮询先处理到期的延迟任务，再处理过期的保留任务。监听器收到连接、队列和任务快照，空批次的任务列表为空。

`JobFailed::NAME` 提供失败 Redis 投递的连接、保留快照和异常文本。Worker 失败时，事件在 Redis 确认和失败回调执行后派发，监听器可读取失败任务详情和标签。直接调用 `Job::fail` 时，也会在 Redis 确认后派发。

原生 Redis 投递延后到数据库事务提交时，带延迟的任务会在发起投递时计算 Horizon 元数据，即时任务会在提交回调中计算。两者的 `JobPending` 和 `JobPushed` 都在提交回调中围绕 Redis 写入派发。事务回滚时，队列和面板都不会出现新投递。

通过 `JobPayload::from_payload` 读取 Redis 任务上的 Horizon 元数据。Horizon 在写入前计算标签和静默状态，将结果、以十进制秒表示的投递时间和入队入口确定的 `job_type()` 一并保存。类型序列化为 `job`、`broadcast`、`event`、`mail` 或 `notification`。Horizon 重试沿用已保存的标签，清除静默标志，通过 `retry_of()` 提供原投递 ID，新投递的观测类型为 `job`。原投递保存了重试截止时间和投递时间时，新投递沿用相同的重试时长。旧载荷可能没有 Horizon 元数据，也可能返回默认类型 `job`。

在 `horizon.silenced` 中配置广播事件、监听器、邮件或通知的底层 Rust 完整类型名。普通任务也可使用已注册的队列任务名称。通过代码进行静默时，为类型实现 `openrust_horizon::Silenced`，并在应用启动时调用 `Horizon::silence::<YourType>()`。注册只在当前应用内生效。

监督器队列名称遵循原生 Redis 传输的 255 字节上限，保留空格、Unicode 字符和冒号。监督器队列列表以逗号分隔。以 `:delayed`、`:reserved` 或 `:notify` 结尾的名称由传输保留。

长等待告警在邮件主题、短信正文和 Slack 详情中显示部署名称。`Horizon::route_slack_notifications_to` 接受传入 Webhook 地址或频道 ID。频道 ID 使用 Slack Bot API，需配置 `services.slack.notifications.bot_user_oauth_token`。

使用 `Horizon::route_slack_webhook_notifications_to(url, channel)` 可在传入 Webhook 的载荷中指定频道。Slack 应用的传入 Webhook 固定投递到安装时选定的频道。需要按请求指定目标频道时，使用 Bot API 频道 ID。

如需自定义长等待告警，在应用服务提供者中绑定 `dyn LongWaitNotificationFactory`。工厂接收 `LongWaitDetected` 事件并返回 `LongWaitDetectedNotification`。返回值的 `signature()` 决定五分钟去重窗口的归属。

为 `SupervisorLooped::NAME` 注册同步监听器，可在 Supervisor 持久化心跳后观察其状态。`SupervisorLooped` 提供 Supervisor 名称和暂停状态，Horizon 为该事件注册 `MonitorWaitTimes`。

Horizon 在补位 Worker 启动前派发 `WorkerProcessRestarting`，提供监督器、队列、可用的原进程 PID 和退出码。

每个 Worker 席位首次尝试启动后，非零退出和进程创建失败受一秒冷却限制。已退出的 Worker 在冷却到期后补位时，Horizon 在 `WorkerProcessRestarting` 之后派发 `UnableToLaunchProcess`，并开始一分钟冷却。正常退出可在冷却期间补位。操作系统无法创建进程时，原监督器保留席位，后续 `UnableToLaunchProcess` 的 PID 和退出码为空。

匹配的环境部署计划准备好后，Horizon 派发 `MasterSupervisorDeployed`，提供 Master 名称、环境和待启动的监督器。

Master 持久化心跳后也会派发 `MasterSupervisorLooped::NAME`，事件提供 Master 名称和暂停状态。Horizon 注册 `ExpireSupervisors`，清理两个进程索引中的过期条目。

进程超过内存上限时，会在以退出码 `12` 关闭前派发 `MasterSupervisorOutOfMemory` 或 `SupervisorOutOfMemory`。事件提供进程名称、实测常驻内存及上限，内存单位为 MiB。

Horizon 在对应循环事件上注册 `MonitorMasterSupervisorMemory` 和 `MonitorSupervisorMemory`。每轮共享一次检查，即使其他同步监听器提前终止派发，内存保护仍会执行。

Master 也会清理过期的任务索引。最近、待处理、已完成、静默和最近失败索引每分钟检查一次。失败和监控索引各自按配置的保留分钟数除以 12 后向下取整，最短间隔为一分钟。

暂停、继续、终止、监控变更和重试均须逐次通过授权回调。浏览器对写操作发送 `X-Horizon-Request: 1`，直接调用接口也须携带该请求头；拒绝跨站浏览器写操作。为整个路由组应用已有的认证与中间件。面板授权包含查看任务数据和执行运维控制的权限。

## 命令 {#commands}

```sh
./artisan horizon:status
./artisan horizon:status --json
./artisan horizon:pause
./artisan horizon:continue
./artisan horizon:terminate
./artisan horizon:failed
./artisan horizon:retry JOB_ID
./artisan horizon:forget JOB_ID
./artisan horizon:forget --all
./artisan horizon:list
./artisan horizon:supervisors
./artisan horizon:supervisor-status supervisor-1
./artisan horizon:pause-supervisor supervisor-1
./artisan horizon:continue-supervisor supervisor-1
./artisan horizon:snapshot
./artisan horizon:clear-metrics
./artisan horizon:clear redis --queue=default
./artisan horizon:purge
```

`horizon:status` 在运行时返回 0、暂停时返回 1、无存活 Master 时返回 2。暂停阻止领取新任务，已运行任务可完成；继续恢复领取；终止使当前 Master 优雅退出，新启动的 Master 使用新代次。监督器命令操作具名监督器，控制命令作用于共享元数据前缀。

Horizon 任务 ID 标识独立的 Redis 投递，由队列推送和面板返回。`horizon:retry` 与 `horizon:forget` 使用此 ID。重试获得新 ID，原失败监控记录保留并记录重试结果。原生 `queue:retry` 和失败任务存储使用载荷 UUID，由 Horizon 内部转换；批次重试也会将这些 UUID 解析为当前失败投递。

按[队列](../digging-deeper/queues.md)配置持久失败存储，重试复用原生领取与重新派发机制。`horizon:forget` 删除监控失败记录与原生失败记录，`--all` 分页处理全部 Horizon 失败记录。`horizon:clear` 清空指定队列和待处理监控记录，`horizon:purge` 处理孤立进程。通过[任务调度](../digging-deeper/scheduling.md)定期执行 `horizon:snapshot` 保存指标历史；`horizon:clear-metrics` 清理指标并保留任务记录。

`horizon:listen` 是开发命令，每 500 毫秒轮询一次 `horizon.watch` 文件，即使省略 `--poll` 也是如此。文件变化后重启 Horizon；源码应用包含 `Cargo.toml` 时通过 Cargo 启动，以重新编译 Rust 源码和配置改动。监控时跳过符号链接。

默认监控应用、启动、配置、数据库、路由和视图目录，以及 `public/` 和 `resources/` 下的 Rust 文件和根目录的 `main.rs`、`build.rs`、`Cargo.toml`、`Cargo.lock`、`.env`。

## 任务标签 {#tags}

在应用 Provider 中、Horizon 注册后调用 `Horizon::tag(fn(&Payload) -> Vec<String>)`。回调对每个被观察的载荷生成标签，忽略空字符串，重复标签保留首次出现顺序。Rust 任务通过这些回调显式提供标签，不通过反射自动发现载荷中的模型字段。

`Tags::for_model(&model)` 将 Rust 模型类型与整数、无符号整数或字符串主键组合为标签；缺少主键或主键类型不支持时返回错误。任务携带模型身份时，可将返回的字符串加入回调生成的标签。各应用注册的标签回调彼此隔离。面板的监控操作决定哪些标签保留监控任务。

## 指标与限制 {#metrics}

`Repository::recent(1..=100)`、`metrics(connection, queue)`、`masters()`、`pause()`、`r#continue()` 和 `terminate()` 提供相同的存储操作。`SupervisorOptions::from_config` 验证配置，`AutoScaler::number_of_workers_per_queue` 计算浮点目标，`scale_pool` 计算逐池调整，`MasterSupervisor` 管理子进程生命周期。

Redis 队列事件记录 ID、任务名、连接、队列、尝试次数、状态、耗时与 Redis 服务端时间。授权后的任务详情还提供序列化载荷、标签和异常文本。`Repository::recent(1..=100)` 保持纯元数据投影，近期索引最多 1000 个任务身份；`retention`（60–86400 秒，默认 3600）控制近期元数据和滚动 60 分钟尝试指标。

`horizon.trim` 按分钟控制任务列表保留时间：`recent`、`pending`、`completed` 默认为 60，`recent_failed`、`failed`、`monitored` 默认为 10080。任务列表每页返回 50 条。`horizon.metrics.trim_snapshots.job` 与 `.queue` 默认各保留 24 个快照。平均耗时和吞吐量只计算成功完成的任务，失败、释放和等待重试不参与计算。重复结束事件在下一次处理事件前忽略；手动重试重置尝试次数时开始新的观测。

`Repository::jobs()`、`tags()`、`metrics_repository()`、`master_supervisors()`、`supervisor_repository()`、`processes()` 与 `workload()` 提供独立仓储操作。监控标签按元数据前缀保存。每个指标的快照写入与当前计数重置原子执行。Master 心跳 15 秒后过期，监督器心跳 30 秒后过期。元数据服务错误会暂停领取新任务并结束 Master，由操作系统进程管理器恢复。监控记录会过期，不作为持久执行账本。

负载接口（默认 `/horizon/api/workload`）对单队列返回 `split_queues: null`。分组队列的 `split_queues` 以队列名为键。去重后的键按顺序为 `0`、`1` 等连续整数时，编码为 JSON 数组。重复队列在积压数和分项中只计一次，每次出现都计入整组等待时间估算。

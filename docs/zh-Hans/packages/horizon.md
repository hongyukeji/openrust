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

在事务内通过 failover 投递时，由首个接受任务的连接处理其事务配置。接受前失败可以尝试下一个连接。一旦所选连接登记了提交回调，该回调的错误就交给事务调用方处理。

通过 `JobPayload::from_payload` 读取 Redis 任务上的 Horizon 元数据。Horizon 在写入前计算标签和静默状态，将结果、以十进制秒表示的投递时间和入队入口确定的 `job_type()` 一并保存。类型序列化为 `job`、`broadcast`、`event`、`mail` 或 `notification`。Horizon 重试沿用已保存的标签，清除静默标志，通过 `retry_of()` 提供原投递 ID，新投递的观测类型为 `job`。原投递保存了重试截止时间和投递时间时，新投递沿用相同的重试时长。旧载荷可能没有 Horizon 元数据，也可能返回默认类型 `job`。

入队前可用 `JobPayload::with_tags(payload, tags)` 设置预置标签。Horizon 将应用回调生成的标签追加到预置标签之后，保留预置标签内部和两组标签之间的重复项。需要读取或编辑原始任务 JSON 时使用 `JobPayload::new`。`offset_set` 和 `offset_unset` 只修改 `decoded()`，`set` 合并字段并更新 `value()` 中的完整 JSON。`JobPayload::from_payload` 返回原生 Redis 的类型化快照 `JobPayloadMetadata`。

`value_mut()` 和 `decoded_mut()` 可分别修改原始文本和解码数据。`JobPayload::new` 接受 JSON 对象或数组。`set` 按合并顺序重新编号整数键，连续且按顺序排列的整数键编码为 JSON 数组。

在 `horizon.silenced` 中配置广播事件、监听器、邮件或通知的底层 Rust 完整类型名。普通任务也可使用已注册的队列任务名称。通过代码进行静默时，为类型实现 `openrust_horizon::Silenced`，并在应用启动时调用 `Horizon::silence::<YourType>()`。注册只在当前应用内生效。

监督器队列名称遵循原生 Redis 传输的 255 字节上限，保留空格、Unicode 字符和冒号。监督器队列列表以逗号分隔。以 `:delayed`、`:reserved` 或 `:notify` 结尾的名称由传输保留。

长等待告警在邮件主题、短信正文和 Slack 详情中显示部署名称。`Horizon::route_slack_notifications_to` 接受传入 Webhook 地址或频道 ID。频道 ID 使用 Slack Bot API，需配置 `services.slack.notifications.bot_user_oauth_token`。

使用 `Horizon::route_slack_webhook_notifications_to(url, channel)` 可在传入 Webhook 的载荷中指定频道。Slack 应用的传入 Webhook 固定投递到安装时选定的频道。需要按请求指定目标频道时，使用 Bot API 频道 ID。

如需自定义长等待告警，在应用服务提供者中绑定 `dyn LongWaitNotificationFactory`。工厂接收 `LongWaitDetected` 事件并返回 `LongWaitDetectedNotification`。返回值的 `signature()` 决定五分钟去重窗口的归属。

为 `SupervisorLooped::NAME` 注册同步监听器，可在 Supervisor 持久化心跳后观察其状态。`SupervisorLooped` 提供 Supervisor 名称和暂停状态，Horizon 为该事件注册 `MonitorWaitTimes`。

Horizon 在补位 Worker 启动前派发 `WorkerProcessRestarting`，提供监督器、队列、可用的原进程 PID 和退出码。

每个 Worker 席位首次尝试启动后，非零退出和进程创建失败受一秒冷却限制。已退出的 Worker 在冷却到期后补位时，Horizon 在 `WorkerProcessRestarting` 之后派发 `UnableToLaunchProcess`，并开始一分钟冷却。正常退出可在冷却期间补位。操作系统无法创建进程时，原监督器保留席位，后续 `UnableToLaunchProcess` 的 PID 和退出码为空。

匹配的环境部署计划准备好后，Horizon 派发 `MasterSupervisorDeployed`，提供 Master 名称、环境和待启动的监督器。

应用可显式派发携带 Master 名称的 `MasterSupervisorReviving`，或携带共享 `SupervisorProcess` 的 `SupervisorProcessRestarting`。

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

`horizon:status` 根据所有活跃 Master 的心跳返回状态。运行时返回 0，任一 Master 暂停时返回 1，无存活 Master 时返回 2。请在每台需要控制的主机上运行 `horizon:pause`、`horizon:continue` 和 `horizon:terminate`。暂停阻止领取新任务，已运行任务可以完成。继续恢复领取。终止关闭本机 Master，并通过应用缓存发布原生队列重启通知。启用 `fast_termination` 时，可传入 `--wait` 等待 Worker 排空。具名监督器命令也只选当前主机。升级后重启旧 Master 和监督器，以便在 Redis 发布主机信息。

如果旧版本在 Redis 留下共享暂停状态，可在已授权的面板选择「继续」，解除该元数据前缀上的暂停。

在 Windows MSVC 上，`horizon:terminate` 会请求本机进程树正常关停。应用的可执行文件在构建时预留 16 MiB 栈空间。使用自定义 Windows MSVC 应用入口运行 Horizon 时，请在链接时使用 `/STACK:16777216`。

Master 名称由主机名转换出的短名和进程内固定的四字符令牌组成。要自定义短名，可在应用 Provider 的 `boot` 方法调用 `openrust_horizon::MasterSupervisor::determine_name_using(|| "orders.primary".into())?`。启动 Horizon 和运行控制命令时使用同一短名。自定义短名可以为空，也可以包含 Unicode 文本、空格和标点符号。短名不能包含控制字符。在 shell 中向 Artisan 传递含空格的监督器全名时，请使用引号。

要向运行中的 Master 添加监督器，请从应用容器解析 `RedisHorizonCommandQueue`。通过 `MasterSupervisor::command_queue_for(Some(master_name))` 获取目标队列，将 `AddSupervisor::NAME` 和 `SupervisorOptions::to_array()` 生成的选项推入队列。监督器名称由 Master 全名、`:` 和监督器名称组成。处于暂停状态的 Master 会在继续运行后启动新监督器。

`ProvisioningPlan::get(master_name)` 读取应用的部署计划。部署按配置顺序选择首个匹配的环境，将监督器选项与默认值合并，并跳过 `max_processes` 为零的监督器。Artisan 启动时会应用该计划。要向运行中的 Master 应用计划，可调用 `plan.deploy(environment, &app).await`。

要从应用代码控制运行中的监督器，通过 `RedisHorizonCommandQueue` 将 `openrust_horizon::supervisor_commands` 中的命令发往监督器全名。`SCALE` 的 `scale` 参数指定 Worker 总数，`BALANCE` 接收各队列的 Worker 数，`TERMINATE` 可通过 `status` 指定退出码。`RESTART` 会更换 Worker 并恢复暂停的监督器。

在应用 Provider 中，可用 `instance_named` 按队列中的命令名绑定 `dyn MasterSupervisorCommandHandler` 或 `dyn SupervisorCommandHandler`。绑定可处理自定义命令，也可覆盖同名内置命令。处理器接收当前进程的监控实例和队列传入的 JSON 选项。Supervisor 的处理器返回 `Some(status)` 时，当前监督器会排空并以该状态码退出。

要替换结构化命令队列，在应用容器绑定 `dyn HorizonCommandQueue`，并在所有 Horizon 进程加载同一应用配置。发送命令时，通过 Horizon `Repository::command_target(name).await` 解析目标，再调用队列的 `push`。后端须同时使用 `target.name()` 和 `target.scope()` 区分队列，并按入队顺序一次领取一批。命令目标对应当前进程的所有权，请在每次发布前重新解析。独立运行的 Supervisor 不带所有权代次。

要更换运行中 Master 的所有监督器及其 Worker，把空选项的 `RESTART` 发送到 `MasterSupervisor::command_queue_for(Some(master_name))`。`MasterSupervisor::restart().await` 按实例名称提交同一请求。Master 从暂停中恢复。默认情况下，各监督器会等待正在执行的任务完成后再更换。

`horizon` 实时将受管进程的输出写到 Artisan。从应用代码运行 `MasterSupervisor` 或 `Supervisor` 时，可用 `handle_output_using` 接收直接子进程的输出。回调接收 `openrust::process::OutputStream` 和字节片段，可区分该子进程的 stdout 与 stderr。`horizon:supervisor` 会把 Worker 的两个流都写入自己的 stdout，因此 Master 收到合并后的输出。

直接使用 `WorkerProcess` 或 `SupervisorProcess` 时，通过 `handle_output_using` 注册回调，把已配置的 `PendingProcess` 传给 `start`。通过 `take_process` 取出当前句柄并等待或停止进程。后续启动会继续使用已注册的回调，直到调用方替换它。

`WorkerProcess::monitor` 检查当前进程，并在冷却允许时启动传入的命令。两个包装器都提供 `terminate` 请求关停，以及 `stop` 在宽限期内等待退出。独立包装器的 `pause` 与 `continue_working` 使用 Unix 信号。Windows 受管 Horizon 命令通过共享进程状态暂停和继续。自定义 Windows 子进程需要监听关停管道才能正常排空。通过 `SupervisorProcess::terminate_with_status` 可向配置的命令队列发送带退出码的终止请求。

独立管理包装器时，调用 `SupervisorProcess::monitor` 并传入目标 Master 的完整名称。通过 `dont_restart_on` 可指定冷却期结束后终止监控的退出码。默认清单为 `[0, 2, 13]`，退出码 13 始终立即结束监控。其他退出码在冷却期结束后检查 Supervisor 心跳，心跳缺失或仍属于已退出进程时，按原选项发送 `AddSupervisor` 命令。

Horizon 任务 ID 标识独立的 Redis 投递，由队列推送和面板返回。`horizon:retry` 与 `horizon:forget` 使用此 ID。重试获得新 ID，原失败监控记录保留并记录重试结果。原生 `queue:retry` 和失败任务存储使用载荷 UUID，由 Horizon 内部转换；批次重试也会将这些 UUID 解析为当前失败投递。

按[队列](../digging-deeper/queues.md)配置持久失败存储，重试复用原生领取与重新派发机制。`horizon:forget` 删除监控失败记录与原生失败记录，`--all` 分页处理全部 Horizon 失败记录。`horizon:clear` 清空指定队列和待处理监控记录，`horizon:purge` 处理孤立进程。通过[任务调度](../digging-deeper/scheduling.md)定期执行 `horizon:snapshot` 保存指标历史；`horizon:clear-metrics` 清理指标并保留任务记录。

`horizon:listen` 是开发命令，每 500 毫秒轮询一次 `horizon.watch` 文件，即使省略 `--poll` 也是如此。文件变化后重启 Horizon；源码应用包含 `Cargo.toml` 时通过 Cargo 启动，以重新编译 Rust 源码和配置改动。监控时跳过符号链接。

默认监控应用、启动、配置、数据库、路由和视图目录，以及 `public/` 和 `resources/` 下的 Rust 文件和根目录的 `main.rs`、`build.rs`、`Cargo.toml`、`Cargo.lock`、`.env`。

## 任务标签 {#tags}

在应用 Provider 中，Horizon 注册后可用 `Horizon::tag_for::<T>(callback)` 为底层任务、邮件、通知或事件类型指定标签。用 `Horizon::tag_models_for::<T>(callback)` 注册回退回调，它会在该类型的显式标签为空时执行。排队监听器与其事件各自使用这套规则，先取监听器标签，再取事件标签，重复标签保留首次出现顺序。两个回调都可以读取载荷中的序列化事件。

`Tags::for_model(&model)` 将 Rust 模型类型与标量主键组合为标签。缺少主键、主键为 null 或 false 时后缀为空，true 的后缀为 `1`。结构化值或无效 UTF-8 字节键返回错误。`Tags::for_models` 按框架保存的模型状态去重。不同模型生成的同名标签按原顺序保留。应用自定义字段或独立加载的关联影响等价判定时，使用 `Tags::for_models_by` 提供比较规则。Rust 应用通过回调显式声明载荷中的模型字段。

`Horizon::tag(callback)` 可为应用中的每个载荷补充标签，忽略空字符串。各应用的回调彼此隔离。面板的监控操作决定哪些标签保留监控任务。

## 指标与限制 {#metrics}

`Repository::recent(1..=100)`、`metrics(connection, queue)`、`masters()`、`pause()`、`r#continue()` 和 `terminate()` 提供相同的存储操作。`SupervisorOptions::from_config` 验证配置，`AutoScaler::number_of_workers_per_queue` 计算浮点目标，`scale_pool` 计算逐池调整，`MasterSupervisor` 管理子进程生命周期。

Redis 队列事件记录 ID、任务名、连接、队列、尝试次数、状态、耗时与 Redis 服务端时间。授权后的任务详情还提供序列化载荷、标签和异常文本。`Repository::recent(1..=100)` 保持纯元数据投影，近期索引最多 1000 个任务身份；`retention`（60–86400 秒，默认 3600）控制近期元数据和滚动 60 分钟尝试指标。

`horizon.trim` 按分钟控制任务列表保留时间：`recent`、`pending`、`completed` 默认为 60，`recent_failed`、`failed`、`monitored` 默认为 10080。任务列表每页返回 50 条。`horizon.metrics.trim_snapshots.job` 与 `.queue` 默认各保留 24 个快照。平均耗时和吞吐量只计算成功完成的任务，失败、释放和等待重试不参与计算。重复结束事件在下一次处理事件前忽略；手动重试重置尝试次数时开始新的观测。

`Repository::jobs()`、`tags()`、`metrics_repository()`、`master_supervisors()`、`supervisor_repository()`、`processes()` 与 `workload()` 提供独立仓储操作。监控标签按元数据前缀保存。每个指标的快照写入与当前计数重置原子执行。Master 心跳 15 秒后过期，监督器心跳 30 秒后过期。元数据服务错误会暂停领取新任务并结束 Master，由操作系统进程管理器恢复。监控记录会过期，不作为持久执行账本。

负载接口（默认 `/horizon/api/workload`）对单队列返回 `split_queues: null`。分组队列的 `split_queues` 以队列名为键。去重后的键按顺序为 `0`、`1` 等连续整数时，编码为 JSON 数组。重复队列在积压数和分项中只计一次，每次出现都计入整组等待时间估算。

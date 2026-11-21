# Horizon

## 安装 {#installation}

启用根应用的 `horizon` feature，即可使用可选 Redis 队列监督扩展 `openrust-horizon` 与只读面板。`HorizonServiceProvider` 通过包发现自动注册；独立使用包的应用显式注册此 Provider。加载 Provider 不启动 Worker，也不连接 Redis。

在应用 `Cargo.toml` 的 `[features].default` 列表中加入 `horizon`，保留已有功能，然后使用源码入口：

```sh
./artisan vendor:publish --tag=horizon-config
./artisan horizon
```

发布配置后重新编译应用使其生效。重复发布保留已有文件，`--force` 才覆盖。配置缓存见[配置](../getting-started/configuration.md)。

## 配置 {#configuration}

复用 `database.redis` 中的单机 Redis 连接和 `queue.connections` 中的 Redis 队列连接。`horizon.use` 选择元数据连接，默认 `default`；`horizon.prefix` 默认 `horizon:`，须按应用和环境隔离。不支持 Redis Cluster。原生[队列](../digging-deeper/queues.md)仍按自身契约支持其他传输。

`horizon.defaults` 按监督器名称配置；`horizon.environments` 选择第一条匹配 `app.env` 的模式，支持 `*`。未匹配时不启动监督器，`maxProcesses: 0` 禁用对应监督器。环境选项与默认值递归合并，数组按索引覆盖。默认模板在 `production` 使用 10 个进程预算，在 `local` 使用 3 个。

监督器支持 `connection`、`queue`（字符串或字符串数组）、`balance`、`autoScalingStrategy`、`minProcesses`、`maxProcesses`、`balanceMaxShift`、`balanceCooldown`、`tries`、`timeout`、`sleep`、`memory`、`maxJobs`、`maxTime`、`workersName`、`backoff`、`force`、`rest`、`nice` 和 `directory`。`backoff` 接受整数、逗号分隔字符串或整数数组。`tries: 0` 表示不限尝试次数，`sleep: 0` 不等待，`timeout: 0` 关闭任务超时。`nice` 非零时需要 Unix 的 `nice` 命令；其他平台明确失败。配置支持 snake_case 与 `processes` 别名；Rust 的选项结构使用 `maxTries`，配置仍可使用 `tries`。

`balance: "auto"` 按预计清空耗时（`time`）、待处理数量（`size`）或 `log1p` 数量比例（`log`）计算整个 `maxProcesses` 预算中的浮点目标。按目标从小到大调整进程池，目标向上取整，并应用 `minProcesses`、剩余预算和 `balanceMaxShift`。未知耗时按零处理，非空但总耗时为零的队列以最大预算为目标。`simple` 均分预算；`false` 或 `"off"` 使用一个优先队列池，按待处理数量在最小与最大值间缩放。`balanceCooldown` 控制间隔；计算时包含退出中的进程，各 Master 独立管理预算。

Worker 的 `timeout` 必须短于队列的 `retry_after`，默认分别为 60 和 90 秒。Worker 使用应用自身可执行文件、基础目录、环境、处理器和配置。`maxJobs` 与 `maxTime` 触发进程轮换，异常退出后自动补位。使用操作系统进程管理器托管 Master。SIGTERM／SIGINT 及终止请求等待当前任务完成，宽限期为配置的超时加五秒，之后强制结束。

## 面板授权 {#authorization}

在应用路由装配中注册授权回调及路由。每次请求（包括指标接口）都须通过回调，否则拒绝访问。回调使用应用已有的登录会话与授权策略。

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

`Horizon::routes()` 注册 `GET /horizon` 和 `GET /horizon/api/stats`；使用 `routesAt("/operations/horizon")` 指定其他静态路径。现有路由组继续应用自身前缀与中间件。面板每五秒刷新，设置严格的内容安全策略并禁用响应缓存，展示队列积压、监督器、近期任务和聚合指标。控制操作使用 Artisan。

## 命令 {#commands}

```sh
./artisan horizon:status
./artisan horizon:status --json
./artisan horizon:pause
./artisan horizon:continue
./artisan horizon:terminate
./artisan horizon:failed
./artisan horizon:retry JOB_UUID
./artisan horizon:forget JOB_UUID
```

没有存活 Master 时，`horizon:status` 退出码为 1。暂停阻止 Horizon Worker 领取新任务，已运行任务可完成；继续恢复领取。终止请求使该请求前启动的 Master 优雅退出，新启动的 Master 使用新代次。控制命令作用于共享元数据前缀。

重试和删除先检查失败任务属于已配置的 Horizon 连接与队列，再调用原生失败任务提供器。按[队列](../digging-deeper/queues.md)配置持久失败存储。近期失败列表仅保留脱敏元数据，原生失败记录删除后，该列表仍可能保留到过期；不展示载荷或异常文本。

## 指标与限制 {#metrics}

`Repository::recent(1..=100)`、`metrics(connection, queue)`、`masters()`、`pause()`、`r#continue()` 和 `terminate()` 提供相同的存储操作。`SupervisorOptions::fromConfig` 验证配置，`AutoScaler::numberOfWorkersPerQueue` 计算浮点目标，`scalePool` 计算逐池调整，`MasterSupervisor` 管理子进程生命周期。

已配置 Redis 队列的任务事件记录 ID、规范化任务名、连接、队列、尝试次数、状态、毫秒耗时与 Redis 服务端时间，不记录载荷、异常消息及凭据。指标聚合最近 60 个分钟桶，同时受 `retention` 限制（60–86400 秒，默认 3600）。近期索引最多保存 1000 个任务身份，每次最多读取 100 个；同一任务展示最近一次尝试。重复结束事件在下一次处理事件前忽略；手动重试即使重置尝试次数，也开始新的观测。这些数据用于监控，不是持久执行账本。

心跳在 15 秒后过期。元数据服务错误会暂停 Horizon 领取新任务并结束 Master，由进程管理器恢复。HTTP 授权由应用负责。本包不提供 Laravel Horizon 的标签、通知发送、长等待告警、快照命令或可写 Web 控制。

平均耗时只计算成功完成的任务，失败、释放与等待重试的尝试不参与计算。

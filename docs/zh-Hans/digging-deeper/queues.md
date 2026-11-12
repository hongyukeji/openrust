# 队列

## 生成任务类型 {#generating-job-classes}

Artisan 在 `app/jobs` 下生成任务模块。队列任务携带可序列化数据并注册 Worker 所需的处理器。用 `--sync` 生成的任务在等待其 `dispatch` Future 时直接执行。`--test` 生成配套功能测试，`--force` 覆盖现有文件。

```sh
./artisan make:job SendReport --test
./artisan make:job LocalReport --sync
```

在应用中声明生成的模块，在服务提供者的 `boot` 方法调用 `register_jobs`。示例先执行本地任务，再派发队列任务。

```rust
use crate::app::jobs::{local_report::LocalReport, send_report::SendReport};
use openrust::queue::{DispatchResult, QueueResult};

fn register_jobs() -> QueueResult<()> {
    SendReport::register()
}

async fn dispatch_reports() -> QueueResult<DispatchResult> {
    LocalReport.dispatch().await?;
    SendReport.dispatch().await
}
```

通过 `--batched` 生成可加入批次的任务。同时传入 `--sync` 时，Artisan 选择批次队列模板。

```sh
./artisan make:job BatchReport --batched --test
```

声明模块，并在服务提供者的 `boot` 方法调用 `BatchReport::register()`。生成的处理器跳过已取消的批次，`handle` 接收可选的批次 ID。

```rust
use crate::app::jobs::batch_report::BatchReport;
use openrust::{bus::Batch, queue::QueueResult, support::facades::Bus};

async fn dispatch_batch() -> QueueResult<Batch> {
    Bus::batch([BatchReport.payload()?, BatchReport.payload()?])
        .name("monthly reports")
        .onQueue("reports")
        .dispatch()
        .await
}
```

## 创建与派发任务

应用启动时通过 `QueueManager::register` 或 `Queue::register` 登记稳定的任务名。`JobHandler` 从 `Payload` 读取 JSON 数据，并通过 `JobContext` 获取连接、队列和尝试次数。使用 `Queue::dispatch` 将任务发送到已配置的连接。

```rust
use openrust::{
    queue::{DispatchOptions, DispatchResult, Payload, QueueResult},
    support::facades::Queue,
};

async fn dispatch_email(user_id: u64, connection: &str) -> QueueResult<DispatchResult> {
    let payload = Payload::new("emails.send", serde_json::json!({ "user_id": user_id }))?;
    let options = DispatchOptions {
        connection: Some(connection.into()),
        ..Default::default()
    };
    Queue::dispatch(payload, options, None).await
}
```

## 连接与预约

应用骨架默认使用 `QUEUE_CONNECTION=database`。运行 `./artisan migrate` 创建 `jobs` 表。`sync` 立即执行并返回处理结果，`database` 和 `redis` 将任务入队。连接可以配置默认队列和 `retry_after`。用 `Delay::For` 设置相对时长，或用 `Delay::Until` 指定绝对时间。

持久化连接通过队列与任务契约提供 `pop`、`delete` 和 `release`。预约过期后，任务可以重新领取，旧句柄无法确认或释放新预约。Redis 的 `block_for` 单位为秒，`null` 立即返回，`0` 持续等待任务。

## 在数据库事务中派发

将活动事务作为 `Queue::dispatch` 的最后一个参数。连接开启 `after_commit` 时，任务在最外层提交后派发。可以通过 `DispatchOptions.after_commit` 覆盖连接配置。回滚会丢弃待派发任务。立即派发时，同一连接的数据库队列在事务内写入，sync 处理器可用 `JobContext::transaction` 访问当前事务。

## 运行队列 Worker

运行 `./artisan queue:work database --stop-when-empty` 处理待执行任务并退出。`--once` 尝试一个任务。用 `--tries`、`--backoff` 和 `--timeout` 配置 Worker，任务可通过 `JobOptions` 覆盖。收到 SIGTERM 时，Worker 停止领取新任务并完成当前尝试。

## 任务批次 {#job-batching}

运行 `./artisan migrate` 创建批次账本和投递记录。在应用启动时注册任务处理器，再通过 `Bus::batch` 一起派发这些任务。

```rust
use openrust::{
    bus::Batch,
    queue::{Payload, QueueResult},
    support::facades::Bus,
};

async fn dispatch_reports() -> QueueResult<Batch> {
    Bus::batch([
        Payload::new("reports.generate", "summary")?,
        Payload::new("reports.generate", "detail")?,
    ])
    .name("monthly reports")
    .onQueue("batch")
    .dispatch()
    .await
}
```

运行 `./artisan queue:work database --queue=batch --stop-when-empty` 处理批次。用 `Bus::findBatch(&id)` 读取当前状态，用 `Bus::cancelBatch(&id)` 主动取消批次。任务最终失败时默认取消批次。通过 `allowFailures()` 允许批次继续，通过 `SkipIfBatchCancelled` 跳过已取消批次的任务。失败任务在重试成功前仍计入待处理数。

批次派发目前使用与批次仓库同一连接的数据库队列。批次创建和所有入队操作在同一事务提交。自定义批次表时，还需建立同名加 `_jobs` 后缀的投递记录表。Redis 派发和回调待接入。

### 清理批次 {#pruning-batches}

定期运行此命令可删除已完成超过 24 小时的批次。通过 `--hours` 修改已完成批次的保留期限。还可以通过 `--unfinished` 和 `--cancelled` 分别指定未完成和已取消批次的保留小时数。这两个选项都从批次创建时间起算。命令分别输出每条规则删除的记录数，批次的投递记录会随批次一起清理。

```sh
./artisan queue:prune-batches
./artisan queue:prune-batches --hours=48 --unfinished=72 --cancelled=36
```

## 任务中间件 {#job-middleware}

通过 `JobHandler::middleware` 返回中间件。用 `JobNext::run` 继续执行，或用 `JobContext::release` 安排下一次尝试。`WithoutOverlapping` 使用配置的缓存锁，可延迟释放冲突任务。未完成的锁会在租期结束时过期。

生成的中间件位于 `app/jobs/middleware`，其处理器将载荷与上下文交给下一步。

```sh
./artisan make:job-middleware GuardReport --test
```

声明中间件模块，并在生成任务的 `impl JobHandler` 中加入以下方法以启用中间件。

```rust
fn middleware(&self) -> Vec<std::sync::Arc<dyn openrust::queue::middleware::JobMiddleware>> {
    vec![std::sync::Arc::new(crate::app::jobs::middleware::guard_report::GuardReport)]
}
```

## 失败任务

运行 `./artisan migrate` 创建 `failed_jobs`。用 `./artisan queue:failed --json` 查看失败记录，用 `./artisan queue:retry <uuid>` 将任务重新送到原连接和队列。`queue:forget` 和 `queue:flush` 可移除失败记录。超时会取消当前 Rust future，任务需要自行管理它在该 future 外启动的工作。

## 队列监听器

通过 `ListenerRegistry::listenQueued::<YourEvent>` 登记可序列化的事件及稳定的监听器 ID。Worker 在其应用内解析监听器。监听器的 `after_commit` 可将入队推迟到事务提交后。

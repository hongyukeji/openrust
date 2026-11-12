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

`--batched` 依赖队列批次运行时，目前会在写入文件前报错退出。

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

运行 `./artisan migrate` 创建 `job_batches` 账本。`DatabaseBatchRepository` 保存批次计数、失败任务 ID、取消时间和完成时间。展示当前进度时，从写连接读取批次。

```rust
use openrust::{
    bus::DatabaseBatchRepository,
    database::Connection,
    queue::QueueResult,
};

async fn batch_progress(connection: Connection, id: &str) -> QueueResult<Option<u32>> {
    let batches = DatabaseBatchRepository::new(connection, "job_batches")?;
    Ok(batches.find(id).await?.map(|batch| batch.progress()))
}
```

当前已提供 SQL 账本。批次派发和 Worker 自动记账依赖批次运行时，接通后才能使用 `make:job --batched`。

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

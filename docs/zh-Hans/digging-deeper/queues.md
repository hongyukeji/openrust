# 队列

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

## 任务中间件

通过 `JobHandler::middleware` 返回中间件。用 `JobNext::run` 继续执行，或用 `JobContext::release` 安排下一次尝试。`WithoutOverlapping` 使用配置的缓存锁，可延迟释放冲突任务。未完成的锁会在租期结束时过期。

## 失败任务

运行 `./artisan migrate` 创建 `failed_jobs`。用 `./artisan queue:failed --json` 查看失败记录，用 `./artisan queue:retry <uuid>` 将任务重新送到原连接和队列。`queue:forget` 和 `queue:flush` 可移除失败记录。超时会取消当前 Rust future，任务需要自行管理它在该 future 外启动的工作。

## 队列监听器

通过 `ListenerRegistry::listenQueued::<YourEvent>` 登记可序列化的事件及稳定的监听器 ID。Worker 在其应用内解析监听器。监听器的 `after_commit` 可将入队推迟到事务提交后。

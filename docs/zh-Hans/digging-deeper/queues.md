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

## 任务链 {#job-chaining}

将已注册的任务载荷传给 `Bus::chain`，在构建器上调用 `dispatch().await` 派发首个任务。使用数据库或 Redis 驱动时，Worker 在同一次存储操作中确认当前任务成功并将下一项入队。重试或释放时，任务链停在当前任务。任务最终失败时，任务链停止。sync 驱动在当前进程内依次执行。

```rust
use openrust::{
    bus::PendingChain,
    queue::Payload,
    support::facades::Bus,
};

fn report_chain(prepare: Payload, generate: Payload, publish: Payload) -> PendingChain {
    Bus::chain([prepare, generate, publish]).onQueue("reports")
}
```

用 `onConnection` 和 `onQueue` 选择任务链的投递位置。`delay` 作用于首个任务。派发前可用构建器的 `prepend` 和 `append` 调整顺序。

在服务提供者的 `boot` 方法中用 `Bus::registerChainCallback` 按名称注册失败回调，再用 `catch("reports.failed")` 关联。Worker 在自己的应用内解析回调，并传入失败的任务载荷和队列错误。任务应保证副作用幂等，因为 Worker 丢失预约后可能再次执行任务。任务链回调没有独立的重试记录。

### 任务链连接和队列 {#chain-connection-queue}

用 `PendingChain::onConnection` 和 `onQueue` 设置任务链的默认目标。在任务载荷上调用同名方法可覆盖该任务的目标。后继任务仍使用链的默认值，除非它也设置了覆盖。后继任务的延迟从前项成功时开始计算。首任务的延迟优先于链构建器的延迟。

```rust
use openrust::queue::{Delay, Payload};

fn route_job(job: Payload, connection: &str, queue: &str, delay: Delay) -> Payload {
    job.onConnection(connection).onQueue(queue).delay(delay)
}
```

同连接内换队列时，Worker 原子交接。跨持久连接时，源 Worker 会在自己的队列中保存转发任务。保持源队列有 Worker 运行，目标恢复后可重试投递。在每个数据库目标执行 `./artisan migrate`，创建 `job_chain_deliveries`。SQL 和 Redis 保留投递凭据，重放时据此避免重复入队。持久队列中的任务链不能切换到 sync 后继。

### 向当前任务链添加任务 {#adding-jobs-to-the-chain}

在任务处理器内，使用 `JobContext::prependToChain` 让新任务紧接当前任务执行，使用 `appendToChain` 将新任务放到现有链尾。处理器成功后修改生效。失败或释放会丢弃本次尝试的修改。

```rust
use openrust::queue::{JobContext, Payload, QueueResult};

fn edit_chain(context: &mut JobContext<'_>, before: Payload, after: Payload) -> QueueResult<()> {
    context.prependToChain(before)?;
    context.appendToChain(after)
}
```

每次调用接收一个任务载荷或待派发批次。插入批次遵循[任务链中的批次](#batches-within-chains)规则。若当前链已属于某个批次，可插入任务载荷来扩展该批次。新任务共享批次 ID，Worker 在记录当前任务成功时一并保存新增数量和后继。使用批次链的运行时修改前，请执行 `./artisan migrate`。

### 按条件派发任务链 {#conditional-chain-dispatch}

条件为真时可用 `dispatchIf` 派发，条件为假时可用 `dispatchUnless` 派发。两者都接收布尔值或返回布尔值的闭包。等待派发 Future 时，闭包只执行一次。

```rust
use openrust::{
    bus::PendingChain,
    queue::{DispatchResult, QueueResult},
};

async fn dispatch_chain_if(
    chain: PendingChain,
    enabled: bool,
) -> QueueResult<Option<DispatchResult>> {
    chain.dispatchIf(enabled).await
}
```

跳过时返回 `None`，派发成功时返回 `Some(DispatchResult)`。跳过的任务链不会解析队列连接或执行任务。

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

数据库队列与批次仓库须使用同一 SQL 连接，批次和入队操作在同一事务提交。派发到 Redis 时，在 `dispatch()` 前调用 `onConnection("redis")`。

### 批次中的任务链 {#chains-and-batches}

在待派发批次上用 `addChain` 加入一组顺序执行的任务。它对应 Laravel `Bus::batch` 里的嵌套数组。所有成员共用批次 ID、连接和队列。批次统计每个成员，初次入队只放入各链的首任务。多个 Worker 可以并行处理不同的任务链。

```rust
use openrust::{bus::PendingBatch, queue::Payload, support::facades::Bus};

fn batched_chains(left: [Payload; 2], right: [Payload; 2]) -> PendingBatch {
    Bus::batch([]).addChain(left).addChain(right)
}
```

在构建器上调用 `dispatch().await` 派发。批次派发后，可用 `Batch::addChain(...).await` 追加任务链。任务最终失败会停止所在的链。即使批次允许失败，链中尚未执行的任务仍保持待处理状态。重试失败任务可恢复该链。

### 任务链中的批次 {#batches-within-chains}

用 `appendBatch` 或 `prependBatch` 将待派发批次加入任务链。Worker 执行到该步骤时派发批次。批次默认继承任务链的连接和队列，也可以单独配置。两个阶段都支持 Redis 以及与批次仓库共用连接的数据库队列。

```rust
use openrust::{
    bus::{PendingBatch, PendingChain},
    queue::{Payload, QueueResult},
    support::facades::Bus,
};

fn release_chain(first: Payload, batch: PendingBatch, last: Payload) -> QueueResult<PendingChain> {
    Ok(Bus::chain([first]).appendBatch(batch)?.append(last))
}
```

批次进入最终回调阶段后，外层任务链继续。批次允许失败时，仍可进入该阶段并继续。取消批次会停止外层链。任务失败导致批次取消时，外层链的 `catch` 回调会收到失败的批次任务和错误。

执行 `./artisan migrate` 创建续链表。发布失败时，续链任务保留在 SQL 中，执行 `./artisan queue:publish-batches` 可恢复发布。

### 向批次追加任务 {#adding-jobs-to-batches}

可在任务处理器或其他应用入口调用 `Batch::add`。它沿用批次的队列和连接选项，并在批次仍存在时返回更新后的快照。追加会增加任务总数和待处理数，并清除完成时间。已取消的批次保留取消状态。每个追加任务获得新的队列 UUID。

```rust
use openrust::{
    bus::Batch,
    queue::{Payload, QueueResult},
    support::facades::Bus,
};

async fn append_report(id: &str, report: &str) -> QueueResult<Option<Batch>> {
    let Some(batch) = Bus::findBatch(id).await? else {
        return Ok(None);
    };
    batch.add([Payload::new("reports.generate", report)?]).await
}
```

Redis 每次追加都有独立的 SQL 发布记录。初始发布确认后才会发布追加任务。恢复命令也会重试待发布的追加记录。自定义批次表时，还须建立同名加 `_additions` 后缀的配套表。

### 向 Redis 发布批次 {#redis-batches}

OpenRust 先在 SQL 中提交批次和待发布记录，再联系 Redis。发布失败时，`dispatch()` 仍返回已受理的批次。定期运行 `queue:publish-batches` 重试待发布记录。Redis 一起写入任务与发布标记，SQL 确认发布后，OpenRust 尝试给标记设置 24 小时有效期。Redis 的持久化和淘汰配置需保留队列数据与发布标记。自定义批次表时，还须建立同名加 `_jobs`、`_outbox`、`_additions`、`_continuations` 后缀的配套表。

```sh
./artisan queue:publish-batches --limit=1
```

### 批次回调 {#batch-callbacks}

在应用和 Worker 的 Provider 启动时，用稳定名称注册回调。批次持久化这些名称，独立 Worker 可按名称解析。回调接收批次快照，适用时也会收到任务最终失败的消息。

```rust
use openrust::{
    bus::Batch,
    log::Log,
    queue::{Payload, QueueError, QueueFuture, QueueResult},
    support::facades::Bus,
};

fn report_complete<'a>(batch: &'a Batch, _: Option<&'a str>) -> QueueFuture<'a, QueueResult<()>> {
    Box::pin(async move {
        let context = serde_json::Map::from_iter([("batch_id".into(), batch.id.clone().into())]);
        Log::info("Reports batch completed", &context)
            .map_err(|source| QueueError::Execution { source: Box::new(source) })
    })
}

fn register_batch_callbacks() -> QueueResult<()> {
    Bus::registerBatchCallback("reports.completed", report_complete)
}

async fn dispatch_with_callbacks() -> QueueResult<Batch> {
    Bus::batch([Payload::new("reports.generate", "documentation")?])
        .name("report export")
        .onQueue("reports")
        .then("reports.completed")
        .dispatch()
        .await
}
```

`before` 在入队前接收初始快照，`progress` 用于终态进度，`then` 在待处理数归零时运行，`catch` 处理首次最终失败，`finally` 在全部任务都有终态时运行。`onFailure` 允许失败并为每次最终失败注册回调。回调出错会写日志，Worker 继续处理。Worker 在 SQL 状态提交后调用回调，若进程在提交和调用之间退出，该次回调可能丢失。

### 批次事件 {#batch-events}

从 `openrust::bus::events` 为 `BatchDispatched`、`BatchStarted`、`BatchFinished` 和 `BatchCanceled` 注册监听器。每个事件携带批次快照，任务失败引起取消时还会携带失败消息。在 Provider 的 `boot` 中调用注册函数。

```rust
use std::sync::Arc;
use openrust::{
    bus::events::BatchFinished,
    contracts::events::{Event as Payload, Listener},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    log::Log,
    support::facades::Event,
};

struct ReportBatchFinished;

impl Listener for ReportBatchFinished {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Payload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(event) = event.downcast_ref::<BatchFinished>() {
                let context = serde_json::Map::from_iter([
                    ("batch_id".into(), event.batch.id.clone().into()),
                ]);
                let _ = Log::info("Batch finished", &context);
            }
            Ok(None)
        })
    }
}

fn register_batch_events() -> EventResult<()> {
    Event::listen(BatchFinished::NAME, Arc::new(ReportBatchFinished), Delivery::Sync)
}
```

事件在状态提交后派发。`BatchStarted` 在首个任务达到终态时触发，`BatchFinished` 在待处理任务归零时触发。同一次投递重放不会重复触发事件，不同 Worker 的监听器可能并发执行。

### 重试批次失败任务 {#batch-failures}

传入一个或多个批次 ID，将失败存储中仍可重试的任务放回原连接和队列。同一 SQL 连接中的重新入队和删除失败记录在一笔事务内提交。任务重试成功后，Worker 清除该任务的失败 ID。配置为跳过已取消批次的任务仍会遵守取消状态。

```sh
./artisan queue:retry-batch "$BATCH_ID"
```

使用共享缓存时，可以加上 `--isolated`，使相同批次 ID 列表的并发命令跳过执行。使用 `--isolated=12` 可在锁被占用时返回退出码 12。

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

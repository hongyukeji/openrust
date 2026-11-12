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

### 队列 Worker 与部署 {#queue-workers-and-deployment}

部署时运行重启命令。Worker 会完成当前任务并正常退出，随后由进程管理器启动新版 Worker。命令和 Worker 需要共用应用缓存，可使用数据库、Redis 或共享文件缓存。数组缓存只在当前进程生效。

```sh
./artisan queue:restart
```

### Worker 状态与退出原因 {#worker-status-and-exit-reasons}

Worker 在运行时输出任务进度及暂停、恢复通知。`--json` 每行输出一条 JSON 记录。`--quiet` 或 `--silent` 会隐藏正常输出。

```sh
./artisan queue:work --json --stop-when-empty
```

正常退出时，`stopped` 记录包含原因、退出码和处理任务数。当前原因包括 `empty`、`empty_for`、`max_jobs`、`max_time`、`memory`、`timed_out`、`lost_connection`、`interrupted` 和 `restart_signal`。单次 Worker 输出任务记录并省略退出摘要。应用可监听 `WorkerStopping` 观察正常退出。

时间戳优先使用 `queue.output_timezone`，其次使用 `app.timezone` 或 UTC。内存用量目前输出为 `null`。

#### Worker 生命周期事件 {#worker-lifecycle-events}

守护 Worker 启动时派发 `WorkerStarting`，每次轮询前派发 `Looping`，轮询没有找到可用任务时派发 `WorkerIdle`。正常退出时派发 `WorkerStopping`。这些事件携带连接、队列和 Worker 选项。

同步 `Looping` 监听器可返回 `Some(ListenerResponse::Value(Box::new(false)))` 跳过当前轮询。Worker 等待后再试，并继续遵守退出限制。被跳过的轮询不会派发 `WorkerIdle`。事件分发使用第一个监听器响应，返回 `None` 可让后续监听器决定。

队列为空时，以下守护进程会依次派发启动、循环、空闲和退出事件。`--once` 不派发这些守护进程事件。

```sh
./artisan queue:work --json --stop-when-empty
```

#### 任务预约事件 {#job-reservation-events}

监听 `JobPopping` 可观察 Worker 轮询，事件包含连接和所选队列。成功预约后、执行任务前派发 `JobPopped`。它携带只读快照，包括驱动任务 ID、队列、尝试次数和载荷。

`JobPopping` 监听器报错会在预约前终止命令。`JobPopped` 监听器报错会交给应用异常处理器。Worker 保留预约，驱动可在 `retry_after` 到期后重试任务。

队列为空时，下列命令会派发 `JobPopping` 后退出。成功预约任务时才会派发 `JobPopped`。

```sh
./artisan queue:work --json --once
```

#### 任务尝试事件 {#job-attempt-events}

`JobProcessing` 在执行前派发，`JobProcessed` 在任务成功返回后派发。任务主动释放时，随后派发 `JobReleased`。尝试发生异常时派发 `JobExceptionOccurred`，Worker 安排重试后派发 `JobReleasedAfterException`。永久失败记录写入并移除预约后，派发 `JobFailed`。

`JobAttempted` 在该次尝试的其他事件之后派发，提供 `successful()`、可选异常消息，以及实际开始执行时的毫秒耗时。事件携带只读任务快照。`JobProcessing` 监听器报错时按任务重试策略处理。其他监听器报错交给应用异常处理器，并保留队列操作的结果。

`JobTimedOut` 提供配置的超时值。OpenRust 取消异步任务 future，并应用 Worker 的重试策略，包括 `fail_on_timeout`。

默认队列有待处理任务时，运行一次尝试并打印状态。

```sh
./artisan queue:work --json --once --tries=2
```

#### 任务间隔与空闲退出 {#worker-rest-and-idle-timeout}

使用 `--rest` 设置任务执行之间的等待时间。使用 `--stop-when-empty-for` 时，队列为空且 Worker 连续指定秒数未处理任务，Worker 就会退出。空闲计时从启动开始，每次任务执行结束后重置。

```sh
./artisan queue:work --json --rest=1 --stop-when-empty-for=2
```

达到空闲时限时，退出原因为 `empty_for`。设为零可关闭空闲时限。`--stop-when-empty` 会在无可用任务时立即退出。设置空闲时限后，Redis 阻塞取任务最多等待剩余的空闲时间。

#### 限制 Worker 内存 {#worker-memory-limit}

`queue:work` 的默认上限为 128 MiB。守护进程在一次任务执行或轮询结束后检查进程常驻内存。达到上限时以退出码 `12` 和原因 `memory` 结束。当前任务会完成，待处理任务留在队列。平台能提供读数时，JSON 输出的 `memory` 单位为 MiB。

百分比以正值 `QUEUE_WORKER_MEMORY_LIMIT` 为基准，单位为 MiB。零或负值会关闭上限检查。单次 Worker 跳过上限检查。

```sh
./artisan queue:work --json --memory=128 --stop-when-empty
QUEUE_WORKER_MEMORY_LIMIT=256 ./artisan queue:work --json --memory=50% --stop-when-empty
```

#### 从连接中断恢复 {#worker-lost-connection}

守护进程在预约或处理任务时失去数据库或 Redis 连接，会报告异常，并以原因 `lost_connection` 和退出码 `0` 停止。进程管理器可以启动新的 Worker。无法确认完成的预约仍受驱动的重试窗口约束。

设置 `QUEUE_WORKER_STOP_ON_LOST_CONNECTION=false` 可在识别到断连后继续运行。预约失败后，Worker 等待一秒再轮询。任务失败仍按配置的重试和失败策略处理。应用可以在服务提供者中绑定 `openrust::contracts::database::LostConnectionDetector` 契约，自定义识别规则。

```sh
QUEUE_WORKER_STOP_ON_LOST_CONNECTION=false ./artisan queue:work --json --stop-when-empty
```

#### 任务异常次数上限 {#max-exceptions}

设置 `JobOptions::max_exceptions`，让任务在未处理异常或超时达到指定次数时失败。异常次数上限独立于尝试次数上限。主动释放和中间件处理异常后的释放均不消耗异常次数。

```rust
use openrust::queue::JobOptions;

let options = JobOptions {
    max_tries: Some(0),
    max_exceptions: Some(3),
    ..JobOptions::default()
};
```

Worker 在应用默认缓存中按任务 UUID 计数，计数器从首次异常起保留一天。独立 Worker 需要使用共享缓存才能读取同一计数。省略该选项时沿用原有尝试与超时规则。

##### 计入 Worker 崩溃 {#counting-worker-crashes}

为配置了 `max_exceptions` 的任务启用 `count_crashes_as_exceptions`，可把未完成的尝试纳入同一异常额度。Worker 在共享缓存中记录标记。预约到期后驱动重新投递，仍存在的标记计为一次异常。达到上限时，Worker 在再次调用处理器前记录失败。

```rust
use openrust::queue::JobOptions;

let options = JobOptions {
    max_tries: Some(0),
    max_exceptions: Some(3),
    count_crashes_as_exceptions: true,
    ..JobOptions::default()
};
```

正常成功、普通异常、主动释放及已处理的超时都会清除本次标记。标记一天后过期。独立 Worker 需要使用同一缓存存储和命名空间。

#### 按异常停止重试 {#stopping-retries-by-exception}

队列任务遇到特定错误需要直接失败时，在应用的 `withExceptions` 回调中注册规则。`dontRetry` 匹配异常本身或来源链中的类型。`dontRetryWhen` 按谓词判断异常。

```rust
use std::{io, num::ParseIntError};
use openrust::foundation::configuration::Exceptions;

fn configure_exceptions(exceptions: &mut Exceptions) {
    exceptions
        .dontRetry::<ParseIntError>()
        .dontRetryWhen(|exception| {
            exception.downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
        });
}
```

命中的异常进入正常的失败任务流程，仍可参与异常报告。未命中的异常沿用普通重试规则。独立执行器在原始类型仍可用时判断规则，再由父 Worker 结算预约。

#### 任务预约到期与超时 {#job-expirations-and-timeouts}

`queue:work --timeout` 默认 60 秒，任务可以覆盖此值。超时应短于队列连接的 `retry_after`，以便在超时 Worker 停止后，其他 Worker 能重新预约任务。

异步尝试超时后，守护进程取消该尝试，派发 `JobTimedOut`，并以退出码 `1` 和原因 `timed_out` 退出。达到尝试次数上限或启用 `fail_on_timeout` 时会写入失败记录。其余任务在当前租约到期后可重新预约。

使用 `QUEUE_WORKER_TIMED_OUT_EXIT_CODE` 可设置 0 到 255 的退出码。设置 `QUEUE_WORKER_KILL_ON_TIMEOUT=false` 可让守护进程继续运行并采用常规重试策略。单次 Worker 在一次尝试后退出，仍采用常规重试策略。Tokio 超时要求任务 future 在执行中让出控制权。

```sh
QUEUE_WORKER_TIMED_OUT_EXIT_CODE=124 ./artisan queue:work --json --timeout=60 --stop-when-empty
```

##### 阻塞任务的强制超时 {#hard-job-timeouts}

对于可能阻塞执行线程的任务，设置 `JobOptions::hard_timeout`。在 Unix 上，Artisan 在独立进程组中运行该任务。Worker 保留预约，并将实际任务超时应用于子进程，计时包括子进程启动。到期后，Worker 终止进程组、回收执行进程，再派发 `JobTimedOut` 并结算失败或重试。

执行进程通过 `Process` 启动的外部程序共享该进程组。主动建立新会话或进程组的程序可以脱离此边界。强制超时需要内置持久队列的预约和 Unix Worker。

```rust
use std::time::Duration;
use openrust::queue::JobOptions;

let options = JobOptions {
    timeout: Some(Duration::from_secs(10)),
    hard_timeout: true,
    ..JobOptions::default()
};
```

```sh
./artisan queue:work --json --timeout=60 --tries=2 --stop-when-empty
```

### 响应 Worker 系统信号 {#reacting-to-worker-signals}

在 Unix 上，向一个 Worker 守护进程发送 `SIGUSR2` 可暂停后续轮询，发送 `SIGCONT` 可恢复。Worker 会完成已预约的任务。这两个信号分别派发 `WorkerPausing` 和 `WorkerResuming`，共享队列的暂停设置保持原样。请使用编译后 Worker 进程的 PID。

```sh
kill -s USR2 "$WORKER_PID"
kill -s CONT "$WORKER_PID"
kill -s TERM "$WORKER_PID"
```

Unix 上的 `SIGTERM`、`SIGQUIT` 和 `SIGINT` 请求优雅退出，Windows 可用 Ctrl+C。守护进程派发 `WorkerInterrupted`，等待当前任务完成，再派发 `WorkerStopping`。监听器可读取信号编号、连接、队列及 Worker 选项。已开始的驱动阻塞预约可能在暂停生效前返回。

#### 通知运行中的任务 {#notifying-a-running-job}

实现 `Interruptible`，并从 `JobHandler::interruptible` 返回该实现，即可接收 Worker 的退出信号。回调运行在当前任务的应用和日志上下文中，应尽快返回，以便任务完成。回调成功后，Worker 派发 `JobInterrupted`，携带信号编号和只读任务快照。

超时后，OpenRust 取消异步尝试，以 `SIGALRM` 的信号编号调用回调，再按任务的超时失败或重试策略处理。回调报错时，Worker 交给应用异常处理器报告，并继续原有的退出或超时处理。

```rust
use openrust::{
    contracts::queue::{Interruptible, JobHandler},
    queue::{JobContext, Payload, QueueFuture, QueueResult},
};

struct ProcessPodcast;

impl Interruptible for ProcessPodcast {
    fn interrupted(&self, signal: i32) -> QueueResult<()> {
        println!("ProcessPodcast received signal {signal}");
        Ok(())
    }
}

impl JobHandler for ProcessPodcast {
    fn interruptible(&self) -> Option<&dyn Interruptible> {
        Some(self)
    }

    fn handle<'a>(
        &'a self,
        _context: &'a mut JobContext<'_>,
        _payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
```

### 暂停和恢复队列 Worker {#pausing-and-resuming-queue-workers}

通过连接和队列名称暂停队列。只写队列名时使用默认连接。Worker 会完成当前任务，此后跳过已暂停的队列，并继续处理其他可用队列。

```sh
./artisan queue:pause database:default
./artisan queue:continue database:default
```

用 `--all` 暂停或恢复所有队列。全局恢复保留单独设置的暂停状态。`queue:continue` 是 `queue:resume` 的别名。

```sh
./artisan queue:pause --all
./artisan queue:resume --all
```

Worker 在任务之间及空闲时检查共享缓存。`--sleep` 控制空闲轮询间隔。若要对某个应用关闭重启和暂停轮询，在服务提供者的 `boot` 方法中调用 `Queue::withoutInterruptionPolling()?`。 Redis Worker 配置了 `block_for` 时，会在阻塞取任务返回后再次检查。需要空闲 Worker 及时响应控制信号时，请配置有限的阻塞时长。

#### 在应用代码中暂停队列 {#pausing-queues-in-code}

在应用作用域内调用 Queue 门面。使用 `Ttl::Seconds` 指定时长，或用 `Ttl::Until` 指定绝对截止时间。暂停到期或显式调用 `Queue::resume` 后，Worker 可以继续消费该队列。

```rust
use openrust::{cache::Ttl, queue::QueueResult, support::facades::Queue};

async fn pause_default_queue() -> QueueResult<()> {
    let connection = Queue::connection(None)?;
    let name = connection.get_connection_name();
    Queue::pauseFor(name, "default", Ttl::Seconds(3)).await?;
    assert!(Queue::isPaused(name, "default").await?);
    assert_eq!(Queue::getPausedQueues(name, &["low", "default"]).await?, ["default"]);
    Ok(())
}
```

`QueuePaused`、`QueueResumed`、`QueuesPaused` 和 `QueuesResumed` 对应应用代码或 Artisan 发起的状态修改。`WorkerQueuePaused` 和 `WorkerQueueResumed` 对应运行中的 Worker 观察到的状态变化。例如限时暂停到期后，Worker 下次检查时会派发 `WorkerQueueResumed`。

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

同连接内换队列时，Worker 原子交接。跨持久连接时，源 Worker 会在自己的队列中保存转发任务。保持源队列有 Worker 运行，目标恢复后可重试投递。在每个数据库目标执行 `./artisan migrate`，创建 `job_chain_deliveries`。

指定为 `sync` 的后继会在源 Worker 处理转发任务时于该进程内执行。同步队列忽略任务延迟。处理器失败时，任务链停止并调用失败回调。源 Worker 在确认前崩溃时，同步处理器可能重跑。

源队列确认转发后，Redis 凭据在 24 小时后过期。SQL 凭据可用下列命令在目标队列连接清理，只删除确认时间早于 `--hours` 的记录。待确认记录和升级前缺少确认时间的记录继续保留，用于重放去重。

```sh
./artisan queue:prune-chain-deliveries database --hours=24
```

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

通过 `JobHandler::middleware` 返回中间件。用 `JobNext::run` 继续执行，或用 `JobContext::release` 安排下一次尝试。

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

### 队列限流 {#rate-limiting}

在服务提供者中注册命名任务限流器，并从任务处理器的中间件清单返回 `RateLimited`。策略接收队列载荷，可返回单个 `Limit`、多个限额或 `Limit::none()`。找不到策略或策略返回无限制时，任务继续执行。

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Limit},
    queue::middleware::RateLimited,
    support::facades::RateLimiter,
};

fn register_job_budget() -> CacheResult<()> {
    RateLimiter::forJob("job-budget", |job| {
        Limit::perMinute(1).by(job.data().as_str().unwrap_or_default())
    })
}

fn job_rate_limit() -> RateLimited {
    RateLimited::new("job-budget")
}
```

达到限额后，任务默认在剩余窗口再加三秒后可重新预约。使用 `releaseAfter` 可覆盖延迟，使用 `dontRelease` 则跳过处理器并确认该任务。释放仍消耗一次尝试。`RateLimitedWithRedis::new("job-budget").connection("default")` 在指定 Redis 连接上原子应用限额。

### 防止任务重叠 {#preventing-job-overlaps}

用 `WithoutOverlapping` 让修改同一资源的任务串行执行。默认只有注册名和锁键相同的任务竞争同一把锁。用 `shared()` 让不同任务共享锁键，用 `withPrefix` 区分锁的命名空间。

```rust
use std::time::Duration;
use openrust::queue::middleware::WithoutOverlapping;

fn account_lock(account_id: u64) -> WithoutOverlapping {
    WithoutOverlapping::new(account_id.to_string())
        .withPrefix("account-update:")
        .shared()
        .releaseAfter(Duration::from_secs(15))
        .expireAfter(Duration::from_secs(120))
}
```

发生竞争时默认立即释放任务。用 `releaseAfter` 指定相对延迟或绝对时间，或用 `dontRelease` 确认任务并跳过处理器。默认锁租约由缓存驱动决定。可设置 `expireAfter`，使原 Worker 在释放锁前停止时，其他 Worker 能在租约到期后取得锁。

### 异常节流 {#throttling-exceptions}

任务依赖的服务可能连续报错时，使用 `ThrottlesExceptions`。匹配的异常消耗失败额度，成功执行会清零。额度用完后，Worker 跳过处理器，把任务释放到窗口到期后重试。缓存版本会在等待时间上额外加三秒。

```rust
use openrust::queue::middleware::ThrottlesExceptions;

fn job_exception_throttle() -> ThrottlesExceptions {
    ThrottlesExceptions::new(10, 300)
        .by("billing-service")
        .backoff(1)
        .report()
}
```

用 `by` 让多种任务共享失败额度，或用 `byJob` 按任务 UUID 单独计数。`backoff` 设置达到阈值前的退避分钟数，`backoffWhen` 可按异常计算。`when` 选择参与节流的异常，`reportWhen` 通过应用异常处理器报告。`deleteWhen` 确认并删除任务，`failWhen` 记录终态失败。两个条件都命中时优先删除。

用 `ThrottlesExceptionsWithRedis::new(10, 300).connection("default")` 选择 Redis 连接。`when` 和 `reportWhen` 的第二个参数是 `ExceptionThrottleLimiter`，提供缓存服务，或 Redis 窗口的剩余额度和到期时间戳。

### 释放任务 {#releasing-jobs}

从处理器的中间件清单返回 `Release::when` 或 `Release::unless`，按条件延迟任务并跳过处理器。用 `Duration` 指定延迟，用 `Duration::ZERO` 让任务立即可再次预约。释放会消耗一次尝试。

```rust
use std::time::Duration;
use openrust::queue::middleware::Release;

fn defer_when_unavailable(available: bool) -> Release {
    Release::unless(available, Duration::from_secs(30))
}
```

### 跳过任务 {#skipping-jobs}

使用 `Skip::when` 或 `Skip::unless`，按条件确认任务并跳过处理器。两种中间件均接受布尔值或返回布尔值的闭包，条件在构造中间件时求值一次。

```rust
use openrust::queue::middleware::Skip;

fn skip_when_cancelled(cancelled: bool) -> Skip {
    Skip::when(|| cancelled)
}
```

## 失败任务

运行 `./artisan migrate` 创建 `failed_jobs`。用 `./artisan queue:failed --json` 查看失败记录，用 `./artisan queue:retry <uuid>` 将任务重新送到原连接和队列。`queue:forget` 和 `queue:flush` 可移除失败记录。超时会取消当前 Rust future，任务需要自行管理它在该 future 外启动的工作。

## 清理队列任务 {#clearing-jobs-from-queues}

用 `queue:clear` 删除指定数据库或 Redis 队列中待执行、延迟和已预约的任务。省略连接名或 `--queue` 时使用对应配置的默认值。多个队列名用逗号分隔。

```sh
./artisan queue:clear database --queue=emails,reports
```

命令输出删除数量。已经运行的处理器仍可能继续执行，但其预约已移除。失败任务和批次记录由各自的命令管理。

生产环境需要在终端确认，或为明确的非交互操作传入 `--force`。应用可以在服务提供者中调用 `ClearCommand::prohibit(true)` 禁用该命令，禁用时 `--force` 也无法执行。

## 队列监听器

通过 `ListenerRegistry::listenQueued::<YourEvent>` 登记可序列化的事件及稳定的监听器 ID。Worker 在其应用内解析监听器。监听器的 `after_commit` 可将入队推迟到事务提交后。

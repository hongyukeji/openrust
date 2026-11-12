# 佇列

## 產生工作型別 {#generating-job-classes}

Artisan 在 `app/jobs` 下產生工作模組。佇列工作攜帶可序列化資料並註冊 Worker 所需的處理器。用 `--sync` 產生的工作在等待其 `dispatch` Future 時直接執行。`--test` 產生配套功能測試，`--force` 覆寫現有檔案。

```sh
./artisan make:job SendReport --test
./artisan make:job LocalReport --sync
```

在應用程式中宣告產生的模組，在服務提供者的 `boot` 方法呼叫 `register_jobs`。範例先執行本機工作，再派送佇列工作。

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

透過 `--batched` 產生可加入批次的工作。同時傳入 `--sync` 時，Artisan 會選擇批次佇列範本。

```sh
./artisan make:job BatchReport --batched --test
```

宣告模組，並在服務提供者的 `boot` 方法呼叫 `BatchReport::register()`。產生的處理器略過已取消的批次，`handle` 接收可選的批次 ID。

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

## 建立與派送工作

應用程式啟動時透過 `QueueManager::register` 或 `Queue::register` 登記穩定的工作名稱。`JobHandler` 從 `Payload` 讀取 JSON 資料，並透過 `JobContext` 取得連線、佇列和嘗試次數。使用 `Queue::dispatch` 將工作送往已設定的連線。

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

## 連線與預約

應用程式骨架預設使用 `QUEUE_CONNECTION=database`。執行 `./artisan migrate` 建立 `jobs` 資料表。`sync` 立即執行並傳回處理結果，`database` 和 `redis` 將工作加入佇列。連線可以設定預設佇列和 `retry_after`。使用 `Delay::For` 設定相對時長，或使用 `Delay::Until` 指定絕對時間。

持久化連線透過佇列與工作契約提供 `pop`、`delete` 和 `release`。預約到期後可重新領取工作，舊控制代碼無法確認或釋放新預約。Redis 的 `block_for` 以秒為單位，`null` 立即傳回，`0` 持續等待工作。

## 在資料庫交易中派送

將使用中的交易作為 `Queue::dispatch` 的最後一個參數。連線啟用 `after_commit` 時，工作於最外層提交後派送。可以使用 `DispatchOptions.after_commit` 覆寫連線設定。復原會捨棄待派送工作。立即派送時，同一連線的資料庫佇列在交易內寫入，sync 處理器可使用 `JobContext::transaction` 存取目前交易。

## 執行佇列 Worker

執行 `./artisan queue:work database --stop-when-empty` 處理待執行工作並結束。`--once` 嘗試一個工作。使用 `--tries`、`--backoff` 和 `--timeout` 設定 Worker，工作可透過 `JobOptions` 覆寫。收到 SIGTERM 時，Worker 停止領取新工作並完成目前嘗試。

### 佇列 Worker 與部署 {#queue-workers-and-deployment}

部署時執行重啟命令。Worker 會完成目前工作並正常結束，再由程序管理器啟動新版 Worker。命令和 Worker 需要共用應用程式快取，可使用資料庫、Redis 或共用檔案快取。陣列快取只在目前程序生效。

```sh
./artisan queue:restart
```

### Worker 狀態與結束原因 {#worker-status-and-exit-reasons}

Worker 在執行時輸出工作進度及暫停、恢復通知。`--json` 每行輸出一筆 JSON 記錄。`--quiet` 或 `--silent` 會隱藏一般輸出。

```sh
./artisan queue:work --json --stop-when-empty
```

正常結束時，`stopped` 記錄包含原因、結束代碼和已處理工作數。現有原因包括 `empty`、`empty_for`、`max_jobs`、`max_time`、`memory`、`timed_out`、`lost_connection`、`interrupted` 和 `restart_signal`。單次 Worker 輸出工作記錄並省略結束摘要。應用程式可監聽 `WorkerStopping` 觀察正常結束。

時間戳記優先使用 `queue.output_timezone`，其次使用 `app.timezone` 或 UTC。記憶體用量目前輸出為 `null`。

#### Worker 生命週期事件 {#worker-lifecycle-events}

常駐 Worker 啟動時派發 `WorkerStarting`，每次輪詢前派發 `Looping`，輪詢沒有找到可用工作時派發 `WorkerIdle`。正常結束時派發 `WorkerStopping`。這些事件帶有連線、佇列和 Worker 選項。

同步 `Looping` 監聽器可傳回 `Some(ListenerResponse::Value(Box::new(false)))` 略過目前的輪詢。Worker 等待後再試，並繼續遵守結束限制。略過的輪詢不會派發 `WorkerIdle`。事件分派採用第一個監聽器回應，傳回 `None` 可讓後續監聽器決定。

佇列為空時，下列常駐程序會依序派發啟動、循環、閒置及結束事件。`--once` 不派發這些常駐程序事件。

```sh
./artisan queue:work --json --stop-when-empty
```

#### 工作預約事件 {#job-reservation-events}

監聽 `JobPopping` 可觀察 Worker 輪詢，事件包含連線和所選佇列。成功預約後、執行工作前派發 `JobPopped`。它帶有唯讀快照，包括驅動程式工作 ID、佇列、嘗試次數和承載資料。

`JobPopping` 監聽器出錯會在預約前結束指令。`JobPopped` 監聽器出錯會交由應用程式例外處理器。Worker 保留預約，驅動程式可在 `retry_after` 到期後重試工作。

佇列為空時，下列指令會派發 `JobPopping` 後結束。成功預約工作時才會派發 `JobPopped`。

```sh
./artisan queue:work --json --once
```

#### 工作嘗試事件 {#job-attempt-events}

`JobProcessing` 在執行前派發，`JobProcessed` 在工作成功傳回後派發。工作主動釋放時，接著派發 `JobReleased`。嘗試發生例外時派發 `JobExceptionOccurred`，Worker 安排重試後派發 `JobReleasedAfterException`。永久失敗記錄寫入並移除預約後，派發 `JobFailed`。

`JobAttempted` 在該次嘗試的其他事件之後派發，提供 `successful()`、選用的例外訊息，以及實際開始執行時的毫秒耗時。事件攜帶唯讀工作快照。`JobProcessing` 監聽器出錯時按工作重試策略處理。其他監聽器出錯則交由應用程式例外處理器，並保留佇列操作的結果。

`JobTimedOut` 提供設定的逾時值。OpenRust 取消非同步工作 future，並套用 Worker 的重試策略，包括 `fail_on_timeout`。

預設佇列有待處理工作時，執行一次嘗試並列印狀態。

```sh
./artisan queue:work --json --once --tries=2
```

#### 工作間隔與閒置結束 {#worker-rest-and-idle-timeout}

使用 `--rest` 設定工作執行之間的等待時間。使用 `--stop-when-empty-for` 時，佇列為空且 Worker 連續指定秒數未處理工作，Worker 就會結束。閒置計時從啟動開始，每次工作執行結束後重設。

```sh
./artisan queue:work --json --rest=1 --stop-when-empty-for=2
```

達到閒置時限時，結束原因為 `empty_for`。設為零可關閉閒置時限。`--stop-when-empty` 會在沒有可用工作時立即結束。設定閒置時限後，Redis 阻塞取工作最多等待剩餘的閒置時間。

#### 限制 Worker 記憶體 {#worker-memory-limit}

`queue:work` 的預設上限為 128 MiB。常駐程序在一次工作執行或輪詢結束後檢查程序常駐記憶體。達到上限時以結束碼 `12` 和原因 `memory` 結束。目前工作會完成，待處理工作留在佇列。平台能提供讀數時，JSON 輸出的 `memory` 單位為 MiB。

百分比以正值 `QUEUE_WORKER_MEMORY_LIMIT` 為基準，單位為 MiB。零或負值會關閉上限檢查。單次 Worker 略過上限檢查。

```sh
./artisan queue:work --json --memory=128 --stop-when-empty
QUEUE_WORKER_MEMORY_LIMIT=256 ./artisan queue:work --json --memory=50% --stop-when-empty
```

#### 從連線中斷恢復 {#worker-lost-connection}

常駐程序在預約或處理工作時失去資料庫或 Redis 連線，會回報例外，並以原因 `lost_connection` 和結束碼 `0` 停止。程序管理員可以啟動新的 Worker。無法確認完成的預約仍受驅動程式的重試視窗約束。

設定 `QUEUE_WORKER_STOP_ON_LOST_CONNECTION=false` 可在辨識到斷線後繼續執行。預約失敗後，Worker 等待一秒再輪詢。工作失敗仍依設定的重試和失敗策略處理。應用程式可在服務提供者中繫結 `openrust::contracts::database::LostConnectionDetector` 契約，自訂辨識規則。

```sh
QUEUE_WORKER_STOP_ON_LOST_CONNECTION=false ./artisan queue:work --json --stop-when-empty
```

#### 工作例外次數上限 {#max-exceptions}

設定 `JobOptions::max_exceptions`，讓工作在未處理例外或逾時達到指定次數時失敗。例外次數上限獨立於嘗試次數上限。主動釋放和中介軟體處理例外後的釋放均不消耗例外次數。

```rust
use openrust::queue::JobOptions;

let options = JobOptions {
    max_tries: Some(0),
    max_exceptions: Some(3),
    ..JobOptions::default()
};
```

Worker 在應用程式的預設快取中依工作 UUID 計數，計數器從首次例外起保留一天。獨立 Worker 需要使用共用快取才能讀取同一計數。省略此選項時沿用原有嘗試與逾時規則。

##### 計入 Worker 崩潰 {#counting-worker-crashes}

為設定了 `max_exceptions` 的工作啟用 `count_crashes_as_exceptions`，可將未完成的嘗試納入同一例外額度。Worker 在共用快取中記錄標記。預約到期後驅動程式重新投遞，仍存在的標記計為一次例外。達到上限時，Worker 在再次呼叫處理器前記錄失敗。

```rust
use openrust::queue::JobOptions;

let options = JobOptions {
    max_tries: Some(0),
    max_exceptions: Some(3),
    count_crashes_as_exceptions: true,
    ..JobOptions::default()
};
```

正常成功、一般例外、主動釋放及已處理的逾時都會清除本次標記。標記一天後到期。獨立 Worker 需要使用相同的快取儲存與命名空間。

#### 依例外停止重試 {#stopping-retries-by-exception}

佇列工作遇到特定錯誤需要直接失敗時，在應用程式的 `withExceptions` 回呼中註冊規則。`dontRetry` 比對例外本身或來源鏈中的型別。`dontRetryWhen` 依條件函式判斷例外。

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

符合規則的例外進入正常的失敗工作流程，仍可參與例外回報。未符合的例外沿用一般重試規則。獨立執行器在原始型別仍可用時判斷規則，再由父 Worker 結算預約。

#### 工作預約到期與逾時 {#job-expirations-and-timeouts}

`queue:work --timeout` 預設為 60 秒，工作可覆寫此值。逾時應短於佇列連線的 `retry_after`，以便在逾時 Worker 停止後，由其他 Worker 重新預約工作。

非同步嘗試逾時後，常駐程序取消該次嘗試、派發 `JobTimedOut`，並以結束碼 `1` 和原因 `timed_out` 結束。達到嘗試次數上限或啟用 `fail_on_timeout` 時會寫入失敗記錄。其餘工作在目前租約到期後可重新預約。

使用 `QUEUE_WORKER_TIMED_OUT_EXIT_CODE` 可設定 0 到 255 的結束碼。設定 `QUEUE_WORKER_KILL_ON_TIMEOUT=false` 可讓常駐程序繼續執行並採用一般重試策略。單次 Worker 在一次嘗試後結束，仍採用一般重試策略。Tokio 逾時要求工作 future 在執行時交出控制權。

```sh
QUEUE_WORKER_TIMED_OUT_EXIT_CODE=124 ./artisan queue:work --json --timeout=60 --stop-when-empty
```

##### 阻塞工作的強制逾時 {#hard-job-timeouts}

對於可能阻塞執行緒的工作，設定 `JobOptions::hard_timeout`。在 Unix 上，Artisan 在獨立程序群組中執行該工作。Worker 保留預約，並將實際工作逾時套用至子程序，計時包含子程序啟動。到期後，Worker 終止程序群組、回收執行程序，再派發 `JobTimedOut` 並結算失敗或重試。

執行程序透過 `Process` 啟動的外部程式共用該程序群組。主動建立新工作階段或程序群組的程式可以脫離此邊界。強制逾時需要內建持久佇列的預約與 Unix Worker。

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

### 回應 Worker 系統訊號 {#reacting-to-worker-signals}

在 Unix 上，向一個 Worker 常駐程序傳送 `SIGUSR2` 可暫停後續輪詢，傳送 `SIGCONT` 可恢復。Worker 會完成已預約的工作。這兩個訊號分別派發 `WorkerPausing` 和 `WorkerResuming`，共用佇列的暫停設定維持原樣。請使用編譯後 Worker 程序的 PID。

```sh
kill -s USR2 "$WORKER_PID"
kill -s CONT "$WORKER_PID"
kill -s TERM "$WORKER_PID"
```

Unix 上的 `SIGTERM`、`SIGQUIT` 和 `SIGINT` 請求正常停止，Windows 可用 Ctrl+C。常駐程序派發 `WorkerInterrupted`，等待目前工作完成，再派發 `WorkerStopping`。監聽器可讀取訊號編號、連線、佇列及 Worker 選項。已開始的驅動程式阻塞預約可能在暫停生效前傳回。

#### 通知執行中的工作 {#notifying-a-running-job}

實作 `Interruptible`，並從 `JobHandler::interruptible` 傳回該實作，即可接收 Worker 的結束訊號。回呼在目前工作的應用程式與記錄上下文中執行，應儘快傳回，以便工作完成。回呼成功後，Worker 派發 `JobInterrupted`，帶有訊號編號與唯讀工作快照。

逾時後，OpenRust 取消非同步嘗試，以 `SIGALRM` 的訊號編號呼叫回呼，再依工作的逾時失敗或重試策略處理。回呼出錯時，Worker 交由應用程式例外處理器回報，並繼續原有的結束或逾時處理。

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

### 暫停和恢復佇列 Worker {#pausing-and-resuming-queue-workers}

透過連線和佇列名稱暫停佇列。只寫佇列名稱時使用預設連線。Worker 會完成目前工作，此後略過已暫停的佇列，並繼續處理其他可用佇列。

```sh
./artisan queue:pause database:default
./artisan queue:continue database:default
```

使用 `--all` 暫停或恢復所有佇列。全域恢復保留個別設定的暫停狀態。`queue:continue` 是 `queue:resume` 的別名。

```sh
./artisan queue:pause --all
./artisan queue:resume --all
```

Worker 在工作之間及閒置時檢查共用快取。`--sleep` 控制閒置輪詢間隔。若要對某個應用程式關閉重啟和暫停輪詢，請在服務提供者的 `boot` 方法中呼叫 `Queue::withoutInterruptionPolling()?`。 Redis Worker 設定了 `block_for` 時，會在阻塞取工作返回後再次檢查。若要讓閒置 Worker 及時回應控制訊號，請設定有限的阻塞時間。

#### 在應用程式碼中暫停佇列 {#pausing-queues-in-code}

在應用程式作用域內呼叫 Queue 門面。使用 `Ttl::Seconds` 指定時長，或用 `Ttl::Until` 指定絕對截止時間。暫停到期或明確呼叫 `Queue::resume` 後，Worker 可以繼續消費該佇列。

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

`QueuePaused`、`QueueResumed`、`QueuesPaused` 和 `QueuesResumed` 對應應用程式碼或 Artisan 發起的狀態變更。`WorkerQueuePaused` 和 `WorkerQueueResumed` 對應執行中的 Worker 觀察到的狀態變化。例如限時暫停到期後，Worker 下次檢查時會派發 `WorkerQueueResumed`。

## 工作鏈 {#job-chaining}

將已註冊的工作承載資料傳給 `Bus::chain`，在建構器上呼叫 `dispatch().await` 派送第一個工作。使用資料庫或 Redis 驅動時，Worker 在同一次儲存操作中確認目前工作成功並將下一項入列。重試或釋放時，工作鏈停在目前工作。工作最終失敗時，工作鏈停止。sync 驅動在目前程序內依序執行。

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

使用 `onConnection` 和 `onQueue` 選擇工作鏈的派送位置。`delay` 作用於第一個工作。派送前可用建構器的 `prepend` 和 `append` 調整順序。

在服務提供者的 `boot` 方法中以 `Bus::registerChainCallback` 按名稱註冊失敗回呼，再以 `catch("reports.failed")` 關聯。Worker 在自己的應用程式內解析回呼，並傳入失敗的工作承載資料和佇列錯誤。工作應確保副作用具冪等性，因為 Worker 遺失預約後可能再次執行工作。工作鏈回呼沒有獨立的重試紀錄。

### 工作鏈連線和佇列 {#chain-connection-queue}

使用 `PendingChain::onConnection` 和 `onQueue` 設定工作鏈的預設目標。在工作承載資料上呼叫同名方法可覆寫該工作的目標。後續工作仍使用鏈的預設值，除非它也設定了覆寫。後續工作的延遲從前一項成功時開始計算。第一個工作的延遲優先於鏈建構器的延遲。

```rust
use openrust::queue::{Delay, Payload};

fn route_job(job: Payload, connection: &str, queue: &str, delay: Delay) -> Payload {
    job.onConnection(connection).onQueue(queue).delay(delay)
}
```

同一連線內切換佇列時，Worker 原子交接。跨持久連線時，來源 Worker 會在自己的佇列中儲存轉送工作。保持來源佇列有 Worker 執行，目標恢復後可重試派送。在每個資料庫目標執行 `./artisan migrate`，建立 `job_chain_deliveries`。

指定為 `sync` 的後續工作會在來源 Worker 處理轉送工作時於該程序內執行。同步佇列忽略工作延遲。處理器失敗時，工作鏈停止並呼叫失敗回呼。來源 Worker 在確認前當機時，同步處理器可能重新執行。

來源佇列確認轉送後，Redis 憑據於 24 小時後到期。SQL 憑據可用下列命令在目標佇列連線清理，只刪除確認時間早於 `--hours` 的紀錄。待確認紀錄和升級前缺少確認時間的紀錄繼續保留，用於重放去重。

```sh
./artisan queue:prune-chain-deliveries database --hours=24
```

### 向目前工作鏈加入工作 {#adding-jobs-to-the-chain}

在工作處理器內，使用 `JobContext::prependToChain` 讓新工作緊接目前工作執行，使用 `appendToChain` 將新工作放到現有鏈尾。處理器成功後變更生效。失敗或釋放會捨棄本次嘗試的變更。

```rust
use openrust::queue::{JobContext, Payload, QueueResult};

fn edit_chain(context: &mut JobContext<'_>, before: Payload, after: Payload) -> QueueResult<()> {
    context.prependToChain(before)?;
    context.appendToChain(after)
}
```

每次呼叫接收一個工作承載資料或待派送批次。插入批次遵循[工作鏈中的批次](#batches-within-chains)規則。若目前工作鏈已屬於某個批次，可插入工作承載資料來擴充該批次。新工作共用批次 ID，Worker 在記錄目前工作成功時一併儲存新增數量和後繼。使用批次鏈的執行期間變更前，請執行 `./artisan migrate`。

### 依條件派送工作鏈 {#conditional-chain-dispatch}

條件為真時可用 `dispatchIf` 派送，條件為假時可用 `dispatchUnless` 派送。兩者都接受布林值或回傳布林值的閉包。等待派送 Future 時，閉包只執行一次。

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

略過時回傳 `None`，派送成功時回傳 `Some(DispatchResult)`。略過的工作鏈不會解析佇列連線或執行工作。

## 工作批次 {#job-batching}

執行 `./artisan migrate` 建立批次帳本和投遞紀錄。在應用程式啟動時註冊工作處理器，再透過 `Bus::batch` 一起派送這些工作。

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

執行 `./artisan queue:work database --queue=batch --stop-when-empty` 處理批次。用 `Bus::findBatch(&id)` 讀取目前狀態，用 `Bus::cancelBatch(&id)` 主動取消批次。工作最終失敗時預設取消批次。透過 `allowFailures()` 允許批次繼續，透過 `SkipIfBatchCancelled` 略過已取消批次的工作。失敗工作在重試成功前仍計入待處理數。

資料庫佇列與批次儲存庫須使用相同 SQL 連線，批次和入列操作在同一交易提交。派送到 Redis 時，在 `dispatch()` 前呼叫 `onConnection("redis")`。

### 批次中的工作鏈 {#chains-and-batches}

在待派送批次上使用 `addChain` 加入一組依序執行的工作。它對應 Laravel `Bus::batch` 裡的巢狀陣列。所有成員共用批次 ID、連線和佇列。批次統計每個成員，首次入列只放入各鏈的第一個工作。多個 Worker 可以平行處理不同的工作鏈。

```rust
use openrust::{bus::PendingBatch, queue::Payload, support::facades::Bus};

fn batched_chains(left: [Payload; 2], right: [Payload; 2]) -> PendingBatch {
    Bus::batch([]).addChain(left).addChain(right)
}
```

在建構器上呼叫 `dispatch().await` 派送。批次派送後，可用 `Batch::addChain(...).await` 加入工作鏈。工作最終失敗會停止所在的鏈。即使批次允許失敗，鏈中尚未執行的工作仍維持待處理狀態。重試失敗的工作可恢復該鏈。

### 工作鏈中的批次 {#batches-within-chains}

使用 `appendBatch` 或 `prependBatch` 將待派送批次加入工作鏈。Worker 執行到該步驟時派送批次。批次預設繼承工作鏈的連線和佇列，也可以個別設定。兩個階段都支援 Redis 及與批次儲存庫共用連線的資料庫佇列。

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

批次進入最終回呼階段後，外層工作鏈繼續。批次允許失敗時，仍可進入該階段並繼續。取消批次會停止外層工作鏈。工作失敗導致批次取消時，外層工作鏈的 `catch` 回呼會收到失敗的批次工作和錯誤。

執行 `./artisan migrate` 建立續鏈資料表。發佈失敗時，續鏈工作保留在 SQL 中，執行 `./artisan queue:publish-batches` 可恢復發佈。

### 向批次新增工作 {#adding-jobs-to-batches}

可在工作處理器或其他應用程式入口呼叫 `Batch::add`。它沿用批次的佇列和連線選項，批次仍存在時會回傳更新後的快照。新增工作會增加工作總數和待處理數，並清除完成時間。已取消的批次保留取消狀態。每個新增工作會取得新的佇列 UUID。

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

Redis 每次新增工作都有獨立的 SQL 發佈紀錄。初始發佈確認後才會發佈新增工作。復原指令也會重試待發佈的新增紀錄。自訂批次資料表時，還須建立同名加 `_additions` 後綴的配套資料表。

### 發佈批次至 Redis {#redis-batches}

OpenRust 先在 SQL 提交批次和待發佈紀錄，再聯絡 Redis。發佈失敗時，`dispatch()` 仍回傳已受理的批次。定期執行 `queue:publish-batches` 重試待發佈紀錄。Redis 一併寫入工作與發佈標記，SQL 確認發佈後，OpenRust 嘗試為標記設定 24 小時有效期。Redis 的持久化和淘汰設定須保留佇列資料與發佈標記。自訂批次資料表時，還須建立同名加 `_jobs`、`_outbox`、`_additions`、`_continuations` 後綴的配套資料表。

```sh
./artisan queue:publish-batches --limit=1
```

### 批次回呼 {#batch-callbacks}

在應用程式和 Worker 的 Provider 啟動時，使用穩定名稱註冊回呼。批次持久保存這些名稱，獨立 Worker 可依名稱解析。回呼接收批次快照，適用時也會收到工作最終失敗的訊息。

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

`before` 在入列前接收初始快照，`progress` 用於終態進度，`then` 在待處理數歸零時執行，`catch` 處理首次最終失敗，`finally` 在所有工作都有終態時執行。`onFailure` 允許失敗並為每次最終失敗註冊回呼。回呼出錯會寫入日誌，Worker 繼續處理。Worker 在 SQL 狀態提交後呼叫回呼，若程序在提交和呼叫之間退出，該次回呼可能遺失。

### 批次事件 {#batch-events}

從 `openrust::bus::events` 為 `BatchDispatched`、`BatchStarted`、`BatchFinished` 和 `BatchCanceled` 註冊監聽器。每個事件都帶有批次快照，工作失敗導致取消時還會帶有失敗訊息。在 Provider 的 `boot` 中呼叫註冊函式。

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

事件在狀態提交後派送。`BatchStarted` 在首個工作達到終態時觸發，`BatchFinished` 在待處理工作歸零時觸發。同一次投遞重播不會重複觸發事件，不同 Worker 的監聽器可能同時執行。

### 重試批次失敗工作 {#batch-failures}

傳入一個或多個批次 ID，將失敗儲存中仍可重試的工作放回原連線和佇列。同一 SQL 連線中的重新入列和刪除失敗紀錄在一筆交易內提交。工作重試成功後，Worker 清除該工作的失敗 ID。設定為略過已取消批次的工作仍會遵守取消狀態。

```sh
./artisan queue:retry-batch "$BATCH_ID"
```

使用共用快取時，可以加上 `--isolated`，讓相同批次 ID 清單的並行指令略過執行。使用 `--isolated=12` 可在鎖被占用時回傳結束碼 12。

### 清理批次 {#pruning-batches}

定期執行此指令可刪除已完成超過 24 小時的批次。透過 `--hours` 修改已完成批次的保留期限。也可以透過 `--unfinished` 和 `--cancelled` 分別指定未完成和已取消批次的保留時數。這兩個選項都從批次建立時間起算。指令會分別顯示每條規則刪除的紀錄數，批次的投遞紀錄會隨批次一起清理。

```sh
./artisan queue:prune-batches
./artisan queue:prune-batches --hours=48 --unfinished=72 --cancelled=36
```

## 工作中介軟體 {#job-middleware}

透過 `JobHandler::middleware` 傳回中介軟體。使用 `JobNext::run` 繼續執行，或以 `JobContext::release` 安排下一次嘗試。

產生的中介層位於 `app/jobs/middleware`，其處理器將承載資料與上下文交給下一步。

```sh
./artisan make:job-middleware GuardReport --test
```

宣告中介層模組，並在產生工作的 `impl JobHandler` 中加入以下方法以啟用中介層。

```rust
fn middleware(&self) -> Vec<std::sync::Arc<dyn openrust::queue::middleware::JobMiddleware>> {
    vec![std::sync::Arc::new(crate::app::jobs::middleware::guard_report::GuardReport)]
}
```

### 佇列限流 {#rate-limiting}

在服務提供者中註冊具名工作限流器，並從工作處理器的中介軟體清單傳回 `RateLimited`。策略接收佇列承載資料，可傳回單個 `Limit`、多個限額或 `Limit::none()`。找不到策略或策略傳回無限制時，工作繼續執行。

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

達到限額後，工作預設在剩餘視窗再加三秒後可重新預約。使用 `releaseAfter` 可覆寫延遲，使用 `dontRelease` 則略過處理器並確認該工作。釋放仍會消耗一次嘗試。`RateLimitedWithRedis::new("job-budget").connection("default")` 在指定 Redis 連線上原子套用限額。

### 防止工作重疊 {#preventing-job-overlaps}

用 `WithoutOverlapping` 讓修改同一資源的工作循序執行。預設只有註冊名稱和鎖鍵相同的工作競爭同一把鎖。用 `shared()` 讓不同工作共用鎖鍵，用 `withPrefix` 區分鎖的命名空間。

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

發生競爭時預設立即釋放工作。用 `releaseAfter` 指定相對延遲或絕對時間，或用 `dontRelease` 確認工作並略過處理器。預設鎖租約由快取驅動程式決定。可設定 `expireAfter`，讓原 Worker 在釋放鎖前停止時，其他 Worker 能在租約到期後取得鎖。

### 例外節流 {#throttling-exceptions}

工作依賴的服務可能連續失敗時，使用 `ThrottlesExceptions`。符合條件的例外會消耗失敗額度，成功執行會清零。額度用完後，Worker 略過處理器，將工作釋放到視窗到期後重試。快取版本會在等待時間上額外加三秒。

```rust
use openrust::queue::middleware::ThrottlesExceptions;

fn job_exception_throttle() -> ThrottlesExceptions {
    ThrottlesExceptions::new(10, 300)
        .by("billing-service")
        .backoff(1)
        .report()
}
```

用 `by` 讓多種工作共用失敗額度，或用 `byJob` 依工作 UUID 分別計數。`backoff` 設定達到門檻前的退避分鐘數，`backoffWhen` 可依例外計算。`when` 選擇參與節流的例外，`reportWhen` 透過應用程式的例外處理器回報。`deleteWhen` 確認並刪除工作，`failWhen` 記錄終態失敗。兩個條件都符合時優先刪除。

用 `ThrottlesExceptionsWithRedis::new(10, 300).connection("default")` 選擇 Redis 連線。`when` 和 `reportWhen` 的第二個參數是 `ExceptionThrottleLimiter`，提供快取服務，或 Redis 視窗的剩餘額度與到期時間戳記。

### 釋放工作 {#releasing-jobs}

從處理器的中介軟體清單傳回 `Release::when` 或 `Release::unless`，依條件延後工作並略過處理器。用 `Duration` 指定延遲，用 `Duration::ZERO` 讓工作立即可再次預約。釋放會消耗一次嘗試。

```rust
use std::time::Duration;
use openrust::queue::middleware::Release;

fn defer_when_unavailable(available: bool) -> Release {
    Release::unless(available, Duration::from_secs(30))
}
```

### 略過工作 {#skipping-jobs}

使用 `Skip::when` 或 `Skip::unless`，依條件確認工作並略過處理器。兩種中介軟體均接受布林值或傳回布林值的閉包，條件在建構中介軟體時求值一次。

```rust
use openrust::queue::middleware::Skip;

fn skip_when_cancelled(cancelled: bool) -> Skip {
    Skip::when(|| cancelled)
}
```

## 失敗工作

執行 `./artisan migrate` 建立 `failed_jobs`。使用 `./artisan queue:failed --json` 查看失敗記錄，以 `./artisan queue:retry <uuid>` 將工作重新送至原連線與佇列。`queue:forget` 和 `queue:flush` 可移除失敗記錄。逾時會取消目前的 Rust future，工作需自行管理在該 future 外啟動的作業。

## 清除佇列工作 {#clearing-jobs-from-queues}

用 `queue:clear` 刪除指定資料庫或 Redis 佇列中待執行、延遲和已預約的工作。省略連線名稱或 `--queue` 時使用對應設定的預設值。多個佇列名稱以逗號分隔。

```sh
./artisan queue:clear database --queue=emails,reports
```

命令輸出刪除數量。已執行中的處理器仍可能繼續執行，但其預約已移除。失敗工作和批次記錄由各自的命令管理。

正式環境需要在終端機確認，或為明確的非互動操作傳入 `--force`。應用程式可以在服務提供者中呼叫 `ClearCommand::prohibit(true)` 禁用此命令，禁用時 `--force` 也無法執行。

## 佇列監聽器

透過 `ListenerRegistry::listenQueued::<YourEvent>` 登記可序列化的事件與穩定的監聽器 ID。Worker 在其應用程式內解析監聽器。監聽器的 `after_commit` 可將加入佇列的動作延後至交易提交後。

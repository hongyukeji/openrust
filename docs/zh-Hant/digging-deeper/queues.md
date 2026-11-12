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

OpenRust 先在 SQL 提交批次和待發佈紀錄，再聯絡 Redis。發佈失敗時，`dispatch()` 仍回傳已受理的批次。定期執行 `queue:publish-batches` 重試待發佈紀錄。Redis 一併寫入工作與發佈標記，SQL 確認發佈後，OpenRust 嘗試為標記設定 24 小時有效期。Redis 的持久化和淘汰設定須保留佇列資料與發佈標記。自訂批次資料表時，還須建立同名加 `_jobs`、`_outbox` 後綴的配套資料表。

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

透過 `JobHandler::middleware` 傳回中介軟體。使用 `JobNext::run` 繼續執行，或以 `JobContext::release` 安排下一次嘗試。`WithoutOverlapping` 使用設定的快取鎖，可延後釋放衝突的工作。未完成的鎖會在租期結束時到期。

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

## 失敗工作

執行 `./artisan migrate` 建立 `failed_jobs`。使用 `./artisan queue:failed --json` 查看失敗記錄，以 `./artisan queue:retry <uuid>` 將工作重新送至原連線與佇列。`queue:forget` 和 `queue:flush` 可移除失敗記錄。逾時會取消目前的 Rust future，工作需自行管理在該 future 外啟動的作業。

## 佇列監聽器

透過 `ListenerRegistry::listenQueued::<YourEvent>` 登記可序列化的事件與穩定的監聽器 ID。Worker 在其應用程式內解析監聽器。監聽器的 `after_commit` 可將加入佇列的動作延後至交易提交後。

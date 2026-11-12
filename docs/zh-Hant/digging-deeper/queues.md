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

批次派送目前使用與批次儲存庫相同連線的資料庫佇列。批次建立和所有入列操作在同一交易提交。自訂批次資料表時，還需建立同名加 `_jobs` 後綴的投遞紀錄表。Redis 派送和回呼待接入。

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

# 佇列

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

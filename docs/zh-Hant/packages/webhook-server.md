# Webhook 發送（擴展）

## 支援狀態 {#support-status}

可選套件 `openrust-webhook-server` 提供請求快照、目的地驗證、具名簽名器、退避策略及持久投遞嘗試。重放與端點管理仍在開發。

## 啟用與設定 {#configuration}

將 `webhook-server` 加入根應用 `[features].default`，保留既有 feature，套件發現會註冊 `WebhookServerServiceProvider`；其他應用可直接註冊。預設設定合併不存取資料庫或網路。用以下命令發布範本，在 `config/mod.rs` 增加 `mod webhook_server;` 與 `("webhook-server".to_owned(), webhook_server::load(env)?),`，再重新編譯。重複發布保留自訂，明確指定 `--force` 才覆寫。

```sh
./artisan vendor:publish --tag=webhook-server-config
```

## 可用入口 {#available-entry-points}

### 簽名原始位元組 {#signatures}

`DefaultSigner` 使用 HMAC-SHA256，以小寫十六進位寫入 `Signature`。預設演算法只涵蓋本文，修改 URL、訊息 ID 或時間戳不會改變該簽名。`DefaultSigner::new` 可更換簽名頭。`StandardWebhookSigner` 重用官方 `standardwebhooks 1.3.0`，回傳協定要求的三個頭；金鑰須為合法 base64，可帶 `whsec_` 前綴，本文須為 UTF-8 位元組。

`StandardWebhookSigner::headers` 要求非空 ASCII 訊息 ID，最多256位元組，不得包含控制字元或首尾空白，以確保產生的頭經過 HTTP 解析後保持一致。

```rust
use openrust::http::HeaderMap;
use openrust_webhook_server::{
    DefaultSigner, Result, Signer, SigningContext, StandardWebhookSigner,
};

pub fn spatie_signature(body: &[u8], secret: &str) -> Result<String> {
    DefaultSigner::default().calculate_signature(&SigningContext {
        webhook_url: "https://example.com/webhook",
        body, secret, message_id: "message-1", timestamp: 0,
    })
}

pub fn standard_headers(
    body: &[u8], secret: &str, message_id: &str, timestamp: i64,
) -> Result<HeaderMap> {
    StandardWebhookSigner.headers(&SigningContext {
        webhook_url: "https://example.com/webhook",
        body, secret, message_id, timestamp,
    })
}

fn invoice_call() -> Result<openrust_webhook_server::WebhookCall> {
    use openrust_webhook_server::WebhookCall;

    Ok(WebhookCall::new()?
        .url("https://billing.example.com/callback")
        .uuid("invoice-issued-inv-123")
        .payload(serde_json::json!({"invoice_id": "inv-123"}))
        .use_secret_reference("billing-v1")
        .on_connection(Some("database".into())))
}

pub fn prepare_invoice() -> Result<openrust_webhook_server::PreparedWebhook> {
    invoice_call()?.prepare(&openrust_webhook_server::TargetPolicy::default())
}

pub async fn submit_invoice(store: &openrust_webhook_server::DeliveryStore) -> Result<String> {
    store.submit("invoice-issued-inv-123", &prepare_invoice()?, 3600).await
}

pub async fn queue_invoice(
    service: &openrust_webhook_server::DeliveryService,
    queue: &openrust::queue::QueueManager,
) -> Result<openrust_webhook_server::QueuedDelivery> {
    invoice_call()?.dispatch(
        service, queue, &openrust_webhook_server::TargetPolicy::default(),
        "invoice-issued-inv-123", 3600,
    ).await
}

pub fn schedule_webhook_recovery(schedule: &mut openrust::console::scheduling::Schedule) {
    schedule
        .command("webhook-server:recover", ["--scope=billing".into()])
        .every_minute();
}
```


### 準備對外請求 {#prepare}

在已啟動的應用中呼叫 `WebhookCall::create()` 讀取套件設定，或使用 `WebhookCall::new()` 取得內建預設。設定目的地、載荷與憑據，再呼叫 `prepare(&TargetPolicy::default())`。快照保存穩定訊息 ID、已序列化載荷與排程設定。`headers_at` 按試行時間戳為已保存的位元組產生簽名頭。憑據由應用在執行時解析的場景可傳入具名憑據引用。

POST 等正文方法使用已準備的位元組。GET 將載荷編碼為 RFC 3986 查詢字串，Spatie 簽名繼續使用已準備的 JSON。原始正文模式可保留任意位元組。快照限制正文為 16 MiB、Header 為 64 KiB。透過 `register_signer` 和 `register_backoff_strategy` 將外部實作註冊到應用容器。

預設目的地策略要求 HTTPS，並重新核對解析所得的全部 Socket 位址。內部端點使用應用可信設定提供固定來源、路徑及位址。`PreparedWebhook::bind_target` 可驗證單次試行的位址清單。HTTP 用戶端固定實際連線位址，保留 Host 與 SNI，禁止重新導向和代理轉送，HTTPS 預設驗證憑證。

### 登記與投遞 {#delivery}

啟用套件後執行 `./artisan migrate`。用應用資料庫連線和投遞範圍建構 `DeliveryStore`。業務變更需要與投遞登記原子提交時，使用同庫的 `submit_in_transaction`。個別登記則呼叫 `submit`。同一鍵與相同快照會回傳既有投遞 ID，同一鍵搭配不同快照會回傳衝突。需要重複登記時，先為快照指定穩定的訊息 ID。

透過 `register_delivery_policy` 註冊應用策略。每次嘗試都按目前設定核對投遞範圍、URL 和憑據引用，在執行時解析金鑰。保存的快照拒絕內嵌金鑰與敏感請求頭，佇列載荷只含投遞 ID。避免把敏感值放進 URL、本文與中繼資料。

佇列發送需要在各 Worker 程序用 `DeliveryService::register_worker` 註冊準備時選定的 Job 名稱，統一具名資料庫、投遞範圍和策略，並設定 `database` 等直接供 Worker 使用的佇列連線。同一投遞同一時間只允許一個領取。HTTP 2xx 視為成功，其餘狀態按設定重試。請求逾時或程序中斷可能使遠端結果未知，接收端可按穩定的訊息 ID 去重。

Builder 的 `dispatch`、`dispatch_if` 和 `dispatch_unless` 登記投遞並入佇列。對應的 `dispatch_sync` 系列會在登記後由目前程序嘗試投遞。條件方法傳入 Rust 布林值，略過時直接回傳 `None`。啟用 `throw_exception_on_failure(true)` 後，同步投遞最終失敗會回傳帶有投遞 ID 的型別化錯誤，佇列投遞則交由原生 Worker 記錄失敗工作。

在 `openrust_webhook_server::events` 下註冊同步監聽器。`DispatchingWebhookCallEvent` 在投遞記錄提交後、入佇列前觸發。`WebhookCallSucceededEvent` 和 `WebhookCallFailedEvent` 在嘗試結果提交後觸發，最終失敗隨後還會派發 `FinalWebhookCallFailedEvent`。共用載荷提供訊息與投遞 ID、請求方法、URL 路徑、本文和中繼資料的存取方法，以及嘗試狀態。事件省略 URL 查詢字串、請求標頭的值和回應內容。入佇列前監聽器失敗時可從持久記錄繼續投遞，結果監聽器失敗時以已提交的結果為準。

```sh
./artisan webhook-server:send <delivery-id> --scope=billing
./artisan webhook-server:send <delivery-id> --scope=billing --queue
./artisan webhook-server:send --scope=billing --limit=100
```

### 定時恢復 {#recovery}

在應用調度器中註冊 `webhook-server:recover`，並每分鐘執行 `./artisan schedule:run`。命令依投遞記錄的 Worker 路由入佇列，報告候選、成功入列、過期或耗盡、略過與派發失敗的數量。`--pretend` 僅統計候選。投遞儲存使用具名連線時，在命令中指定 `--database`。

恢復程序在入佇列前預留 60 秒喚醒視窗。未入列或遺失的工作可在視窗到期後補派。Worker 領取或提交重試結果時會清除視窗，下一次嘗試繼續受投遞租約與退避控制。單批選擇不超過 `--limit` 筆，派發預算為 30 秒。多個調度程序透過共用資料庫協調，各節點須提供記錄對應的 Worker 路由。

```sh
./artisan webhook-server:recover --scope=billing --limit=100
./artisan webhook-server:recover --scope=billing --pretend
```

## 自訂簽名與退避 {#customization}

外部 crate 可實作 `Signer`。`SigningContext` 提供不可變本文、URL、執行時憑據、穩定訊息 ID 和單次嘗試時間戳，這是上游簽名參數的 Rust 適配。金鑰以借用傳入，上下文不支援序列化或 Debug 輸出。`headers` 方法支援協定附加的認證頭。不要把金鑰加入請求中繼資料、日誌或一般佇列載荷。

`BackoffStrategy::wait_in_seconds_after_attempt` 回傳秒數。預設 `ExponentialBackoffStrategy` 沿固定基線：嘗試1至4分別等待10、100、1000、10000秒，超過4等待100000秒。底層契約的0回傳1秒，首個實際嘗試編號為1。


## 使用邊界 {#scope}

網路和任務執行重用既有 [HTTP 用戶端](../digging-deeper/http-client.md)與[佇列](../digging-deeper/queues.md)。待重試的投遞到期後，可再次喚醒佇列任務或使用有界掃描。

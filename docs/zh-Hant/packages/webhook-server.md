# Webhook 發送（擴展）

## 支援狀態 {#support-status}

可選套件 `openrust-webhook-server` 提供請求快照、目的地驗證、具名簽名器、退避策略及持久投遞嘗試。支援具名版本化端點。

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

struct StopOnGone;

impl openrust_webhook_server::CallWebhookJob for StopOnGone {
    fn should_be_removed_from_queue(
        &self,
        attempt: &openrust_webhook_server::WebhookJobAttempt<'_>,
    ) -> bool {
        attempt.get_response().is_some_and(|response| response.status().as_u16() == 410)
    }
}

pub fn register_delivery_job(
    container: &openrust::container::Container,
    service: &openrust_webhook_server::DeliveryService,
    queue: &openrust::queue::QueueManager,
) -> Result<()> {
    openrust_webhook_server::register_call_webhook_job(
        container, "stop-on-gone", std::sync::Arc::new(StopOnGone),
    )?;
    service.register_worker(queue, "stop-on-gone")
}

pub fn register_application_replay_policy(
    container: &openrust::container::Container,
    policy: std::sync::Arc<dyn openrust_webhook_server::ReplayPolicy>,
) -> Result<()> {
    openrust_webhook_server::register_replay_policy(container, policy)
}

pub async fn prepare_endpoint_invoice(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::PreparedWebhook> {
    let endpoint = store.endpoints().find("billing").await?
        .ok_or(openrust_webhook_server::Error::EndpointRevoked)?;
    invoice_call()?.use_endpoint(&endpoint)
        .prepare(&openrust_webhook_server::TargetPolicy::default())
}

pub fn configure_webhook_limits(container: &openrust::container::Container) -> Result<()> {
    use openrust_webhook_server::{DeliveryLimit, DeliveryLimits, register_delivery_limits};

    register_delivery_limits(container, DeliveryLimits {
        sends_per_scope: Some(DeliveryLimit::new(100, 60)?),
        sends_per_endpoint: Some(DeliveryLimit::new(20, 60)?),
        replays_per_scope: Some(DeliveryLimit::new(5, 60)?),
    })
}

pub async fn publish_invoice(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::PublishedMessage> {
    use openrust_webhook_server::{EventType, PublishOptions, TargetPolicy};

    store.publish(
        "invoice-paid-inv-123",
        &EventType::new("invoice.paid", 1)?,
        serde_json::json!({"invoice_id": "inv-123"}),
        &PublishOptions::default(),
        &TargetPolicy::default(),
    ).await
}

pub fn grant_invoice_tls(
    policy: openrust_webhook_server::TargetPolicy,
    ca: &[u8],
    client_chain: &[u8],
    client_key: &[u8],
) -> Result<openrust_webhook_server::DeliveryPermission> {
    openrust_webhook_server::DeliveryPermission::new(policy)
        .with_tls_ca(ca)?
        .with_mutual_tls(client_chain, client_key)
}


pub fn grant_invoice_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
) -> Result<openrust_webhook_server::DeliveryPermission> {
    let proxy = openrust::http::client::PinnedSocksProxy::new(address)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}


pub fn grant_invoice_connect_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
) -> Result<openrust_webhook_server::DeliveryPermission> {
    let proxy = openrust::http::client::PinnedHttpProxy::new(address)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}

pub fn grant_invoice_https_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
    server_name: &str,
    ca_pem: &[u8],
) -> Result<openrust_webhook_server::DeliveryPermission> {
    use openrust::http::client::{PinnedHttpProxy, TlsCertificate};
    let roots = TlsCertificate::from_pem_bundle(ca_pem)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    let proxy = PinnedHttpProxy::https(address, server_name, roots)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}

pub async fn inspect_invoice_attempts(
    store: &openrust_webhook_server::DeliveryStore,
    id: &str,
    after: u32,
) -> Result<openrust_webhook_server::DeliveryAttemptsPage> {
    store.inspect_attempts(id, after, 25).await
}


pub async fn preview_invoice_pruning(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::DeliveryPruneReport> {
    store.prune(openrust_webhook_server::DeliveryRetention::Payloads, 30, 100, true).await
}

pub async fn preview_shared_body_pruning(
    store: &openrust_webhook_server::DeliveryStore,
    after: Option<&str>,
) -> Result<openrust_webhook_server::PublicationPruneReport> {
    store.prune_publication_bodies(30, 100, after, true).await
}

pub async fn hold_invoice_delivery(
    store: &openrust_webhook_server::DeliveryStore,
    id: &str,
) -> Result<bool> {
    store.set_retention_hold(openrust_webhook_server::RetentionRecord::Delivery, id, true).await
}

pub async fn preview_expiring_invoice_keys(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::IdempotencyPruneReport> {
    store.prune_idempotency_keys(openrust_webhook_server::IdempotencyKind::Delivery, 30, 100, true).await
}

pub async fn preview_invoice_history_pruning(
    store: &openrust_webhook_server::DeliveryStore,
    after: Option<&str>,
) -> Result<openrust_webhook_server::HistoryPruneReport> {
    store.prune_history(openrust_webhook_server::IdempotencyKind::Delivery, 90, 100, after, true).await
}

pub async fn preview_invoice_replay_audit_pruning(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::ReplayAuditPruneReport> {
    store.prune_replay_audit(90, 100, true).await
}

```


### 準備對外請求 {#prepare}

在已啟動的應用中呼叫 `WebhookCall::create()` 讀取套件設定，或使用 `WebhookCall::new()` 取得內建預設。設定目的地、載荷與憑據，再呼叫 `prepare(&TargetPolicy::default())`。快照保存穩定訊息 ID、已序列化載荷與排程設定。`headers_at` 按試行時間戳為已保存的位元組產生簽名頭。憑據由應用在執行時解析的場景可傳入具名憑據引用。

POST 等正文方法使用已準備的位元組。GET 將載荷編碼為 RFC 3986 查詢字串，Spatie 簽名繼續使用已準備的 JSON。原始正文模式可保留任意位元組。快照限制正文為 16 MiB、Header 為 64 KiB。透過 `register_signer` 和 `register_backoff_strategy` 將外部實作註冊到應用容器。

預設目的地策略要求 HTTPS，並重新核對解析所得的全部 Socket 位址。內部端點使用應用可信設定提供固定來源、路徑及位址。`PreparedWebhook::bind_target` 可驗證單次試行的位址清單。HTTP 用戶端固定實際連線位址，保留 Host 與 SNI，禁止重新導向，預設直連並驗證 HTTPS 憑證。

### 設定 TLS {#tls}

使用私有 CA 或雙向 TLS 時，應用程式的 `DeliveryPolicy` 先授權範圍和具體目的地，再透過 `DeliveryPermission::with_tls_ca(&pem_bundle)` 為該對端取代系統信任庫，或透過 `with_mutual_tls(&certificate_chain, &private_key)` 提供用戶端身分。Worker 從可信應用設定讀取 PEM 材料，私鑰由應用程式在執行時解密。佇列、投遞快照與事件均不包含 PEM 材料。連線保留目的地主機名稱供 SNI 與憑證驗證使用。

隔離測試中，呼叫 `do_not_verify_ssl()` 且投遞策略同時回傳 `allow_unverified_tls()` 時，才停用伺服器憑證與主機名稱驗證。未得到策略許可、指定自訂 CA 或附帶用戶端憑證時，該請求會被拒絕。呼叫 `verify_ssl(true)` 可恢復驗證。

### 使用具名出站代理 {#proxy}

在 `webhook-server.proxy` 中設定已核准的名稱，例如 `corp-egress`，也可為單次投遞呼叫 `use_proxy(Some("corp-egress".into()))`。呼叫 `use_proxy(None)` 可選擇直連。應用程式策略授權投遞範圍及目的地後，以 `DeliveryPermission::with_proxy` 核准相同名稱。可覆寫 `DeliveryPolicy::authorize_transport` 讀取所請求的名稱。投遞記錄只儲存名稱。使用可信設定中的數字 Socket 位址建立代理，並在執行時提供選用憑證。

可用 `PinnedSocksProxy` 進行本機解析的 SOCKS5 轉送。使用 CONNECT 時，可用 `PinnedHttpProxy::https` 指定代理的數字位址、憑證名稱和可信 CA 憑證。Worker 分別驗證代理和收件端，收件端的 mTLS 身分只用於收件端。代理收到已驗證的目標 IP 和連接埠，收件端 TLS 和 HTTP 使用原主機。使用 `PinnedHttpProxy::new` 建立明文 HTTP CONNECT 時，鏈路上的代理認證資料未加密，應部署在可信網路。缺少許可或代理連線錯誤時，投遞不回退為直連。目前不支援 SOCKS5 遠端 DNS 或向代理提供 TLS 用戶端憑證。

### 登記與投遞 {#delivery}

啟用套件後執行 `./artisan migrate`。用應用資料庫連線和投遞範圍建構 `DeliveryStore`。業務變更需要與投遞登記原子提交時，使用同庫的 `submit_in_transaction`。個別登記則呼叫 `submit`。同一鍵與相同快照會回傳既有投遞 ID，同一鍵搭配不同快照會回傳衝突。需要重複登記時，先為快照指定穩定的訊息 ID。投遞 ID 和發布訊息 ID 皆以回傳值為準。同一範圍的鍵會繼續回傳原結果，也適用於冪等對應遷移之前的記錄。部署新寫入路徑前先執行遷移，並停止舊版本寫入程序。

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

### 自訂發送 Job {#custom-job}

實作 `CallWebhookJob`，在生產者、Scheduler 和 Worker 用 `register_call_webhook_job` 與 `DeliveryService::register_worker` 註冊同一名稱，再用 `.use_job("stop-on-gone")` 選用。`should_be_removed_from_queue` 可停止原投遞預算允許的重試。`failed` 在一次嘗試提交最終失敗並派發結果事件後執行。兩個鉤子都應快速同步回傳。

鉤子可以讀取原始本文、URL、標籤、中繼資料及有大小上限的原生回應。傳輸、憑據解析、嘗試記錄與持久狀態由套件統一管理。已保存的自訂投遞在執行時要求存在同名鉤子，缺失時撤銷領取且不扣減嘗試額度。啟用這類投遞前，在所有消費者上完成註冊。

### 人工重放投遞 {#replay}

使用重放前，在應用中註冊 `ReplayPolicy`。核實實際操作者身分及其對指定範圍和端點的權限。應用可用 `ReplayPhase::Request` 實施操作限流，並在 `ReplayPhase::Attempt` 重新核對執行許可。

重放登記一筆帶有稽核記錄及獨立嘗試額度和截止時間的投遞。它沿用原訊息 ID、本文及目的地，可依授權改用具名簽名憑據。調度器透過同一 Worker 流程發送。預設只重放最終失敗的投遞。已成功的原投遞須明確傳入 `--include-succeeded`，並取得更高權限。命令報告兩筆投遞的 ID 和新代次。

```sh
./artisan webhook-server:replay <delivery-id> --scope=billing --actor=operator-42 --reason="confirmed receiver recovery"
```

### 管理端點版本 {#endpoints}

在應用授權的操作中使用 `DeliveryStore::endpoints()`，依名稱、URL 和選用憑據引用登記端點。使用 `.use_endpoint(&endpoint)` 綁定發送。`replace`、`pause`、`resume`、`delete` 需要傳入預期版本，以拒絕過期的管理操作。

暫停期間待處理投遞保留原截止時間，恢復後符合條件的投遞繼續。修改 URL 或憑據會建立新版本，供新投遞使用。舊版本與已刪除端點的投遞會在執行時取消。已送出的請求仍可能在對端完成，嘗試歷史保留實際回應。人工重放也會核對端點目前許可。直接指定 URL 的發送沿用應用投遞策略。

### 共用投遞限額 {#limits}

在各節點的應用中註冊 `DeliveryLimits`。範圍發送限額涵蓋直接 URL、佇列投遞、自動重試和人工重放發送。具名端點另外獨立計數，同一端點的各設定版本共用。人工重放登記可另設範圍限額。視窗自首次保留額度起，依資料庫時鐘計算。

發送達到限額時，投遞等待視窗恢復，保留原嘗試額度及截止時間。重放登記達到限額時會回傳下一次可申請的 Unix 時間戳錯誤。共用投遞資料庫的所有節點應使用一致的限額設定。

### 查詢投遞與嘗試 {#inspect}

使用 `./artisan webhook-server:inspect <id> --scope=<scope>` 查詢投遞 ID。查詢 `publish` 回傳的內部訊息 ID 時加入 `--message`。加入 `--attempts --limit=25` 讀取一頁嘗試，下一頁透過 `--after` 傳入回傳的 `next_after`。時間欄位以 Unix 秒表示。使用 `--database` 選擇具名資料庫連線。

查詢回傳狀態、時間、HTTP 狀態碼與有限的嘗試結果。已發布訊息的摘要另包含事件類型、schema 版本和首次投遞 ID。套件不註冊診斷 HTTP 路由，應用程式授權範圍後可呼叫 `DeliveryStore` 的唯讀方法。Artisan 允許選擇資料庫及範圍，僅交由可信維運人員。診斷輸出不包含請求 URL、本文、Header、簽章材料或遠端回應本文。

### 清理出站投遞資料 {#prune}

先以 `./artisan webhook-server:prune --scope=<scope> --days=30 --pretend` 預演候選記錄。移除 `--pretend` 後清理已儲存的請求快照，保留投遞 ID、狀態及冪等摘要。從投遞截止時間起算保留期。加入 `--attempts` 可個別清理已完成的嘗試，保留期按完成時間計算。每批最多 500 筆，預設 100 筆。使用 `--database` 選擇具名連線。

啟用清理前先執行遷移，並升級所有可登記重放的程序。僅清理終態投遞。清理後，原始記錄無法再登記新重放，既有重放仍可使用自身儲存的請求。診斷結果以 Unix 秒回傳 `payload_pruned_at`。

使用 `--messages` 回收達到發布留存期限的共享本文。首次投遞及所有已登記重放的請求快照都已清理後，該本文才可回收。下一頁透過 `--after` 傳入回傳的 `next_after`，下一輪從頭檢查先前未符合回收條件的訊息。訊息 ID、原始投遞 ID 與冪等摘要繼續保留。`webhook-server:inspect --message` 會在清理後顯示 `body_pruned_at`。

### 設定出站業務保留 {#retention-hold}

使用 `./artisan webhook-server:hold <id> --scope=<scope>` 保留一筆投遞的快照和嘗試。加入 `--message` 可按發布訊息 ID 保留共享本文，加入 `--release` 可解除保留。訊息保留只保護共享本文，投遞保留也會保護其現存快照引用的共享本文。解除保留後仍須達到一般留存期限才會清理。

已設定保留的記錄在診斷中顯示 `retention_hold_at`。保留只保護仍存在的資料。使用保留前先執行遷移，並升級所有執行清理的程序。應用程式授權範圍後可呼叫 `DeliveryStore::set_retention_hold`。Artisan 指令僅供可信維運人員使用。

### 釋放到期冪等鍵 {#idempotency-expiry}

使用 `./artisan webhook-server:prune --scope=<scope> --days=30 --idempotency=delivery --pretend` 預演直接投遞鍵到期。發布訊息使用 `--idempotency=publication`。期限從請求快照或共享本文清理時起算，業務保留會阻止到期。移除 `--pretend` 後釋放符合條件的鍵。啟用前先執行遷移，並升級所有寫入及清理程序。

重用已釋放的鍵會產生新記錄和 ID，舊佇列任務仍指向舊投遞。歷史記錄保留原 ID，並在診斷中顯示 `idempotency_expired_at`。若接收方應將續用鍵的直接投遞視為新事件，應用程式需設定新的業務訊息 ID。

### 清理歷史中繼資料 {#history-retention}

使用 `./artisan webhook-server:prune --scope=<scope> --days=90 --history=delivery --pretend` 預演歷史投遞清理。發布收據使用 `--history=publication`。期限從冪等鍵到期起算，使用回傳的 `next_after` 與 `--after` 繼續分頁。下一輪從頭重新檢查先前未符合清理條件的記錄。

投遞歷史只有在嘗試均已清理、重放稽核與現存共享本文皆未引用時才可刪除。發布收據需先清理本文並釋放冪等鍵。業務保留會阻止兩類清理。歷史中繼資料刪除後，診斷無法再查詢該 ID。

### 清理重放稽核 {#replay-retention}

使用 `./artisan webhook-server:prune --scope=<scope> --days=90 --replay-audit --pretend` 預演稽核清理，移除 `--pretend` 後執行。兩筆投遞均須已清理請求快照，且各自的冪等鍵到期均已達指定留存期限，同時兩筆記錄都沒有業務保留。每批最多清理 500 筆稽核。

此操作清理重放關係與操作者中繼資料。原投遞和重放投遞仍依各自的歷史清理規則回收。原快照已清理時仍無法登記新的重放。

### 依訂閱發布 {#subscriptions}

用 `DeliveryStore::subscribe` 為具名端點訂閱確切事件名與版本。用 `unsubscribe` 取消之後的訂閱。發布時提交 JSON 載荷，本文包含 `type`、`schema_version` 和 `data`。訊息、共用本文和當次訂閱端點的所有投遞在同一事務提交。`publish_in_transaction` 可將發布納入同庫業務事務。

單次發布支援最多 32 個相符端點與 1 MiB 的訊息本文。相同範圍、鍵與輸入重複提交時回傳原收據，包括零投遞的結果。同一鍵的輸入發生變更時回傳衝突。各端點與重放沿用同一共用本文與訊息 ID。事務提交後由既有調度器恢復派發。

## 自訂簽名與退避 {#customization}

外部 crate 可實作 `Signer`。`SigningContext` 提供不可變本文、URL、執行時憑據、穩定訊息 ID 和單次嘗試時間戳，這是上游簽名參數的 Rust 適配。金鑰以借用傳入，上下文不支援序列化或 Debug 輸出。`headers` 方法支援協定附加的認證頭。不要把金鑰加入請求中繼資料、日誌或一般佇列載荷。

`BackoffStrategy::wait_in_seconds_after_attempt` 回傳秒數。預設 `ExponentialBackoffStrategy` 沿固定基線：嘗試1至4分別等待10、100、1000、10000秒，超過4等待100000秒。底層契約的0回傳1秒，首個實際嘗試編號為1。


## 使用邊界 {#scope}

網路和任務執行重用既有 [HTTP 用戶端](../digging-deeper/http-client.md)與[佇列](../digging-deeper/queues.md)。待重試的投遞到期後，可再次喚醒佇列任務或使用有界掃描。

### 單機與多副本 {#deployment}

多個副本使用同一個可寫入的 PostgreSQL 或 MySQL 資料庫儲存投遞狀態和限額，各 Worker 使用同一套 database 或 Redis 佇列。所有 Worker 都須能讀取憑據參照和已授權的設定版本。復原任務使用 `on_one_server()` 時，需要共用 database 或 Redis 快取。SQLite 適用於單機。上述 database 與 Redis 組合已在一台主機的獨立應用行程中驗收，跨主機部署前須驗證實際基礎設施。

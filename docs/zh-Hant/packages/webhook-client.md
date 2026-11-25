# Webhook 接收（擴展）

## 支援狀態 {#support-status}

可選套件 `openrust-webhook-client` 提供具名設定、簽名驗證器、手動 `WebhookProcessor` 與按範圍隔離的資料庫接收記錄。應用提供任務派發實作。自動回呼路由、解析後的載荷／附件、去重與恢復尚未提供。儲存記錄及派發成功不代表業務完成，套件不會自動安裝公開回呼路由。

## 啟用與設定 {#configuration}

將 `webhook-client` 加入根應用 `[features].default`，保留既有 feature，套件發現會註冊 `WebhookClientServiceProvider`。其他應用也可直接依賴 crate 並註冊該 Provider。註冊與啟動不查詢資料庫、不聯繫供應商。

預設設定名為 `default`，簽名頭為 `Signature`，憑據引用指向 `WEBHOOK_CLIENT_SECRET`。透過程序環境或受保護的應用 `.env` 提供金鑰；套件設定只儲存環境變數名稱，不儲存解析後的金鑰。自訂時發布範本，在 `config/mod.rs` 增加 `mod webhook_client;` 與 `("webhook-client".to_owned(), webhook_client::load(env)?),`，再重新編譯。重複發布保留自訂，明確指定 `--force` 才覆寫。

```sh
./artisan vendor:publish --tag=webhook-client-config
```

## 可用入口 {#available-entry-points}

### 驗證原始位元組 {#signatures}

讀取 HTTP 串流時先依設定限制本文大小，再建立 `WebhookRequest`。保留原始本文和請求頭值。預設驗證器使用 HMAC-SHA256，要求完全一致的小寫十六進位簽名；不簽 URL 或時間戳，不提供抗重放時間窗。重複簽名頭會被拒絕。

本文預設上限為1 MiB，`with_max_body_bytes` 最多允許16 MiB。請求快照最多接受128個頭值，頭名和值合計不超過64 KiB。設定來源最多128個設定，名稱與修訂標籤最多128位元組，單個金鑰最多8192位元組。HTTP 傳輸層還可能有更低的限制。

自訂驗證器若簽名包含請求目標，使用 `WebhookRequest::with_uri` 附上原始 `Request::uri()`，保留查詢字串編碼。

```rust
use openrust::{foundation::Application, http::{HeaderMap, Method}};
use openrust_webhook_client::{
    DefaultSignatureValidator, Error, SignatureValidator,
    WebhookConfigRepository, WebhookRequest,
};

pub async fn verify_signature(
    app: &Application,
    headers: HeaderMap,
    original_body: Vec<u8>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let configs = app.make::<WebhookConfigRepository>().await?;
    let config = configs.get_config("default")?
        .ok_or(Error::InvalidConfig("name"))?;
    let request = WebhookRequest::new(Method::POST, headers, original_body)?;
    Ok(DefaultSignatureValidator.is_valid(&request, &config)?)
}

pub async fn receive(
    request: &WebhookRequest,
    config: &openrust_webhook_client::WebhookConfig,
    connection: openrust::database::Connection,
    scope: &str,
    job: std::sync::Arc<dyn openrust_webhook_client::WebhookJobDispatcher>,
    events: std::sync::Arc<dyn openrust::contracts::events::Dispatcher>,
) -> openrust_webhook_client::Result<openrust::http::Response> {
    use openrust_webhook_client::{DatabaseWebhookStore, WebhookProcessor, WebhookRuntime};
    let store = std::sync::Arc::new(DatabaseWebhookStore::new(connection, scope)?);
    let runtime = WebhookRuntime::new(store, job, events);
    WebhookProcessor::new(request, config, &runtime).process().await
}
```

`StandardSignatureValidator` 重用官方 `standardwebhooks 1.3.0`，使用 `webhook-id`、`webhook-timestamp`、`webhook-signature`。金鑰為合法 base64 或帶 `whsec_` 前綴的形式。支援五分鐘時間窗與單個頭內的多個簽名，拒絕重複協定頭。該函式庫要求 UTF-8 本文；驗簽不取代持久事件去重。

## 自訂實作與快照 {#customization}

外部 crate 可實作 `SignatureValidator`、`WebhookProfile`、`RespondsToWebhook`、`WebhookStore` 與 `WebhookJobDispatcher`。`ProcessEverythingWebhookProfile` 選擇所有請求，`DefaultRespondsTo` 產生 `{"message":"ok"}`。透過應用的儲存、任務派發器與事件派發器建構 `WebhookRuntime`，用其 Builder 方法選擇驗證器、Profile 與回應；設定中的類別名稱欄位尚不會自動解析這些實作。

`WebhookConfigRepository::add_config` 依名稱替換設定，已取得的 `Arc<WebhookConfig>` 保留原修訂與金鑰快照。未知名稱回傳 `Ok(None)`，解析錯誤仍為錯誤。Provider 將儲存庫綁定到目前 HTTP、Artisan 或 Worker 生命週期，執行時追加不會洩漏到下一生命週期。`WebhookConfig::new` 接受執行時注入的金鑰，私有儲存並對 Debug 脫敏；不要把金鑰序列化到任務或日誌。`with_verification_secrets` 最多增加七個舊金鑰，同一設定總共最多八個。

設定快取儲存憑據引用，因此套件預設值不會讓 `config:show webhook-client` 輸出解析後的金鑰。各程序依自己的不可變環境快照解析設定；變更憑據後重啟常駐程序。跨程序重啟與排隊任務所需的歷史憑據保留尚未實作，`revision` 標籤不能取代持久憑據修訂庫。

### 儲存與派發接收記錄 {#receipts}

Provider 註冊 `webhook_calls` 遷移，使用 `DatabaseWebhookStore` 前執行 `./artisan migrate`。傳入最多128位元組的穩定應用／租戶範圍，同一範圍的各節點使用相同值。讀取與異常更新按該範圍隔離，摘要鍵避免資料庫排序規則混同大小寫不同的範圍。這是儲存隔離，不是呼叫者授權；範圍必須從可信應用上下文取得。

`WebhookProcessor::process` 依序驗簽、Profile 篩選、儲存、清除舊異常、派發任務、產生回應。無效簽名先派發強型別 `InvalidWebhookSignatureEvent` 再回傳錯誤；事件讓應用內可信監聽器讀取請求與設定，不自動提供可序列化的診斷屬性。驗證器報錯仍是錯誤。有效但忽略的請求只產生約定回應。儲存或派發失敗不會回傳成功回應；派發錯誤儲存有界的通用異常記錄，不儲存任意第三方錯誤文字或堆疊。

`models::WebhookCall` 的 `store_webhook`、`find`、`save_exception` 與 `clear_exception` 委託儲存適配。記錄保留原始正文、`with_uri` 提供的請求目標、方法、修訂與選擇的請求頭，包括重複頭和非 UTF-8 頭值。儲存記錄本身不是驗證令牌；`VerifiedWebhook` 僅由 Processor 在驗證及 Profile 通過後產生。資料庫對原始位元組使用 base64，這不是加密；應用仍需控制資料庫存取與保留期限。

預設不儲存請求頭。`with_store_headers(&["X-Source"])` 不區分頭名大小寫，`with_store_headers(&["*"])` 明確儲存全部頭，可能包含敏感值。設定檔的全選值使用字串 `"*"`，不能寫成包含 `"*"` 的陣列。異常 JSON 最多8192位元組，請求目標最多8192位元組，持久方法名最多32位元組；手動傳入的異常詳情須由應用脫敏。

應用實作的 `WebhookJobDispatcher` 應僅把記錄 ID 與可信範圍入列，在 Worker 中重新讀取記錄，不序列化金鑰或整個模型。已接收的排隊記錄不應因簽名時間窗過期而重新驗簽。本入口尚未補齊提交後派發崩潰窗口、請求去重、處理租約與自動註冊的業務 Job，使用時須保留這些限制。

## 使用邊界 {#scope}

應用網路與任務執行複用既有 [HTTP 用戶端](../digging-deeper/http-client.md)與[佇列](../digging-deeper/queues.md)。持久恢復、form／附件解析、公開路由／控制器及完整投遞狀態機仍未實作。

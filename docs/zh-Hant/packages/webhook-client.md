# Webhook 接收（擴展）

## 支援狀態 {#support-status}

可選套件 `openrust-webhook-client` 提供具名設定、型別化實作工廠、明確回呼路由、`WebhookController`、手動 `WebhookProcessor` 與按範圍隔離的資料庫接收記錄。應用提供任務派發實作。去重、恢復及 Storage 磁碟引用尚未提供。儲存記錄及派發成功不代表業務完成，套件不會自動安裝公開回呼路由。

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

pub fn register_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    use openrust_webhook_client::{WebhookRouteOptions, WebhookRoutes};
    Route::webhooks("/callbacks/payments", WebhookRouteOptions::new().name("payments"))
}

pub fn register_dispatcher(
    container: &openrust::container::Container,
    dispatcher: std::sync::Arc<dyn openrust_webhook_client::WebhookJobDispatcher>,
) -> openrust_webhook_client::Result<()> {
    openrust_webhook_client::WebhookRuntime::register_job_dispatcher(
        container, "payments", move |_, _| Ok(dispatcher.clone()),
    )
}

pub async fn process_named(
    app: std::sync::Arc<Application>,
    config: std::sync::Arc<openrust_webhook_client::WebhookConfig>,
    request: &WebhookRequest,
) -> openrust_webhook_client::Result<openrust::http::Response> {
    use openrust_webhook_client::{WebhookProcessor, WebhookRuntime};
    let owner = app.clone();
    app.call(openrust::container::Callback::new(&[], move |_| async move {
        let runtime = WebhookRuntime::from_application(owner, config.clone()).await;
        Ok(match runtime {
            Ok(runtime) => WebhookProcessor::new(request, &config, &runtime).process().await,
            Err(error) => Err(error),
        })
    }), Default::default()).await?
}
```

`StandardSignatureValidator` 重用官方 `standardwebhooks 1.3.0`，使用 `webhook-id`、`webhook-timestamp`、`webhook-signature`。金鑰為合法 base64 或帶 `whsec_` 前綴的形式。支援五分鐘時間窗與單個頭內的多個簽名，拒絕重複協定頭。該函式庫要求 UTF-8 本文；驗簽不取代持久事件去重。

### 註冊回呼路由與實作 {#routes}

匯入 `WebhookRoutes` 後呼叫 `Route::webhooks(uri, options)` 或 `Route::webhooks_with(uri, options, configure)`。`WebhookRouteOptions::default()` 使用 `default` 設定及 `post` 方法，可用 `name`、`methods` Builder 指定其他設定與方法清單。註冊參數只接受小寫 `get`、`query`、`post`、`put`、`patch`、`delete`；GET 繼承原生 HEAD 比對，但不能把 `head` 作為註冊選項。大寫或不支援的方法使註冊失敗；原生 OPTIONS 協商及405／Allow行為保留。

在應用既有 API 或 web 路由登記中註冊。匯入 `WebhookRouter` 後，底層 `Router` 也提供 `webhooks` 擴展，應在應用路由登記期間呼叫。群組前綴、名稱與中介軟體保留，`webhooks_with` 可設定原生參數約束、預設值及路由中介軟體。套件不會關閉 CSRF；供應商使用 web 路由時，僅在應用請求偽造防護設定中明確排除該回呼路徑，其他 web 路由繼續受保護。

設定從路由器實際比對的路由名選擇，不跟隨請求的自訂路由解析器，也不由 URL／正文裡傳入的帳號選擇。預設名稱為 `webhook-client-{name}`，`add_unique_token_to_route_name=true` 追加八位隨機後綴；這是命名機制，不是鑑權。與固定原始碼一致，唯一令牌模式按第一個點分隔名稱，因此該模式下應使用不含點的設定名。

在應用 Provider 中呼叫 `WebhookRuntime::register_signature_validator`、`register_webhook_profile`、`register_webhook_response`、`register_webhook_store` 或 `register_job_dispatcher`。工廠接收不可變 `WebhookConfig` 與目前應用 `Container`，回傳對應 trait 的 `Arc`。`WebhookRuntime::from_application` 需要 await；建構與處理使用指定應用的原生上下文，結束後恢復呼叫者。HTTP 控制器讓請求依賴工廠與處理共用一次請求借用，請求句柄不能帶出該借用。註冊名區分大小寫，最多128位元組，只允許 ASCII 字母、數字及 `-`、`_`、`.`、`:`。未知或型別錯誤的綁定會失敗，不回退到其他應用或帳號。

設定欄位依序為 `signature_validator`、`webhook_profile`、`webhook_response`、`webhook_model`、`process_webhook_job`。內建驗證器為 `default`／`standard`，Profile 為 `process-everything`，回應為 `default`，儲存為 `database`。任務派發器必須明確註冊；目前 `process_webhook_job` 選擇派發器，原生佇列處理可用 `QueuedWebhookJob`。資料庫設定還需要可信穩定 `scope`，可選 `connection` 為資料庫連線名，`null` 使用應用預設連線。發布範本保留空 scope 與空任務名，未設定的接收端會報錯，不會假確認。 必須明確選擇 `signature_validator`、`webhook_profile`、`webhook_model`、`process_webhook_job`，缺失欄位會被拒絕；只有 `webhook_response` 可省略或為 null，並使用預設回應。

例如具名 `payments` 設定可使用 `scope="billing/site-a"`、`signing_secret={"env":"PAYMENT_WEBHOOK_SECRET"}`、內建預設驗證器／Profile／回應、database 儲存及 `process_webhook_job="payments"`。將這些欄位寫入發布後的設定，並在 Provider 註冊 payments 派發器。手動消費者也可透過 `WebhookConfig` 的 `with_signature_validator`、`with_webhook_profile`、`with_webhook_response`、`with_webhook_model`、`with_process_webhook_job`、`with_scope`、`with_connection` Builder 建立設定。

驗證器、Profile 與回應在處理前建構；僅在驗簽及 Profile 通過後解析儲存，有效但忽略的事件不存取儲存。任務工廠先檢查註冊，儲存後、清除舊異常前才建構派發器。缺失設定與無效簽名是普通錯誤，預設控制器按固定原始碼回傳500；應用可用[異常處理](../the-basics/errors.md)自行對應。正文超限保留413。前面直接呼叫驗證器的範例仍可自行決定回應策略。

控制器複用原生 `Request::body_bytes_with_limit`，保留原文，即使中介軟體已解析 JSON／form，也會檢查已快取正文是否超過端點上限；該方法不提高原生8 MiB讀取上限。前置中介軟體可能已經按自己的上限讀取正文，較小端點限制不能倒退減少此前的記憶體使用。

## 自訂實作與快照 {#customization}

外部 crate 可實作 `SignatureValidator`、`WebhookProfile`、`RespondsToWebhook`、`WebhookStore` 與 `WebhookJobDispatcher`。`ProcessEverythingWebhookProfile` 選擇所有請求，`DefaultRespondsTo` 產生 `{"message":"ok"}`。`WebhookRuntime::new` 接受明確依賴；`WebhookRuntime::from_application` 按目前應用的型別化工廠解析選定名稱，不執行類別名稱字串，也不使用全域註冊表。

`WebhookConfigRepository::add_config` 依名稱替換設定，已取得的 `Arc<WebhookConfig>` 保留原修訂與金鑰快照。未知名稱回傳 `Ok(None)`，解析錯誤仍為錯誤。Provider 將儲存庫綁定到目前 HTTP、Artisan 或 Worker 生命週期，執行時追加不會洩漏到下一生命週期。`WebhookConfig::new` 接受執行時注入的金鑰，私有儲存並對 Debug 脫敏；不要把金鑰序列化到任務或日誌。`with_verification_secrets` 最多增加七個舊金鑰，同一設定總共最多八個。

設定快取儲存憑據引用，因此套件預設值不會讓 `config:show webhook-client` 輸出解析後的金鑰。各程序依自己的不可變環境快照解析設定；變更憑據後重啟常駐程序。跨程序重啟與排隊任務所需的歷史憑據保留尚未實作，`revision` 標籤不能取代持久憑據修訂庫。

### 儲存與派發接收記錄 {#receipts}

資料庫儲存僅在驗簽及 Profile 通過後保存解析輸入。用 `call.payload(&store).await?` 讀取 JSON、表單及查詢參數，同名欄位以正文為準。升級前儲存的原文記錄回傳 `None`。用 `call.get_attachments(&store).await?` 讀取從持久原文重建的原生上傳物件群組，資料庫不儲存伺服器上傳路徑。`store_attachments=false` 關閉附件檢視，原始 multipart 正文仍包含檔案位元組。

Provider 註冊 `webhook_calls` 遷移，使用 `DatabaseWebhookStore` 前執行 `./artisan migrate`。傳入最多128位元組的穩定應用／租戶範圍，同一範圍的各節點使用相同值。讀取與異常更新按該範圍隔離，摘要鍵避免資料庫排序規則混同大小寫不同的範圍。這是儲存隔離，不是呼叫者授權；範圍必須從可信應用上下文取得。

`WebhookProcessor::process` 依序驗簽、Profile 篩選、儲存、清除舊異常、派發任務、產生回應。無效簽名先派發強型別 `InvalidWebhookSignatureEvent` 再回傳錯誤；事件讓應用內可信監聽器讀取請求與設定，不自動提供可序列化的診斷屬性。驗證器報錯仍是錯誤。有效但忽略的請求只產生約定回應。儲存或派發失敗不會回傳成功回應；派發錯誤儲存有界的通用異常記錄，不儲存任意第三方錯誤文字或堆疊。

`models::WebhookCall` 的 `store_webhook`、`find`、`save_exception` 與 `clear_exception` 委託儲存適配。記錄保留原始正文、`with_uri` 提供的請求目標、方法、修訂與選擇的請求頭，包括重複頭和非 UTF-8 頭值。儲存記錄本身不是驗證令牌；`VerifiedWebhook` 僅由 Processor 在驗證及 Profile 通過後產生。資料庫對原始位元組使用 base64，這不是加密；應用仍需控制資料庫存取與保留期限。

預設不儲存請求頭。`with_store_headers(&["X-Source"])` 不區分頭名大小寫，`with_store_headers(&["*"])` 明確儲存全部頭，可能包含敏感值。設定檔的全選值使用字串 `"*"`，不能寫成包含 `"*"` 的陣列。異常 JSON 最多8192位元組，請求目標最多8192位元組，持久方法名最多32位元組；手動傳入的異常詳情須由應用脫敏。

### 在 Worker 中處理接收記錄 {#processing-jobs}

實作 `jobs::ProcessWebhookJob` 處理已儲存的接收記錄。在 HTTP 和 Worker 共用的應用 Provider 中，呼叫 `QueuedWebhookJob::register`，傳入原生 `QueueManager`、穩定名稱、可信範圍的儲存及處理器。再按 `process_webhook_job` 選擇的名稱註冊 `QueuedWebhookJob` 派發器。不同儲存範圍使用各自的註冊名。Worker 對照原記錄核對 ID、範圍、設定名和修訂號後才呼叫處理器。

派發器可指定佇列連線和名稱，原生 `JobOptions` 控制試行次數、退避及逾時。業務策略由處理器決定。佇列載荷儲存接收記錄識別資料，Worker 直接讀取已驗收的記錄，無需接收方的驗簽金鑰。處理器失敗時記錄固定異常，下次試行清除上次診斷。跨後端派發恢復及冪等仍待完成。請選擇由 Worker 消費的連線。派發器拒絕內建 sync、null、deferred 和 background 連線，並要求佇列回傳已入列結果。

### 同一事務儲存接收記錄與資料庫佇列任務 {#atomic-database-queue}

在設定中設定 `atomic_database_queue: true`，或在執行時呼叫 `with_atomic_database_queue(true)`。讓 `DatabaseWebhookStore` 和 `QueuedWebhookJob` 使用同一應用資料庫連線。接收記錄與佇列任務共同提交後才回傳 HTTP 確認，佇列寫入失敗時一併回滾。其他驅動或分開的連線實例會被拒絕，即使指向同一資料庫。該選項預設關閉，啟用後涵蓋入列前的程序退出窗口。重試 HTTP 請求仍可能建立獨立接收記錄。

### 多 Worker 領取接收記錄 {#processing-lease}

先執行 `./artisan migrate`，再用 `QueuedWebhookJob::register_with_database_lease(&queue, name, store, handler, lease_seconds)` 註冊處理器。儲存參數為 `Arc<DatabaseWebhookStore>`。協調重新啟動消費該佇列的各個 Worker，使其採用相同註冊。例如任務逾時 20 秒、租約 30 秒、資料庫佇列 `retry_after` 60 秒。租約範圍為 3–86,400 秒，任務必須明確設定正數逾時，且至少比租約短兩秒。應為資料庫操作保留餘裕，並讓佇列保留時間或可見性逾時長於租約。競爭中的任務透過原生佇列釋放，可能耗用一次佇列嘗試額度。

租約有效時，只有領取者會進入業務處理器。已完成記錄被重複投遞時直接確認，處理器也可以主動釋放任務以便再次執行。程序退出後，等佇列保留及記錄租約都到期即可重新領取。業務端應依接收記錄 ID 保持外部操作冪等，因為程序可能在外部操作生效後、標記記錄完成前退出。

## 使用邊界 {#scope}

應用網路與任務執行複用既有 [HTTP 用戶端](../digging-deeper/http-client.md)與[佇列](../digging-deeper/queues.md)。持久恢復、完整投遞狀態機仍未實作。

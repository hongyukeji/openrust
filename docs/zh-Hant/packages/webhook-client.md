# Webhook 接收（擴展）

## 支援狀態 {#support-status}

可選套件 `openrust-webhook-client` 提供具名設定、型別化實作工廠、明確回呼路由、`WebhookController`、手動 `WebhookProcessor` 與按範圍隔離的資料庫接收記錄。應用提供任務派發實作。資料庫接收可依已驗簽的事件 ID 去重，可明確啟用。支援明確啟用復原掃描及私有 Storage 磁碟引用。儲存記錄及派發成功不代表業務完成，套件不會自動安裝公開回呼路由。

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

`StandardSignatureValidator` 重用官方 `standardwebhooks 1.3.0`，使用 `webhook-id`、`webhook-timestamp`、`webhook-signature`。金鑰為合法 base64 或帶 `whsec_` 前綴的形式。支援五分鐘時間窗與單個頭內的多個簽名，拒絕重複協定頭。該函式庫要求 UTF-8 本文；若要抑制同一已驗簽事件的重複投遞，請啟用資料庫事件去重。

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

#### 將 multipart 附件保存在私有 Storage 磁碟 {#attachment-disk}

將 `attachment_disk` 設為已設定的私有磁碟名稱，並執行 `./artisan migrate`。手動建構 profile 時可呼叫 `with_attachment_disk("local")`。啟用附件儲存後，接受的含檔案 multipart 請求會作為一個私有物件寫入。接收記錄保存磁碟、長度與 SHA-256 摘要。物件名稱由可信範圍與接收 ID 產生。Worker 核對長度與摘要後讀取原始正文並還原原生上傳檔案。各 Worker 必須能透過已保存的磁碟名稱讀取同一儲存。

使用共用儲存時，為各節點的 `attachment_disk` 設定同名 S3 磁碟。使用私有儲存貯體，並確認後端支援設定與驗證私有物件 ACL。啟用 ACL 的 SeaweedFS 4.53 已通過雙節點驗收。儲存失聯時，新的附件接收會在確認前失敗，復原後可重試並保留原始位元組。

既有內嵌記錄與設定了 `store_attachments: false` 的請求繼續使用資料庫儲存。資料庫明確回滾時，接收器會嘗試刪除暫存物件。提交前崩潰或儲存結果不確定時，可能留下待清理的暫存登記。清理過期接收記錄時，會在同一資料庫交易中刪除記錄，並登記解除引用的物件。

定期執行 `./artisan webhook-client:prune --scope=payments --attachment-staging`，清理已解除引用的物件及暫存至少一小時的物件。使用 `--pretend` 查看候選數量。刪除失敗時會保留登記，租約到期後可重試，指令以錯誤狀態結束。接收器、Worker 和清理程序應將已保存的磁碟名稱對應至同一私有儲存。即使停用了接收記錄保留期清理，也可以執行附件清理。

回復附件遷移前先停止 Webhook 寫入。存在有效引用或暫存登記時，遷移拒絕回復。

### 在 Worker 中處理接收記錄 {#processing-jobs}

實作 `jobs::ProcessWebhookJob` 處理已儲存的接收記錄。在 HTTP 和 Worker 共用的應用 Provider 中，呼叫 `QueuedWebhookJob::register`，傳入原生 `QueueManager`、穩定名稱、可信範圍的儲存及處理器。再按 `process_webhook_job` 選擇的名稱註冊 `QueuedWebhookJob` 派發器。不同儲存範圍使用各自的註冊名。Worker 對照原記錄核對 ID、範圍、設定名和修訂號後才呼叫處理器。

派發器可指定佇列連線和名稱，原生 `JobOptions` 控制試行次數、退避及逾時。業務策略由處理器決定。佇列載荷儲存接收記錄識別資料，Worker 直接讀取已驗收的記錄，無需接收方的驗簽金鑰。處理器失敗時記錄固定異常，下次試行清除上次診斷。請選擇由 Worker 消費的連線。派發器拒絕內建 sync、null、deferred 和 background 連線，並要求佇列回傳已入列結果。

### 同一事務儲存接收記錄與資料庫佇列任務 {#atomic-database-queue}

在設定中設定 `atomic_database_queue: true`，或在執行時呼叫 `with_atomic_database_queue(true)`。讓 `DatabaseWebhookStore` 和 `QueuedWebhookJob` 使用同一應用資料庫連線。接收記錄與佇列任務共同提交後才回傳 HTTP 確認，佇列寫入失敗時一併回滾。其他驅動或分開的連線實例會被拒絕，即使指向同一資料庫。該選項預設關閉，啟用後涵蓋入列前的程序退出窗口。簽名協定提供已驗證的事件 ID 時，可啟用事件去重。

### 接收記錄提交後復原派發 {#dispatch-recovery}

在 profile 設定 `recover_dispatch: true`，或在 Rust 中呼叫 `with_dispatch_recovery(true)`。執行 `./artisan migrate`。此模式在同一交易提交已驗簽的接收記錄與待派發意圖，再回傳設定的 HTTP 確認。此 profile 的 `atomic_database_queue` 須停用。

在接收器、復原程序和 Worker 中呼叫 `QueuedWebhookJob::register_with_database_recovery` 註冊有租約的處理器，使用相同的處理器名稱、接收資料庫與範圍。透過派發器的 `on_connection` 與 `on_queue` 明確指定持久佇列連線與佇列名稱。任務逾時須為正值，且比 Worker 租約至少短兩秒。內建程序內連線和 failover 連線會被拒絕。Redis 等持久直連佇列可與接收資料庫分開運行。

為每個接收範圍排程有界指令，並將保存的佇列連線名稱始終對應至同一佇列後端。用 `--database` 選擇具名接收連線。

```sh
./artisan webhook-client:recover --scope=billing/site-a --limit=100 --pretend
./artisan webhook-client:recover --scope=billing/site-a --limit=100
```

派發失敗時保留意圖，指令以失敗狀態結束。60 秒後可再次領取。若佇列已接受任務，而復原程序在清除意圖前結束，重試可能產生重複的佇列任務。各 Worker 共用接收記錄的領取與完成狀態。業務副作用須實作冪等，以處理執行期間的中斷。

#### 補派已入列但逾期未完成的記錄 {#processing-recovery}

在已啟用 `recover_dispatch: true` 的 profile 中，設定 `"recover_processing": {"after_seconds": 300, "max_requeues": 2}`，或呼叫 `with_processing_recovery(300, 2)?`。執行 `./artisan migrate`。等待時間可設為 60–86,400 秒，補派預算為 1–10 次，兩者都會隨接收記錄保存。等待時間須涵蓋正常的佇列積壓、重試和退避。

同一個排程指令依保存的佇列路由補派逾期且未完成的記錄，跳過仍有業務租約或尚未到預定處理時間的記錄。每次領取補派消耗一次預算，包括佇列確認前中斷的試行。預算耗盡後停止自動補派，已入列的任務仍可執行。記錄完成或終止後，下次復原指令移除派發追蹤列。`--pretend` 只預覽可派發的記錄。Worker 在產生副作用後結束時仍可能重複處理，業務處理器須實作冪等。

### 多 Worker 領取接收記錄 {#processing-lease}

先執行 `./artisan migrate`，再用 `QueuedWebhookJob::register_with_database_lease(&queue, name, store, handler, lease_seconds)` 註冊處理器。儲存參數為 `Arc<DatabaseWebhookStore>`。協調重新啟動消費該佇列的各個 Worker，使其採用相同註冊。例如任務逾時 20 秒、租約 30 秒、資料庫佇列 `retry_after` 60 秒。租約範圍為 3–86,400 秒，任務必須明確設定正數逾時，且至少比租約短兩秒。應為資料庫操作保留餘裕，並讓佇列保留時間或可見性逾時長於租約。競爭中的任務透過原生佇列釋放，可能耗用一次佇列嘗試額度。

租約有效時，只有領取者會進入業務處理器。已完成記錄被重複投遞時直接確認，處理器也可以主動釋放任務以便再次執行。程序退出後，等佇列保留及記錄租約都到期即可重新領取。業務端應依接收記錄 ID 保持外部操作冪等，因為程序可能在外部操作生效後、標記記錄完成前退出。

資料庫依接收記錄與代次保存每次領取。完成、釋放、延期、失敗或停止會與對應的接收狀態變更一併記錄。崩潰後由另一個 Worker 重新領取時，前一次領取標記為過期。嘗試記錄保存時間與狀態，隨接收記錄保留。只有目前租約持有者能寫入結果。舊代次 Worker 延遲回傳時，無法覆蓋後續代次的完成狀態或診斷。

### 依已驗簽事件去重 {#verified-event-deduplication}

在 profile 中選擇 `atomic_database_queue: true` 或 `recover_dispatch: true`，再啟用 `deduplicate_verified_events: true`，並執行 `./artisan migrate`。執行時設定可搭配所選派發選項使用 `with_verified_event_deduplication(true)`。`standard` 驗證器提供簽名涵蓋的 `webhook-id`，此模式要求 ID 不含半形句點。自訂驗證器可在確認簽名涵蓋穩定事件 ID 後，透過 `SignatureValidator::verified_event_id` 提供該 ID。預設 HMAC 僅涵蓋正文，無法提供事件 ID。啟用去重時，驗證器無法提供有效 ID 則拒絕接收。為每個來源帳號設定獨立的 scope 或 profile。

接收記錄、去重鍵及原生佇列任務在同一交易提交。相同 ID 與請求內容的重試回傳設定的確認回應，只儲存一筆接收記錄與一筆任務。同一 scope 與 profile 內，再次使用同一 ID 且 HTTP 方法、原始路徑與查詢字串、正文或 Content-Type 不同時，回傳 HTTP 409。簽名時間戳、簽名及憑證修訂可在重試時變更。去重鍵隨接收記錄保留，記錄的保留期應涵蓋供應商的重複投遞窗口。預設關閉。

### 無穩定事件 ID 時依請求摘要去重 {#body-deduplication}

在啟用 `atomic_database_queue: true` 或 `recover_dispatch: true` 的 profile 中，將 `deduplicate_body_seconds` 設為 1–3,600。執行時可搭配所選派發選項呼叫 `with_body_deduplication(seconds)`。每個 profile 選擇已驗簽事件 ID 去重或摘要窗口去重。接收方在通過驗簽及 Profile 篩選後套用窗口。

窗口從首次接受請求時的資料庫時間起算。摘要涵蓋 HTTP 方法、原始路徑與查詢字串、原始正文及 Content-Type。窗口內完全相同的請求共用一筆接收記錄與任務，過期後可建立新記錄，較早的接收記錄仍可讀取。只有在業務允許將窗口內完全相同的合法請求視為重試時，才啟用此模式。

### 延期處理尚未關聯的通知 {#deferred-processing}

通知可能早於業務記錄抵達時，使用 `QueuedWebhookJob::register_with_database_deferred_processing` 註冊 Worker。原生資料庫佇列須與接收儲存共用同一資料庫連線。設定 `DeferredProcessingPolicy { max_deferrals, max_age_seconds }`，並沿用上文的租約、任務逾時與佇列保留時間。可設定 1–1,000 次延期，有效期為 2 秒至 30 天，從首次延期開始計時。

覆寫 `ProcessWebhookJob::handle_outcome`，在業務記錄尚未就緒時回傳 `WebhookProcessingOutcome::Deferred { delay_seconds }`。延遲須為正數並處於剩餘期限內。業務處理成功回傳 `Completed`，依原生佇列策略重試回傳 `RetryableFailure`，立即停止回傳 `PermanentFailure`。延期時在同一交易儲存下次執行時間與入列，新任務使用獨立的佇列嘗試額度。記錄維持待處理，直至 Worker 完成處理或延期額度耗盡。回傳 `Deferred` 時請勿同時呼叫 `JobContext::release`。

業務記錄就緒並完成關聯後，應用可呼叫 `QueuedWebhookJob::wake_deferred(&store, queue, receipt_id)` 提前喚醒。傳入可信範圍對應的儲存及應用的 `Arc<QueueManager>`。方法沿用延期時保存的處理器及佇列位置。成功安排提前喚醒回傳 `true`，記錄已到期可領取、已結案或不屬於該範圍時回傳 `false`。

使用 `recover_dispatch: true` 和獨立佇列後端時，呼叫 `QueuedWebhookJob::register_with_database_recovery_and_deferred_processing` 註冊 Worker。接收記錄交易一併保存延期狀態與下次派發意圖，定時執行的 `webhook-client:recover` 到期後派發。佇列後端離線時仍可登記提前喚醒。即使已確認入列的喚醒任務遺失且未啟用遺失補派，復原指令仍能在總期限到期後直接終止記錄。到期或提前喚醒接續的佇列任務有獨立的試行次數，保留接收記錄的延期預算及已設定的遺失補派預算。

### 查詢並重放已耗盡的接收記錄 {#replay-exhausted}

先執行 `./artisan migrate`，並升級所有 Webhook Worker，再啟用重放。`webhook-client:exhausted` 按可信範圍列出接收識別資料和中繼資料，每次限 1–500 筆。記錄須已確認入列、自動補派預算耗盡且等待期限已過，同時沒有活躍租約、已排定的延期或終態。

在指令、派發器及 Worker 共用的 Provider 註冊應用的 `ReplayPolicy`。策略依可信應用上下文驗證操作者，核對接收範圍與設定或憑據修訂的目前許可，並給出 1–10 個重放代次和 60–86,400 秒有效期。`--actor` 和 `--reason` 是策略收到的稽核輸入。

```rust
use openrust_webhook_client::replay::{self, ReplayPolicy};

pub fn register_replay_policy(
    container: &openrust::container::Container,
    policy: std::sync::Arc<dyn ReplayPolicy>,
) -> openrust_webhook_client::Result<()> {
    replay::register_policy(container, policy)
}
```

```sh
./artisan webhook-client:exhausted --scope=billing/site-a --limit=20
./artisan webhook-client:replay <receipt-id> --scope=billing/site-a --actor=operator-id --reason="Receipt reviewed"
./artisan webhook-client:recover --scope=billing/site-a
```

申請成功後會登記一個新代次，交由 `webhook-client:recover` 派發。復原指令和 Worker 都會重新核對應用策略。舊代次的佇列任務無法領取此記錄。重放沿用原接收 ID、驗簽歷程與原始酬載。稽核資料隨接收記錄保留，業務副作用仍須由消費端確保冪等。

### 清理過期接收記錄 {#retention}

套件的 `delete_after_days` 預設為 30 天，設為 `null` 可停用保留期清理。應用可透過排程器定時執行 Artisan 指令。傳入接收記錄的可信 scope，可用 `--database` 選擇具名連線。

```sh
./artisan webhook-client:prune --scope=billing/site-a --pretend
./artisan webhook-client:prune --scope=billing/site-a --limit=100
```

資料庫儲存只清理建立時間及結案時間都超過保留期的記錄。待處理記錄、仍持有租約的記錄及缺少結案標記的記錄會保留。刪除接收記錄時，會一併刪除關聯去重鍵與處理嘗試。每次指令最多處理 500 筆。`WebhookCall::prunable` 透過儲存配接器預覽候選記錄 ID。

### 無簽名回呼的查詢提示 {#unverified-hints}

供應商無法提供簽名時，可在 profile 將 `unverified_hint_receiver` 指向透過 `WebhookRuntime::register_unverified_hint_receiver` 註冊的實作。接收器取得 `UnverifiedHint`。預設驗證器僅在簽名欄位缺失時採用此流程。Standard 驗證器要求三個協定欄位全部缺失。帶有簽名欄位的請求沿用一般驗簽及回應流程。自訂驗證器若使用其他簽名欄位，可覆寫 `SignatureValidator::has_signature`。內建接收器要求儲存範圍與 profile 設定的 `scope` 一致。

`QueuedWebhookHint` 使用與提示儲存共用連線的資料庫佇列。透過 `WebhookHintPolicy` 只關聯可信範圍內已登記的本地任務，再由獨立 `ProcessWebhookHint` 在查詢前複核。各部署節點共用限流快取。內建配接器依範圍與 profile 實施每分鐘 1–600 次的設定限額。已接受的提示及未知任務都回傳同樣的 HTTP 202。提示記錄保存本地任務引用，五分鐘後失去查詢資格，每筆提示只可領取一次，任務逾時為 15 秒。

查詢處理器須依業務契約獨立驗證供應商回應。原始通知維持無簽名提示，提示任務只攜帶提示 ID 及修訂。可信接收記錄與佇列繼續處理已驗簽回呼。

### 保留驗簽判定依據 {#verification-audit}

在設有可信 `scope` 的 profile 中啟用 `verification_audit: true`，並執行 `./artisan migrate`。稽核使用 profile 的 `connection`。帳本保存驗證器和策略名稱、profile 修訂、UTC 分鐘，以及分類和固定原因。修訂是設定中的憑證集合標籤，憑證集合變更時應更新。

每個範圍、profile、修訂、驗證器、策略和原因在一分鐘內只保留一筆觀測。稽核記錄業務處理前的可信度判定，區分已驗簽、無簽名提示和拒絕。稽核僅保存上述中繼資料。啟用稽核後，寫入失敗會回傳錯誤並中止後續業務處理。

```sh
./artisan webhook-client:prune --scope=billing/site-a --verification-audit --pretend
./artisan webhook-client:prune --scope=billing/site-a --verification-audit
```

該指令依套件的 `delete_after_days` 清理驗簽判定記錄。

## 使用邊界 {#scope}

應用網路與任務執行複用既有 [HTTP 用戶端](../digging-deeper/http-client.md)與[佇列](../digging-deeper/queues.md)。持久復原涵蓋已提交接收記錄的派發。各佇列後端與部署拓撲需各自驗收。

### 單機與多副本 {#deployment}

為所有副本設定同一套可寫入的 PostgreSQL 或 MySQL 資料庫。同庫原子接收和入列使用 database 佇列，任務投遞到 Redis 時可設定持久派發復原。HTTP 節點與 Worker 使用一致的佇列名稱、應用程式金鑰及已授權的設定版本。持久附件使用所有 Worker 都能存取的私有儲存。本機驗收在同一主機使用獨立節點目錄和共用目錄。跨主機部署前須驗證實際共用儲存和部署環境。

已設定持久派發復原的接收記錄，在佇列失聯期間仍可於資料庫提交後確認。為所選佇列連線安排復原任務。接收資料庫無法連線時，請求失敗，傳送端可重試。

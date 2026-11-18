# 通知

## 產生通知 {#generating-notifications}

命令會在 `app/notifications` 下產生 Rust 通知，檔案和模組目錄使用 snake_case。預設類型會透過郵件管道傳送純文字。

```sh
./artisan make:notification InvoicePaid
./artisan make:notification Billing/PaymentReceived --markdown=mail.billing.paid --test
```

在終端機執行不帶名稱的 `./artisan make:notification`，可輸入類型名稱並選擇是否建立 Markdown 視圖。確認建立後，可接受建議的視圖名稱或自行輸入。傳入選項時，Artisan 只詢問類型名稱。使用 `--no-interaction` 或管線輸入時，需明確提供名稱。

Markdown 選項也會產生 Blade 範本。省略視圖名稱時，會在 `mail.` 後加入以連字號分詞的各名稱段。命令在產生或重新產生通知原始碼時，會覆寫同路徑的 Markdown 範本。用 `--force` 重新產生既有的 Rust 類型，用 `--test` 產生配套功能測試。重新建置前，在上層 `mod.rs` 宣告新模組。應用程式可在 `stubs/` 覆寫原始碼範本。

## 傳送通知

實作 `Notification::via` 選擇管道，再實作對應的內容方法。收件人實作 `Notifiable`，提供各管道路由和選用的語言偏好。

```rust
use openrust::{
    notifications::{Notifiable, Notification, NotificationResult, messages::DatabaseMessage},
    support::facades::Notification as Notifications,
};

pub struct InvoicePaid;

impl Notification for InvoicePaid {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["database".into()])
    }

    fn to_database(&self, _: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(Some(DatabaseMessage::new(serde_json::json!({"invoice": 42}))))
    }
}

pub async fn notify(customer: &dyn Notifiable) -> NotificationResult<()> {
    Notifications::send(&[customer], &InvoicePaid).await
}
```

郵件通知在 `to_mail` 回傳 `MailDelivery::Message`，透過 `NotificationRoute::Mail` 提供地址。`MailDelivery::Mailable` 可傳送自帶收件人的完整郵件。`sendNow` 可明確指定管道。語言依序採用通知、傳送器、收件人和應用程式的設定。

監聽器可透過 `NotificationSending` 阻止投遞。派送器也會發出 `NotificationSent`、`NotificationSkipped` 和 `NotificationFailed`。

## 資料庫通知

執行 `./artisan make:notifications-table`，將產生的遷移加入應用程式遷移清單，再執行 `./artisan migrate`。產生的關聯鍵預設為無號整數。模型使用 UUID 或 ULID 時，在產生的遷移中調整 `notifiable_id` 欄位的類型。

透過 `NotificationRoute::Database` 提供模型類型和主鍵。資料庫管道儲存通知 UUID 和 JSON 資料。`DatabaseRoute::query` 依模型限制查詢。`find` 回傳可呼叫 `markAsRead` 和 `markAsUnread` 的記錄。

## 通知加入佇列

透過 `Notification::queue_options` 為每位收件人的每個管道回傳 `DispatchOptions`。每次投遞有自己的佇列任務。Worker 解析管道並觸發投遞事件。在使用中的交易裡，呼叫 `ChannelManager::sendInTransaction` 並啟用 `after_commit`，使交易回復時捨棄待加入佇列的通知。

如果佇列通知依賴可能在 Worker 啟動前變動的狀態，透過 `Notification::queue_guard` 回傳帶名稱和 JSON 上下文的 `NotificationQueueGuard`。在 Worker 載入的服務提供者中，用 `ChannelManager::extendQueueGuard` 註冊非同步判定函式。結果為 false 時會跳過該管道並發出 `NotificationSkipped`。立即傳送和未設定 guard 的佇列通知沿用傳送端的 `should_send`。

若要依管道設定 Worker 限制，從 `queue_job_options` 回傳 `JobOptions`。要為通知啟用佇列中介軟體，在傳送端和 Worker 都載入的服務提供者中，用 `SendQueuedNotifications::registerMiddleware` 註冊具名鏈，並讓 `queue_middleware` 回傳該名稱。Worker 會對該工作執行佇列的中介軟體管線。

## 隨選通知

用 `Notifications::route` 直接向地址傳送通知，無需載入模型。範例中的 `PaymentReceived` 是 `via` 包含 `mail` 的通知。可鏈式呼叫 `route` 加入其他管道地址。資料庫管道需要已持久化的收件人。

```rust
use openrust::{
    notifications::{Notifiable, NotificationRoute},
    support::facades::Notification as Notifications,
};

Notifications::route(
    "mail",
    NotificationRoute::Mail(vec!["ondemand@example.test".into()]),
)?
.notifyNow(&PaymentReceived, None)
.await?;
```

## 廣播通知

在 `via` 中加入 `broadcast`。由 `to_broadcast` 回傳 `BroadcastMessage`，或由 `to_array` 回傳物件。用訊息的 `connection` 和 `queue` 選擇廣播事件的佇列連線和佇列名稱。事件預設附加通知 ID 和類型。有資料庫路由的模型預設在由模型類型和鍵組成的私人頻道上接收事件。可覆寫 `receives_broadcast_notifications_on` 自訂私人頻道名稱，或用 `NotificationRoute::Broadcast` 為隨選收件人指定頻道。

## 郵件通知 {#mail-notifications}

### 格式化郵件訊息 {#formatting-mail-messages}

在 `to_mail` 中回傳 `MailMessage`，即可傳送響應式 HTML 通知與對應的純文字正文。在 `action` 之前加入的文字行出現在按鈕上方，之後加入的行則在按鈕下方。使用 `lineIf` 或 `linesIf` 按條件加入正文。`error` 和 `success` 決定按鈕色彩。預設問候語、結語及複製連結說明依通知語系產生，結語使用 `app.name`。

```rust
use openrust::notifications::messages::{MailDelivery, MailMessage};

let delivery = MailDelivery::Message(Box::new(
    MailMessage::new()
        .subject("Invoice paid")
        .greeting("Hello!")
        .line("Your invoice has been paid.")
        .lineIf(total > 0, format!("Amount paid: {total}"))
        .action("View invoice", invoice_url)
        .line("Thank you for your payment."),
));
```

未指定主旨時，郵件管道依通知類型名稱產生主旨。執行 `./artisan vendor:publish --tag=laravel-notifications`，即可編輯 `resources/views/vendor/notifications/email.blade.html` 覆寫預設視圖。 若要為單則訊息選擇範本，可呼叫 `view` 設定 HTML 視圖，再按需使用 `text` 加入純文字視圖。對新訊息直接呼叫 `text` 可只產生純文字。

### 寄件人與收件人 {#sender-and-recipients}

使用 `from` 覆寫設定中的寄件人，並用 `replyTo`、`cc` 或 `bcc` 新增地址。可傳入電子郵件字串或帶顯示名稱的 `Address`。郵件管道透過 `Notifiable::route_notification_for` 取得收件人，模型通知和隨需通知均使用 `NotificationRoute::Mail`。透過 `MailDelivery::Mailable` 回傳的自訂郵件自行宣告收件人。

### 附件與訊息自訂 {#attachments-and-message-customization}

使用 `attach` 或 `attachMany` 新增[附件來源](mail.md#attachments)，也可呼叫 `attachData`、`attachFromStorage` 或 `attachFromStorageDisk`。佇列 Worker 投遞時解析檔案與磁碟附件。使用 `tag` 和 `metadata` 新增投遞標記。在傳送端與 Worker 應用程式中透過 `MailManager::registerMessageCustomizer` 註冊回呼，再用 `withMessage` 傳入註冊名稱與 JSON 資料。

```rust
use openrust::{
    mail::{MessageCustomization, mailables::Address},
    notifications::messages::MailMessage,
};

let message = MailMessage::new()
    .from(Address {
        address: "finance@example.test".into(),
        name: Some("Finance".into()),
    })
    .replyTo("support@example.test")
    .tag("invoice")
    .metadata("invoice-id", "42")
    .line("Your invoice is ready.")
    .attachData(b"total,42".as_slice(), "summary.csv", Some("text/csv"))
    .withMessage(MessageCustomization::new(
        "invoice.trace",
        serde_json::json!({"id": 42}),
    ));
```

範例使用註冊名稱為 `invoice.trace` 的應用程式回呼。若附件實作了 `Attachable`，將其 `toMailAttachment()` 的回傳值交給 `attach`。

### 預覽通知郵件 {#previewing-mail-notifications}

使用 `render().await` 取得正文，或在路由中回傳 `toResponse().await`。回應依所選視圖提供 HTML 或純文字。預覽只渲染範本，不傳送郵件、讀取一般附件或執行訊息自訂回呼。

```rust
use openrust::{mail::Mailable, notifications::messages::MailMessage};

let mail = MailMessage::new()
    .markdown("mail.invoice.paid", data)
    .theme("billing")
    .priority(2);

let response = mail.toResponse().await?;
```

### 郵件優先順序 {#mail-priority}

使用 `priority` 設定 MIME 的 `X-Priority`。1 為最高，5 為最低，超出範圍時取最近的邊界值。此優先順序供收件方的郵件用戶端使用。

## Markdown 郵件通知

執行 `./artisan make:notification InvoicePaid --markdown=mail.invoice.paid` 產生通知與 Blade 範本。使用 `markdown` 選定渲染雙正文的範本。`template` 調整 Markdown 範本並保留已明確選擇的視圖，`theme` 可為單封郵件覆寫 Markdown 主題。建構方法依鍵合併視圖資料，較晚提供的值優先。`to_mail` 回傳的自訂郵件也可使用相同的 [Markdown 渲染器與元件](mail.md#writing-markdown-messages)。

## 測試通知

在測試應用程式中呼叫 `Notifications::fake()` 記錄通知分派，管道和佇列不會執行。用 `assertSentToOnce` 斷言模型收件人，用 `assertSentOnDemandOnce` 斷言隨選收件人。比對斷言可檢查管道、語言、路由以及 `Notification::fake_data` 提供的資料。

```rust
let notifications = Notifications::fake()?;
let target = Notifications::route(
    "mail",
    NotificationRoute::Mail(vec!["ops@example.test".into()]),
)?;
target.notify(&notice).await?;
notifications.assertSentOnDemandOnce(notice.kind());
```

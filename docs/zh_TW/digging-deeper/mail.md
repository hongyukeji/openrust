# 郵件

## 傳送郵件

實作 `Mailable`，提供信封和內容。透過 `Mail::to` 新增收件人。可用 `Mail::mailer(Some("smtp"))` 選擇命名郵件器，或覆寫 `Mailable::mailer`。

```rust
use openrust::{
    mail::{MailResult, Mailable, mailables::{Content, Envelope}},
    support::facades::Mail,
};

pub struct OrderShipped;

impl Mailable for OrderShipped {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope { subject: "Order shipped".into(), ..Envelope::default() })
    }

    fn content(&self) -> MailResult<Content> {
        Ok(Content { text: Some("Your order is on its way.".into()), ..Content::default() })
    }
}

pub async fn send_order() -> MailResult<()> {
    Mail::to("customer@example.test")?.send(&OrderShipped).await?;
    Ok(())
}
```

`Content` 支援文字、HTML 和帶資料的命名視圖。可用 `view` 和 `text_view` 產生兩種 MIME 內文。附件提供位元組、檔名和 MIME 類型，自訂郵件標頭透過 `headers` 提供。密件副本地址保留在投遞信封中，郵件標頭會移除 Bcc。

## 設定

`config/mail.rs` 定義預設郵件器、命名郵件器和寄件人。骨架預設使用 `log` 驅動，在 debug 層級寫入 MIME 郵件。`array` 驅動可透過 `Mailer::sent` 檢查目前應用程式的郵件。

使用 SMTP 時設定 `MAIL_MAILER=smtp`，再設定 `MAIL_HOST`、`MAIL_PORT` 或 `MAIL_URL`。URL 覆寫其提供的連線欄位。`smtps` 使用隱含 TLS。`smtp` 在伺服器支援時使用 STARTTLS，設定憑證時必須成功升級 TLS。

選擇 `sendmail` 可執行 `MAIL_SENDMAIL_PATH` 設定的程式，支援透過標準輸入輸出進行 SMTP 的 `-bs` 模式，以及從標準輸入投遞郵件的 `-t` 模式。

選擇 `ses`、`ses-v2`、`postmark`、`resend`、`mailgun` 或 `cloudflare` 透過供應商投遞。先在 `config/services.rs` 設定對應服務，再用 `MAIL_MAILER` 選擇郵件器。

- SES 和 SES v2 使用 AWS 憑證或 AWS SDK 憑證鏈。SES v2 支援 `ConfigurationSetName`、`EmailTags` 和 `ListManagementOptions` 等郵件器選項。
- Postmark 使用 `POSTMARK_TOKEN`，Resend 使用 `RESEND_KEY`。
- Mailgun 使用 `MAILGUN_DOMAIN` 和 `MAILGUN_SECRET`。使用 EU API 時設定 `MAILGUN_ENDPOINT=api.eu.mailgun.net`。預設 `https` 方案會提交已組裝的 MIME 郵件。若要提交 multipart 欄位，將 `scheme` 設為 `api`。
- Cloudflare 使用 `CLOUDFLARE_ACCOUNT_ID` 和 `CLOUDFLARE_KEY`。

在 `config/mail.rs` 中，`failover` 在目前郵件器傳送失敗時切換，`roundrobin` 輪流使用郵件器。兩者皆透過 `mailers` 指定郵件器清單，並在 `retry_after` 秒內跳過失敗的傳輸。

## Markdown 郵件

將 `Content::markdown` 設為包含 CommonMark 的具名 Blade 視圖。郵件器產生 HTML 和純文字兩種版本，並將主題樣式內嵌到 HTML。框架提供預設主題。若要使用應用程式樣式，設定 `MAIL_MARKDOWN_THEME=receipt`，再將 `receipt.css` 放到 `resources/views/vendor/mail/html/themes/`。可透過 `text_view` 提供自己的純文字版本。

```rust
let content = Content {
    markdown: Some("markdown-receipt".into()),
    with: [("name".into(), serde_json::json!("Sam"))].into_iter().collect(),
    ..Content::default()
};
```

## 郵件加入佇列

讓 `Mailable::queue_options` 回傳佇列選項後，`send` 會將郵件加入佇列。`sendNow` 立即傳送。`PendingMail::queue` 接收連線、佇列、延遲和選用交易。`after_commit` 將派送延後到交易提交。Worker 用佇列中的郵件定義呈現命名視圖並傳送。用 `./artisan queue:work --stop-when-empty` 消費郵件佇列。

## 擴充郵件傳輸和投遞事件

在應用程式服務提供者中用 `MailManager::extend` 註冊 Rust 傳輸工廠，透過郵件器的 `transport` 設定選擇它。派發器在呼叫傳輸前發出 `MessageSending`，在傳輸接受郵件後發出 `MessageSent`。監聽器回傳 `false` 可以取消投遞。取消時，`send` 回傳 `None`，`sendNow` 回傳 `MailError::Cancelled`。

## 測試郵件

在測試應用程式中呼叫 `Mail::fake()`，即可檢查郵件並跳過傳輸和加入佇列。斷言區分即時傳送與排隊傳送。用 `Mailable::kind()` 選擇郵件記錄，用比對斷言檢查信封或佇列選項。

```rust
let mail = Mail::fake()?;
Mail::to("ops@example.test")?.sendNow(&receipt).await?;
mail.assertSentMatching(receipt.kind(), |entry| {
    entry.envelope.to.iter().any(|to| to.address == "ops@example.test")
});
mail.assertNothingQueued();
```

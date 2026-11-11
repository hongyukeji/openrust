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

## 郵件加入佇列

讓 `Mailable::queue_options` 回傳佇列選項後，`send` 會將郵件加入佇列。`sendNow` 立即傳送。`PendingMail::queue` 接收連線、佇列、延遲和選用交易。`after_commit` 將派送延後到交易提交。Worker 用佇列中的郵件定義呈現命名視圖並傳送。用 `./artisan queue:work --stop-when-empty` 消費郵件佇列。

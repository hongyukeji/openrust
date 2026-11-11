# 邮件

## 发送邮件

实现 `Mailable`，提供信封和内容。通过 `Mail::to` 添加收件人。可用 `Mail::mailer(Some("smtp"))` 选择命名邮件器，或覆盖 `Mailable::mailer`。

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

`Content` 支持文本、HTML 和带数据的命名视图。可用 `view` 和 `text_view` 生成两种 MIME 正文。附件提供字节、文件名和 MIME 类型，自定义邮件头通过 `headers` 提供。密送地址保留在投递信封中，邮件头会移除 Bcc。

## 配置

`config/mail.rs` 定义默认邮件器、命名邮件器和发件人。骨架默认使用 `log` 驱动，在 debug 级别写入 MIME 邮件。`array` 驱动可通过 `Mailer::sent` 检查当前应用的邮件。

使用 SMTP 时设置 `MAIL_MAILER=smtp`，再配置 `MAIL_HOST`、`MAIL_PORT` 或 `MAIL_URL`。URL 覆盖其提供的连接字段。`smtps` 使用隐式 TLS。`smtp` 在服务器支持时使用 STARTTLS，配置凭据时必须成功升级 TLS。

## 邮件入队

让 `Mailable::queue_options` 返回队列选项后，`send` 会将邮件入队。`sendNow` 立即发送。`PendingMail::queue` 接收连接、队列、延迟和可选事务。`after_commit` 将分派延后至事务提交。Worker 用队列中的邮件定义渲染命名视图并发送。用 `./artisan queue:work --stop-when-empty` 消费邮件队列。

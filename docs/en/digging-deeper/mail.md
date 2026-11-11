# Mail

## Sending mail

Implement `Mailable` with an envelope and content. `Mail::to` adds recipients before sending. Select a named mailer with `Mail::mailer(Some("smtp"))` or override `Mailable::mailer`.

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

`Content` accepts text, HTML, or named views with data. Set `view` and `text_view` for the two MIME alternatives. Use `attachments` for bytes with a filename and MIME type, and `headers` for custom headers. The mailer removes Bcc from the MIME headers and retains those recipients in the transport envelope.

## Configuration

`config/mail.rs` defines the default mailer, named mailers and sender. The skeleton defaults to the `log` transport, which writes the MIME message at debug level. Use `array` to inspect messages in the current application with `Mailer::sent`.

For SMTP, set `MAIL_MAILER=smtp` and configure `MAIL_HOST` and `MAIL_PORT`, or set `MAIL_URL`. The URL overrides the connection fields it supplies. `smtps` uses implicit TLS. `smtp` upgrades with STARTTLS when offered, and credentials require a successful TLS upgrade.

## Queueing mail

Return queue options from `Mailable::queue_options` to make `send` enqueue delivery. Use `sendNow` for immediate delivery. `PendingMail::queue` accepts connection, queue, delay and an optional transaction. Enable `after_commit` to defer dispatch until commit. The worker composes named views from the queued definition in the worker application, then sends the message. Use `./artisan queue:work --stop-when-empty` to process queued mail.

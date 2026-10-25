# Mail

Mail layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework. Implement `Email::message` to build a `lettre::Message`, then send it through the `Mailer` service. `MailerPlugin` registers the log driver by default. Enable `mail-smtp` and set `[mail] driver = "smtp"`, `smtp_host`, and optionally matching `smtp_username` / `smtp_password` fields to send over mandatory STARTTLS. SMTP credentials are redacted from `Debug` output. Enable `test-util` to use `FakeMailer` in tests with `sent_count` and `assert_sent_to` assertions.

`Mailer::send` sends immediately. Queue integration is added with the queue capability in M4.

```rust,no_run
use openrust::mail::{Email, Mailer, lettre::Message};
use openrust::core::{Error, Result};

struct WelcomeEmail;

impl Email for WelcomeEmail {
    fn message(&self) -> Result<Message> {
        Ok(Message::builder()
            .from("OpenRust <noreply@example.com>".parse().map_err(Error::internal)?)
            .to("person@example.com".parse().map_err(Error::internal)?)
            .subject("Welcome")
            .body(String::from("Welcome to OpenRust"))
            .map_err(Error::internal)?)
    }
}

async fn send(mailer: &Mailer) -> Result<()> {
    mailer.send(&WelcomeEmail).await
}
```

For SMTP, add `features = ["mail-smtp"]` to the `openrust` dependency and configure:

```toml
[mail]
driver = "smtp"
smtp_host = "smtp.example.com"
smtp_port = 587
smtp_username = "mailer"
smtp_password = "replace-with-a-secret"
```

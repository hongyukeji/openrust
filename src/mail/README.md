# Mail

`MailConfig` is defined in `openrust::config::mailer` and re-exported from `openrust::mail`.
Keep runtime TOML in the application root `config/`; `.env` may supply local SMTP credentials, while
production reads secrets from process environment variables and ignores `.env`.

Mail layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework. Implement `Email::message` to build a `lettre::Message`, then send it through the `Mailer` service. `MailerPlugin` registers the log driver by default. Enable `mail-smtp` and set `[mail] driver = "smtp"`, `smtp_host`, and optionally matching `smtp_username` / `smtp_password` fields to send over mandatory STARTTLS. SMTP credentials are redacted from `Debug` output. STARTTLS is required by default; only an unauthenticated localhost/loopback development sink such as Mailpit may explicitly set `smtp_starttls = false`, and production rejects that setting. Start the local Mailpit service with `docker compose -f compose.ci.yaml up -d mailpit`, then use `APP_MAIL__DRIVER=smtp`, `APP_MAIL__SMTP_HOST=127.0.0.1`, `APP_MAIL__SMTP_PORT=1025`, and `APP_MAIL__SMTP_STARTTLS=false`. The default `log` driver is rejected in production, so a live mail provider must be configured explicitly. Enable `test-util` to use `FakeMailer` in tests with `sent_count` and `assert_sent_to` assertions. SMS is a sibling service in `openrust::sms`; both use public driver traits, explicit locale selection and non-retrying single submissions. Twilio is available with `sms-twilio` (which enables `http-client`) and requires `sms.account_sid`, `sms.api_key_sid`, `sms.api_key_secret`, and exactly one of `sms.from` / `sms.messaging_service_sid`. Credentials and message data are redacted from debug output; production rejects the log driver.

`Mailer::send` sends immediately. Queue integration is added with the queue capability in M4. An accepted submission is not a delivery receipt.

To localize a message, select a `Locale` from the request's `RequestLocale` or explicitly for a background task. `LocalizedEmail::new` renders separate subject/body keys from the shared `I18n` catalog with named arguments and builds a plain-text `lettre::Message`; applications can still implement `Email` for custom multipart or HTML formats. The mail transport does not infer a locale from global state; callers choose it when building the message.

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

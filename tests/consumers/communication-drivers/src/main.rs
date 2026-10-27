//! 外部应用通过公开 Plugin 与 driver API 接入自定义邮件和短信供应商。

use std::sync::{Arc, Mutex};

use figment::providers::Format;
use openrust::{
    BoxFuture, Error, Result,
    config::FrameworkConfig,
    core::{App, AppBuilder, Plugin, lifecycle},
    mail::{Email, MailDriver, Mailer, lettre::Message},
    sms::{SmsDriver, SmsMessage, SmsReceipt, SmsSender},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct ProviderConfig {
    driver: String,
}

#[derive(Clone, Default)]
struct DeliveryCounts {
    email: Arc<Mutex<usize>>,
    sms: Arc<Mutex<usize>>,
}

struct ApplicationCommunicationPlugin {
    counts: DeliveryCounts,
}

impl Plugin for ApplicationCommunicationPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let mail: ProviderConfig = app.section("mail")?;
        let sms: ProviderConfig = app.section("sms")?;
        if mail.driver != "example" || sms.driver != "example" {
            return Err(Error::custom(
                "example_provider_invalid",
                "the communication consumer requires its example drivers",
            ));
        }

        app.try_insert(Mailer::new(ExampleMailDriver(self.counts.email.clone())))?;
        app.try_insert(SmsSender::new(ExampleSmsDriver(self.counts.sms.clone())))?;
        Ok(())
    }
}

struct ExampleMailDriver(Arc<Mutex<usize>>);

impl MailDriver for ExampleMailDriver {
    fn send(&self, _message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut count = self
                .0
                .lock()
                .map_err(|_| Error::custom("example_lock_poisoned", "driver lock poisoned"))?;
            *count += 1;
            Ok(())
        })
    }
}

struct ExampleSmsDriver(Arc<Mutex<usize>>);

impl SmsDriver for ExampleSmsDriver {
    fn send(&self, _message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async move {
            let mut count = self
                .0
                .lock()
                .map_err(|_| Error::custom("example_lock_poisoned", "driver lock poisoned"))?;
            *count += 1;
            Ok(SmsReceipt {
                provider: "example".into(),
                message_id: None,
            })
        })
    }
}

struct WelcomeEmail;

impl Email for WelcomeEmail {
    fn message(&self) -> Result<Message> {
        Message::builder()
            .from(
                "OpenRust <hello@example.test>"
                    .parse()
                    .map_err(Error::internal)?,
            )
            .to("user@example.test".parse().map_err(Error::internal)?)
            .subject("Welcome")
            .body("Welcome to OpenRust".to_owned())
            .map_err(Error::internal)
    }
}

async fn build_app(counts: DeliveryCounts) -> Result<App> {
    let figment = figment::Figment::new().merge(figment::providers::Toml::string(
        "[mail]\ndriver = 'example'\n[sms]\ndriver = 'example'\n",
    ));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(ApplicationCommunicationPlugin { counts });
    lifecycle::finalize(builder).await
}

#[tokio::test]
async fn application_plugin_registers_custom_drivers_through_public_services() -> Result<()> {
    let counts = DeliveryCounts::default();
    let app = build_app(counts.clone()).await?;
    let mailer = app.state().try_get::<Mailer>()?;
    let sms = app.state().try_get::<SmsSender>()?;

    mailer.send(&WelcomeEmail).await?;
    let receipt = sms
        .send(SmsMessage::text("+14155550123", "Welcome")?)
        .await?;

    assert_eq!(receipt.provider, "example");
    assert_eq!(
        *counts
            .email
            .lock()
            .map_err(|_| Error::custom("example_lock_poisoned", "driver lock poisoned"))?,
        1
    );
    assert_eq!(
        *counts
            .sms
            .lock()
            .map_err(|_| Error::custom("example_lock_poisoned", "driver lock poisoned"))?,
        1
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = build_app(DeliveryCounts::default()).await?;
    let mailer = app.state().try_get::<Mailer>()?;
    let sms = app.state().try_get::<SmsSender>()?;
    mailer.send(&WelcomeEmail).await?;
    sms.send(SmsMessage::text("+14155550123", "Welcome")?)
        .await?;
    Ok(())
}

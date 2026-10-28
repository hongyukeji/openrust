//! 外部应用通过公开 ServiceProvider 与 driver API 接入自定义邮件和短信供应商。

use std::sync::{Arc, Mutex};

use figment::providers::Format;
use openrust::{
    BoxFuture, Error, Result,
    core::{App, ProviderBuilder, RegisterContext, ServiceProvider},
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

struct ApplicationCommunicationProvider {
    counts: DeliveryCounts,
}
impl ServiceProvider for ApplicationCommunicationProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        let counts = self.counts.clone();
        app.singleton::<Mailer, _>(move |resolver| {
            let counts = counts.clone();
            Box::pin(async move {
                let config: ProviderConfig = resolver.section("mail")?;
                if config.driver != "example" {
                    return Err(Error::custom(
                        "example_provider_invalid",
                        "the communication consumer requires its example drivers",
                    ));
                }
                Ok(Arc::new(Mailer::new(ExampleMailDriver(counts.email))))
            })
        });
        let counts = self.counts.clone();
        app.singleton::<SmsSender, _>(move |resolver| {
            let counts = counts.clone();
            Box::pin(async move {
                let config: ProviderConfig = resolver.section("sms")?;
                if config.driver != "example" {
                    return Err(Error::custom(
                        "example_provider_invalid",
                        "the communication consumer requires its example drivers",
                    ));
                }
                Ok(Arc::new(SmsSender::new(ExampleSmsDriver(counts.sms))))
            })
        });
        app.require::<Mailer>();
        app.require::<SmsSender>();
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
    let mut builder = ProviderBuilder::new();
    builder.provider(ApplicationCommunicationProvider { counts });
    App::from_provider_runtime(builder.start(figment).await?)
}

#[tokio::test]
async fn application_provider_registers_custom_drivers_through_public_services() -> Result<()> {
    let counts = DeliveryCounts::default();
    let app = build_app(counts.clone()).await?;
    let mailer = app.state().resolve::<Mailer>().await?;
    let sms = app.state().resolve::<SmsSender>().await?;

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
    let mailer = app.state().resolve::<Mailer>().await?;
    let sms = app.state().resolve::<SmsSender>().await?;
    mailer.send(&WelcomeEmail).await?;
    sms.send(SmsMessage::text("+14155550123", "Welcome")?)
        .await?;
    Ok(())
}

#[path = "../../config/mod.rs"]
mod config;
#[path = "../../database/migrations/mod.rs"]
mod migrations;

use std::{error::Error, process::ExitCode, sync::Arc};

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, runApplication},
    http::HttpFuture,
    notifications::{
        ChannelManager, Notifiable, Notification, NotificationError, NotificationFuture,
        NotificationResult, NotificationRoute,
        channels::SmsChannel,
        sms::{SmsDriver, SmsManager, SmsMessage},
    },
    queue::DispatchOptions,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Http, Notification as Notify, Sms},
    },
};
use serde_json::{Value, json};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    runApplication(|| {
        let root = Application::inferBasePath(Some(env!("CARGO_MANIFEST_DIR")))?;
        Application::configure(root)
            .withConfig(|path, env| {
                let mut config = config::load(path, env)?;
                config.insert(
                    "sms".into(),
                    json!({
                        "default": env.get_utf8("SMS_DRIVER")?.unwrap_or("primary"),
                        "drivers": {
                            "primary": {
                                "driver": "demo-alpha",
                                "endpoint": env.get_utf8("SMS_ALPHA_URL")?,
                                "token": env.get_utf8("SMS_ALPHA_TOKEN")?,
                            },
                            "backup": {
                                "driver": "demo-beta",
                                "endpoint": env.get_utf8("SMS_BETA_URL")?,
                                "token": env.get_utf8("SMS_BETA_TOKEN")?,
                            },
                        }
                    }),
                );
                Ok(config)
            })
            .withProviders(vec![Box::new(SmsServiceProvider)])
            .withMigrations(migrations::entries())
            .withCommands(register_commands)
            .create()
            .map_err(Into::into)
    })
    .await
}

struct SmsServiceProvider;

impl ServiceProvider for SmsServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let sms = app.container().make::<SmsManager>()?;
            sms.extend("demo-alpha", |profile| {
                Ok(Arc::new(Provider::new(profile, Protocol::Alpha)?))
            });
            sms.extend("demo-beta", |profile| {
                Ok(Arc::new(Provider::new(profile, Protocol::Beta)?))
            });

            let channels = app.container().make::<ChannelManager>()?;
            channels.extend("sms", Arc::new(SmsChannel::default()));
            channels.extend("primary", Arc::new(SmsChannel::using("primary")));
            channels.extend("backup", Arc::new(SmsChannel::using("backup")));
            Ok(())
        })
    }
}

enum Protocol {
    Alpha,
    Beta,
}

struct Provider {
    protocol: Protocol,
    endpoint: String,
    token: String,
}

impl Provider {
    fn new(profile: &Value, protocol: Protocol) -> NotificationResult<Self> {
        let required = |name: &str| {
            profile[name]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    delivery_error("Set the endpoint and token for this demo SMS profile.")
                })
        };
        Ok(Self {
            protocol,
            endpoint: required("endpoint")?,
            token: required("token")?,
        })
    }
}

impl SmsDriver for Provider {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            let request =
                Http::acceptJson().map_err(|_| delivery_error("SMS HTTP client unavailable."))?;
            let (request, body) = match self.protocol {
                Protocol::Alpha => (
                    request.withToken(&self.token),
                    json!({"to": message.to(), "text": message.content()}),
                ),
                Protocol::Beta => (
                    request.withBasicAuth("demo", &self.token),
                    json!({"recipient": message.to(), "message": message.content()}),
                ),
            };
            let response = request
                .withoutRedirecting()
                .timeout(std::time::Duration::from_secs(5))
                .maxResponseBytes(16 * 1024)
                .post(&self.endpoint, &body)
                .await
                .map_err(|_| delivery_error("The demo SMS request failed."))?;
            if !response.successful() {
                return Err(delivery_error(
                    "The demo SMS provider rejected the request.",
                ));
            }
            response
                .json()
                .map_err(|_| delivery_error("Invalid demo SMS response."))
        })
    }
}

fn delivery_error(message: &'static str) -> NotificationError {
    NotificationError::Delivery {
        source: Box::<dyn Error + Send + Sync>::from(message),
    }
}

struct DemoNotification {
    channel: String,
    text: String,
}

impl Notification for DemoNotification {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec![self.channel.clone()])
    }

    fn to_sms(&self, _: &dyn Notifiable) -> NotificationResult<Option<String>> {
        Ok(Some(self.text.clone()))
    }

    fn queue_options(&self, _: &dyn Notifiable, _: &str) -> Option<DispatchOptions> {
        Some(DispatchOptions::default())
    }
}

struct SendSms(CommandDefinition);

fn register_commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = clap::Command::new("sms:demo")
        .about("Send a demo SMS through a provider")
        .arg(clap::Arg::new("to").required(true))
        .arg(clap::Arg::new("text").required(true))
        .arg(
            clap::Arg::new("profile")
                .long("profile")
                .value_parser(["primary", "backup"]),
        )
        .arg(
            clap::Arg::new("now")
                .long("now")
                .action(clap::ArgAction::SetTrue),
        );
    artisan.add_command(Box::new(SendSms(CommandDefinition::new(syntax))))
}

impl Command for SendSms {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let to = input
                .matches()
                .get_one::<String>("to")
                .expect("required recipient");
            let text = input
                .matches()
                .get_one::<String>("text")
                .expect("required message");
            let profile = input
                .matches()
                .get_one::<String>("profile")
                .map(String::as_str);
            if input.matches().get_flag("now") {
                let driver = Sms::driver(profile).map_err(console_error)?;
                let message = SmsMessage::new(to, text).map_err(console_error)?;
                driver.send(message).await.map_err(console_error)?;
                output.comment("Demo SMS sent.");
            } else {
                let channel = profile.unwrap_or("sms");
                let recipient = Notify::route("sms", NotificationRoute::Sms(to.into()))
                    .map_err(console_error)?;
                Notify::send(
                    &[&recipient],
                    &DemoNotification {
                        channel: channel.into(),
                        text: text.into(),
                    },
                )
                .await
                .map_err(console_error)?;
                output.comment("Demo SMS dispatched.");
            }
            Ok(0)
        })
    }
}

fn console_error(error: NotificationError) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

#[path = "../../../config/mod.rs"]
mod config;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;

use std::{error::Error, process::ExitCode, sync::Arc};

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, run_application},
    http::HttpFuture,
    notifications::{
        ChannelManager, Notifiable, Notification, NotificationError, NotificationFuture,
        NotificationResult, NotificationRoute,
        channels::SmsChannel,
        sms::{SmsDriver, SmsManager, SmsMessage, SmsTemplate},
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
    if std::env::args_os()
        .nth(1)
        .is_none_or(|mode| mode != "artisan")
    {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root = Application::inferBasePath(Some(env!("CARGO_MANIFEST_DIR")))?;
        Application::configure(root)
            .withConfig(|path, env| {
                let mut config = config::load(path, env)?;
                let sms = config.get_mut("sms").expect("SMS configuration");
                sms["drivers"]["primary"] = json!({
                    "driver": "demo-alpha",
                    "endpoint": env.get_utf8("SMS_ALPHA_URL")?,
                    "token": env.get_utf8("SMS_ALPHA_TOKEN")?,
                });
                sms["drivers"]["backup"] = json!({
                    "driver": "demo-beta",
                    "endpoint": env.get_utf8("SMS_BETA_URL")?,
                    "token": env.get_utf8("SMS_BETA_TOKEN")?,
                });
                sms["drivers"]["aliyun"]["templates"]["login_code"] = json!("SMS_1");
                if let Some(key) = env.get_utf8("SMS_ALERTS_GLOBAL_ACCESS_KEY_ID")? {
                    if env.get_utf8("SMS_TEST_SPLIT_MARKETS")? == Some("1") {
                        sms["drivers"]["aliyun"]["markets"]["international"] = json!({
                            "endpoint": env.get_utf8("SMS_ALERTS_GLOBAL_ENDPOINT")?,
                            "access_key_id": key,
                            "access_key_secret": env.get_utf8("SMS_ALERTS_GLOBAL_ACCESS_KEY_SECRET")?,
                            "sender_id": env.get_utf8("SMS_ALERTS_GLOBAL_SENDER_ID")?,
                            "message_type": env.get_utf8("SMS_ALERTS_GLOBAL_MESSAGE_TYPE")?,
                        });
                    }
                    sms["drivers"]["alerts_global"] = json!({
                        "driver": "aliyun",
                        "market": "international",
                        "endpoint": env.get_utf8("SMS_ALERTS_GLOBAL_ENDPOINT")?,
                        "access_key_id": key,
                        "access_key_secret": env.get_utf8("SMS_ALERTS_GLOBAL_ACCESS_KEY_SECRET")?,
                        "sender_id": env.get_utf8("SMS_ALERTS_GLOBAL_SENDER_ID")?,
                        "message_type": env.get_utf8("SMS_ALERTS_GLOBAL_MESSAGE_TYPE")?,
                    });
                }
                if let Some(key) = env.get_utf8("SMS_ORDERS_CN_API_KEY")? {
                    sms["drivers"]["orders_cn"] = json!({
                        "driver": "yunpian",
                        "api_key": key,
                        "endpoint": env.get_utf8("SMS_ORDERS_CN_ENDPOINT")?,
                    });
                }
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

const PROFILES: [&str; 9] = [
    "log",
    "primary",
    "backup",
    "aliyun",
    "alerts_global",
    "smsbao",
    "yunpian",
    "orders_cn",
    "vonage",
];

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
            for profile in PROFILES {
                channels.extend(profile, Arc::new(SmsChannel::using(profile)));
            }
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
    template: Option<SmsTemplate>,
    named_template: bool,
}

impl Notification for DemoNotification {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec![self.channel.clone()])
    }

    fn to_sms_message(
        &self,
        _: &dyn Notifiable,
        to: String,
    ) -> NotificationResult<Option<SmsMessage>> {
        if let Some(template) = &self.template {
            if self.named_template {
                let openrust::notifications::sms::SmsTemplateParameters::Named(parameters) =
                    &template.parameters
                else {
                    return Err(delivery_error(
                        "A configured template requires named parameters.",
                    ));
                };
                return SmsMessage::named_template(to, &template.id, parameters.clone()).map(Some);
            }
            return SmsMessage::template(to, template.clone()).map(Some);
        }
        SmsMessage::new(to, &self.text).map(Some)
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
        .arg(
            clap::Arg::new("text")
                .required(true)
                .help("Text body or JSON template parameters"),
        )
        .arg(
            clap::Arg::new("template")
                .long("template")
                .help("Provider-approved template ID"),
        )
        .arg(
            clap::Arg::new("named-template")
                .long("named-template")
                .requires("template")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("profile")
                .long("profile")
                .value_parser(PROFILES),
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
            let template = input
                .matches()
                .get_one::<String>("template")
                .map(|id| {
                    serde_json::from_str(text)
                        .map(|parameters| SmsTemplate {
                            id: id.clone(),
                            parameters,
                        })
                        .map_err(|_| {
                            ConsoleError::new(
                                "Template parameters must be a JSON object or array of strings.",
                            )
                        })
                })
                .transpose()?;
            let named_template = input.matches().get_flag("named-template");
            if input.matches().get_flag("now") {
                let driver = match profile {
                    Some(name) => Sms::driver(name),
                    None => Sms::default_driver(),
                }
                .map_err(console_error)?;
                if let Some(template) = template {
                    if named_template {
                        let openrust::notifications::sms::SmsTemplateParameters::Named(parameters) =
                            template.parameters
                        else {
                            return Err(ConsoleError::new(
                                "A configured template requires named parameters.",
                            ));
                        };
                        match profile {
                            Some(_) => driver.send_template(to, template.id, parameters).await,
                            None => Sms::send_template(to, template.id, parameters).await,
                        }
                        .map_err(console_error)?;
                    } else {
                        driver
                            .send(SmsMessage::template(to, template).map_err(console_error)?)
                            .await
                            .map_err(console_error)?;
                    }
                } else if profile.is_some() {
                    driver.send_text(to, text).await.map_err(console_error)?;
                } else {
                    Sms::send_text(to, text).await.map_err(console_error)?;
                }
                output.comment("Demo SMS processed.");
            } else {
                let channel = profile.unwrap_or("sms");
                let recipient = Notify::route("sms", NotificationRoute::Sms(to.into()))
                    .map_err(console_error)?;
                Notify::send(
                    &[&recipient],
                    &DemoNotification {
                        channel: channel.into(),
                        text: text.into(),
                        template,
                        named_template,
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

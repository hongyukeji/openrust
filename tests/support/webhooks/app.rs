#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;
#[path = "../../../bootstrap/packages.rs"]
mod packages;
use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    container::ServiceKey,
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Facade, Route},
};
use openrust_webhook_client::{
    DefaultRespondsTo, DefaultSignatureValidator, RespondsToWebhook, SignatureValidator,
    StandardSignatureValidator, WebhookConfigRepository, WebhookRequest,
};
use openrust_webhook_server::{DefaultSigner, Signer, SigningContext, StandardWebhookSigner};
use serde_json::json;
use std::{collections::BTreeMap, process::ExitCode};
struct Configs;
impl Facade for Configs {
    type Service = WebhookConfigRepository;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<WebhookConfigRepository>()
    }
}
fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
fn verify(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request.route("profile").unwrap_or_default().to_owned();
        let config = Configs::get_facade_root()
            .map_err(internal)?
            .get_config(&name)
            .map_err(internal)?
            .ok_or(HttpError::NotFound)?;
        let uri = request.uri().clone();
        let method = request.method().clone();
        let headers = request.headers().clone();
        let body = axum::body::to_bytes(request.take_body(), config.max_body_bytes())
            .await
            .map_err(|_| HttpError::PayloadTooLarge)?;
        let snapshot = WebhookRequest::new(method, headers, body.to_vec())
            .map_err(internal)?
            .with_uri(uri);
        let validator: &dyn SignatureValidator = if name == "standard" {
            &StandardSignatureValidator
        } else {
            &DefaultSignatureValidator
        };
        if !validator.is_valid(&snapshot, &config).map_err(internal)? {
            return Response::json(StatusCode::FORBIDDEN, json!({"verified":false}));
        }
        // 本夹具只验证公开签名与响应契约，不作为可靠接收端点发布。
        DefaultRespondsTo
            .respond_to_valid_webhook(&snapshot, &config)
            .map_err(internal)
    })
}
struct Sign(CommandDefinition);
impl Command for Sign {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let name = input.matches().get_one::<String>("profile").unwrap();
            let body = input.matches().get_one::<String>("body").unwrap();
            let config = Configs::get_facade_root()
                .map_err(ConsoleError::from_source)?
                .get_config(name)
                .map_err(ConsoleError::from_source)?
                .ok_or_else(|| ConsoleError::new("未知配置"))?;
            let context = SigningContext {
                webhook_url: "https://example.test/callback",
                body: body.as_bytes(),
                secret: config.signing_secret(),
                message_id: "message-fixture",
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
            };
            let signer: &dyn Signer = if name == "standard" {
                &StandardWebhookSigner
            } else {
                &DefaultSigner::default()
            };
            let headers: BTreeMap<_, _> = signer
                .headers(&context)
                .map_err(ConsoleError::from_source)?
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_owned()))
                .collect();
            let environment = application
                .make::<openrust::config::Environment>()
                .await
                .map_err(ConsoleError::from_source)?;
            if let Some(path) = environment
                .get_utf8("WEBHOOK_TEST_OUTPUT")
                .map_err(ConsoleError::from_source)?
            {
                std::fs::write(
                    path,
                    serde_json::to_vec(&json!({"headers":headers}))
                        .map_err(ConsoleError::from_source)?,
                )
                .map_err(ConsoleError::from_source)?;
            }
            output
                .write_raw(&format!("{}\n", json!({"headers":headers})))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
struct Enqueue(CommandDefinition);
impl Command for Enqueue {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let profile = input.matches().get_one::<String>("profile").unwrap();
            let body = input.matches().get_one::<String>("body").unwrap();
            openrust::support::facades::Artisan::queue(
                "webhooks:sign",
                &[profile.into(), body.into()],
            )
            .on_connection("database")
            .on_queue("webhook-primitives")
            .dispatch()
            .await
            .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = |name| {
        clap::Command::new(name)
            .arg(clap::Arg::new("profile").required(true))
            .arg(clap::Arg::new("body").required(true))
    };
    artisan.add_command(Box::new(Sign(CommandDefinition::new(syntax(
        "webhooks:sign",
    )))))?;
    artisan.add_command(Box::new(Enqueue(CommandDefinition::new(syntax(
        "webhooks:enqueue",
    )))))
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("WEBHOOK_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root=Application::infer_base_path(Some(env!("CARGO_MANIFEST_DIR")))?;
        Application::configure(root).with_config(|root,env| {
            let mut config=config::load(root,env)?;
            config.insert("webhook-client".into(),json!({"configs":[
                {"name":"spatie","revision":"one","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"}},
                {"name":"standard","revision":"one","signing_secret":{"env":"FIXTURE_STANDARD_SECRET"}}
            ]}));
            Ok(config)
        }).with_packages(packages::manifest()).with_commands(commands).with_migrations(migrations::entries())
        .with_routing(Routing::default().api(||Route::post("/webhooks/{profile}",verify)))
        .create().map_err(Into::into)
    }).await
}

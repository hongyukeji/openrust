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
struct Databases;
impl Facade for Databases {
    type Service = openrust::database::DatabaseManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<openrust::database::DatabaseManager>()
    }
}
struct ReceiveJob {
    fail: bool,
}
impl openrust_webhook_client::WebhookJobDispatcher for ReceiveJob {
    fn dispatch<'a>(
        &'a self,
        call: &'a openrust_webhook_client::models::WebhookCall,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            if self.fail {
                return Err(openrust_webhook_client::Error::InvalidConfig(
                    "fixture dispatch failure",
                ));
            }
            openrust::support::facades::Artisan::queue(
                "webhooks:record",
                &[call.id().into(), call.scope().into()],
            )
            .on_connection("database")
            .on_queue("webhook-receipts")
            .dispatch()
            .await
            .map_err(|source| openrust_webhook_client::Error::Runtime {
                source: Box::new(source),
            })?;
            Ok(())
        })
    }
}
struct Ignore;
impl openrust_webhook_client::WebhookProfile for Ignore {
    fn should_process(
        &self,
        _: &WebhookRequest,
        _: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<bool> {
        Ok(false)
    }
}
fn receive(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
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
        let bytes = axum::body::to_bytes(request.take_body(), config.max_body_bytes())
            .await
            .map_err(|_| HttpError::PayloadTooLarge)?;
        let snapshot = WebhookRequest::new(method, headers, bytes.to_vec())
            .map_err(internal)?
            .with_uri(uri);
        let store = std::sync::Arc::new(
            openrust_webhook_client::DatabaseWebhookStore::new(
                Databases::get_facade_root()
                    .map_err(internal)?
                    .connection(None)
                    .map_err(internal)?,
                "webhook-acceptance",
            )
            .map_err(internal)?,
        );
        let mut runtime = openrust_webhook_client::WebhookRuntime::new(
            store,
            std::sync::Arc::new(ReceiveJob {
                fail: name == "dispatch-fails",
            }),
            openrust::support::facades::Event::dispatcher().map_err(internal)?,
        );
        if name == "standard" {
            runtime =
                runtime.with_signature_validator(std::sync::Arc::new(StandardSignatureValidator));
        }
        if name == "ignored" {
            runtime = runtime.with_webhook_profile(std::sync::Arc::new(Ignore));
        }
        let config = config
            .as_ref()
            .clone()
            .with_store_headers(&["X-Source", "X-Duplicate", "X-Opaque"])
            .map_err(internal)?;
        match openrust_webhook_client::WebhookProcessor::new(&snapshot, &config, &runtime)
            .process()
            .await
        {
            Err(openrust_webhook_client::Error::InvalidSignature) => {
                Response::json(StatusCode::FORBIDDEN, json!({"verified":false}))
            }
            other => other.map_err(internal),
        }
    })
}
fn attachment_json(value: openrust::http::UploadedFileValue) -> serde_json::Value {
    use openrust::http::UploadedFileValue;
    match value {
        UploadedFileValue::Null => serde_json::Value::Null,
        UploadedFileValue::File(file) => json!({
            "name": file.get_client_original_name(),
            "mime": file.get_client_mime_type(),
            "bytes": file.get().as_ref(),
        }),
        UploadedFileValue::Files(files) => serde_json::Value::Object(
            files
                .into_iter()
                .map(|(key, file)| (key, attachment_json(file)))
                .collect(),
        ),
    }
}
struct Record(CommandDefinition);
impl Command for Record {
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
            let id = input.matches().get_one::<String>("id").unwrap();
            let scope = input.matches().get_one::<String>("scope").unwrap();
            let store = openrust_webhook_client::DatabaseWebhookStore::new(
                Databases::get_facade_root()
                    .map_err(ConsoleError::from_source)?
                    .connection(None)
                    .map_err(ConsoleError::from_source)?,
                scope,
            )
            .map_err(ConsoleError::from_source)?;
            let mut call = openrust_webhook_client::models::WebhookCall::find(&store, id)
                .await
                .map_err(ConsoleError::from_source)?
                .ok_or_else(|| ConsoleError::new("未找到当前范围的接收记录"))?;
            if let Some(value) = input.matches().get_one::<String>("save-exception") {
                call.save_exception(
                    &store,
                    serde_json::from_str(value).map_err(ConsoleError::from_source)?,
                )
                .await
                .map_err(ConsoleError::from_source)?;
            }
            if input.matches().get_flag("clear-exception") {
                call.clear_exception(&store)
                    .await
                    .map_err(ConsoleError::from_source)?;
            }
            let value = read_receipt(&call, &store)
                .await
                .map_err(ConsoleError::from_source)?;
            write_receipt(application, &value)
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{value}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
async fn read_receipt(
    call: &openrust_webhook_client::models::WebhookCall,
    store: &dyn openrust_webhook_client::WebhookStore,
) -> openrust_webhook_client::Result<serde_json::Value> {
    let headers: Vec<_> = call
        .headers()
        .iter()
        .map(|(name, value)| json!([name.as_str(), value.as_bytes()]))
        .collect();
    let payload = call.payload(store).await?;
    let attachments = attachment_json(call.get_attachments(store).await?);
    Ok(
        json!({"payload":payload,"attachments":attachments,"id":call.id(),"scope":call.scope(),"name":call.name(),"revision":call.revision(),"url":call.url(),"method":call.method().as_str(),"body":call.raw_body(),"headers":headers,"exception":call.exception()}),
    )
}
async fn write_receipt(
    application: &Application,
    value: &serde_json::Value,
) -> openrust_webhook_client::Result<()> {
    let env = application.make::<openrust::config::Environment>().await?;
    if let Some(path) = env
        .get_utf8("WEBHOOK_TEST_RECEIPT_OUTPUT")
        .map_err(runtime_error)?
    {
        std::fs::write(path, serde_json::to_vec(value).map_err(runtime_error)?)
            .map_err(runtime_error)?;
    }
    Ok(())
}
fn runtime_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> openrust_webhook_client::Error {
    openrust_webhook_client::Error::Runtime {
        source: Box::new(source),
    }
}
struct FixtureJob;
impl openrust_webhook_client::jobs::ProcessWebhookJob for FixtureJob {
    fn handle<'a>(
        &'a self,
        context: &'a mut openrust::queue::JobContext<'_>,
        call: &'a openrust_webhook_client::models::WebhookCall,
        store: &'a dyn openrust_webhook_client::WebhookStore,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            assert_eq!(context.connection(), "database");
            assert_eq!(context.queue(), "webhook-receipts");
            if (call.raw_body() == b"retry-binary-receipt" && context.attempts() == 1)
                || call.raw_body() == b"terminal-binary-receipt"
            {
                return Err(runtime_error(std::io::Error::other(
                    "private-business-error-sentinel",
                )));
            }
            let app = openrust::support::facades::App::get_facade_root()?;
            write_receipt(&app, &read_receipt(call, store).await?).await
        })
    }
}
struct FixtureProvider;
impl openrust::support::ServiceProvider for FixtureProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            use openrust_webhook_client::WebhookRuntime;
            for name in ["fixture", "fixture-custom", "unregistered"] {
                WebhookRuntime::register_job_dispatcher(
                    openrust::contracts::foundation::Application::container(app),
                    name,
                    move |config, _| {
                        Ok(std::sync::Arc::new(
                            openrust_webhook_client::jobs::QueuedWebhookJob::new(name)?
                                .on_connection(if config.name() == "discarded" {
                                    "null"
                                } else {
                                    "database"
                                })
                                .on_queue("webhook-receipts")
                                .with_options(openrust::queue::JobOptions {
                                    max_tries: Some(2),
                                    timeout: Some(std::time::Duration::from_secs(12)),
                                    ..Default::default()
                                }),
                        ))
                    },
                )?;
            }
            WebhookRuntime::register_job_dispatcher(
                openrust::contracts::foundation::Application::container(app),
                "factory-fails",
                |_, _| {
                    Err(openrust_webhook_client::Error::InvalidConfig(
                        "fixture job factory",
                    ))
                },
            )?;
            WebhookRuntime::register_webhook_profile(
                openrust::contracts::foundation::Application::container(app),
                "ignored",
                |_, _| Ok(std::sync::Arc::new(Ignore)),
            )?;
            WebhookRuntime::register_signature_validator(
                openrust::contracts::foundation::Application::container(app),
                "custom",
                |_, container| {
                    use openrust::contracts::container::Container as _;
                    Ok(std::sync::Arc::new(CustomValidator {
                        request: container.get("request")?,
                    }))
                },
            )?;
            WebhookRuntime::register_webhook_response(
                openrust::contracts::foundation::Application::container(app),
                "custom",
                |_, _| Ok(std::sync::Arc::new(CustomResponse)),
            )?;
            WebhookRuntime::register_webhook_store(
                openrust::contracts::foundation::Application::container(app),
                "custom",
                |config, container| {
                    use openrust::contracts::container::Container as _;
                    let manager = container.make::<openrust::database::DatabaseManager>()?;
                    Ok(std::sync::Arc::new(
                        openrust_webhook_client::DatabaseWebhookStore::new(
                            manager.connection(None)?,
                            config.options()["scope"].as_str().unwrap(),
                        )?,
                    ))
                },
            )?;
            Ok(())
        })
    }
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            let queue = app.make::<openrust::queue::QueueManager>().await?;
            let manager = app.make::<openrust::database::DatabaseManager>().await?;
            for (name, scope) in [
                ("fixture", "webhook-acceptance"),
                ("fixture-custom", "webhook-custom"),
            ] {
                let store = openrust_webhook_client::DatabaseWebhookStore::new(
                    manager.connection(None)?,
                    scope,
                )?;
                openrust_webhook_client::jobs::QueuedWebhookJob::register(
                    &queue,
                    name,
                    std::sync::Arc::new(store),
                    std::sync::Arc::new(FixtureJob),
                )?;
            }
            Ok(())
        })
    }
}
struct CustomValidator {
    request: std::sync::Arc<openrust::support::facades::RequestHandle>,
}
impl SignatureValidator for CustomValidator {
    fn is_valid(
        &self,
        request: &WebhookRequest,
        config: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<bool> {
        assert!(self.request.with(|request| {
            request
                .route_name()
                .unwrap_or_default()
                .contains("webhook-client-custom")
        })?);
        DefaultSignatureValidator.is_valid(request, config)
    }
}
struct CustomResponse;
impl RespondsToWebhook for CustomResponse {
    fn respond_to_valid_webhook(
        &self,
        _: &WebhookRequest,
        config: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<Response> {
        Response::json(StatusCode::ACCEPTED, json!({"profile":config.name()})).map_err(|source| {
            openrust_webhook_client::Error::Runtime {
                source: Box::new(source),
            }
        })
    }
}
struct ResolverSwap;
impl openrust::contracts::http::Middleware for ResolverSwap {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: openrust::http::Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        request.set_route_resolver(openrust::http::RouteResolver(std::sync::Arc::new(|| {
            openrust::http::ResolvedRoute {
                name: Some("webhook-client-missing-config".into()),
                ..Default::default()
            }
        })));
        assert_eq!(request.route_name(), Some("webhook-client-missing-config"));
        Box::pin(async move { Ok(next.run(request).await) })
    }
}
struct Stamp;
impl openrust::contracts::http::Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: openrust::http::Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Ok(next.run(request).await.header(
                "x-webhook-group".parse().unwrap(),
                "preserved".parse().unwrap(),
            ))
        })
    }
}
fn api_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust_webhook_client::{WebhookRouteOptions as Options, WebhookRouter, WebhookRoutes};
    Route::post("/webhooks/{profile}", verify)?;
    Route::post("/received/{profile}", receive)?;
    Route::webhooks("/package/default", Options::default())?;
    Route::webhooks("/package/unregistered", Options::new().name("unregistered"))?;
    Route::webhooks("/package/discarded", Options::new().name("discarded"))?;
    Route::webhooks("/package/post", Options::new().name("spatie"))?;
    Route::webhooks(
        "/package/no-attachments",
        Options::new().name("no-attachments"),
    )?;
    Route::webhooks("/package/standard", Options::new().name("standard"))?;
    Route::webhooks(
        "/package/methods",
        Options::new()
            .name("spatie")
            .methods(["get", "query", "post", "put", "patch", "delete"]),
    )?;
    Route::webhooks("/package/provider/{account}", Options::new().name("spatie"))?;
    Route::webhooks("/package/custom", Options::new().name("custom"))?;
    Route::webhooks("/package/ignored", Options::new().name("package-ignored"))?;
    Route::webhooks("/package/unknown", Options::new().name("missing-config"))?;
    Route::webhooks(
        "/package/factory-fails",
        Options::new().name("factory-fails"),
    )?;
    Route::webhooks_with(
        "/package/resolved",
        Options::new().name("spatie"),
        |route| {
            route.middleware(std::sync::Arc::new(ResolverSwap));
            Ok(())
        },
    )?;
    Route::webhooks("/package/overlap", Options::new().name("default"))?;
    Route::webhooks("/package/overlap", Options::new().name("spatie"))?;
    Route::group(
        openrust::routing::RouteGroup::new()
            .prefix("package/group")
            .name("team.")
            .middleware(std::sync::Arc::new(Stamp)),
        |router| {
            router.webhooks("/direct", Options::new().name("spatie"))?;
            Ok(())
        },
    )?;
    Route::group(
        openrust::routing::RouteGroup::new()
            .prefix("package/domain")
            .name("domain.")
            .domain("{tenant}.webhook.test"),
        |router| {
            router.webhooks("/callback", Options::new().name("spatie"))?;
            Ok(())
        },
    )?;
    Route::webhooks_with(
        "/package/optional/{id?}",
        Options::new().name("spatie"),
        |route| {
            route.defaults("id", "42").where_number("id")?;
            Ok(())
        },
    )?;
    // 注册期无效方法必须失败且不留下半条路由。
    for method in ["POST", "head", "options", "trace", "connect", ""] {
        assert!(
            Route::webhooks(
                "/package/must-not-register",
                Options::new().methods([method])
            )
            .is_err()
        );
    }
    Ok(())
}
fn web_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust_webhook_client::{WebhookRouteOptions as Options, WebhookRoutes};
    Route::webhooks("/webhook-web/protected", Options::new().name("spatie"))?;
    Route::webhooks("/webhook-web/except", Options::new().name("spatie"))?;
    Route::post("/webhook-web/ordinary", ordinary)
}
fn ordinary(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, json!({"ordinary":true})) })
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
    )))))?;
    artisan.add_command(Box::new(Record(CommandDefinition::new(
        clap::Command::new("webhooks:record")
            .arg(clap::Arg::new("id").required(true))
            .arg(clap::Arg::new("scope").required(true))
            .arg(clap::Arg::new("save-exception").long("save-exception"))
            .arg(
                clap::Arg::new("clear-exception")
                    .long("clear-exception")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))
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
            config.insert("webhook-client".into(),json!({"add_unique_token_to_route_name": env.get_utf8("FIXTURE_UNIQUE_ROUTES")?==Some("true"),"configs":[
                {"name":"spatie","revision":"one","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"no-attachments","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","store_attachments":false},
                {"name":"standard","revision":"one","signing_secret":{"env":"FIXTURE_STANDARD_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"standard","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"ignored","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"}},
                {"name":"dispatch-fails","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"}},
                {"name":"default","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"custom","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"signature_header_name":"X-Webhook","scope":"webhook-custom","process_webhook_job":"fixture-custom","signature_validator":"custom","webhook_model":"custom","webhook_response":"custom","webhook_profile":"process-everything"},
                {"name":"package-ignored","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"webhook_profile":"ignored","webhook_model":"missing","process_webhook_job":"fixture","signature_validator":"default"},
                {"name":"discarded","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"unregistered","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"unregistered","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"factory-fails","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"factory-fails","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"}
            ]}));
            Ok(config)
        }).with_packages(packages::manifest()).with_providers(vec![Box::new(FixtureProvider)]).with_commands(commands).with_migrations(migrations::entries())
        .with_middleware(|middleware| { middleware.prevent_request_forgery(["webhook-web/except"],false,false); })
        .with_routing(Routing::default().api(api_routes).web(web_routes))
        .create().map_err(Into::into)
    }).await
}

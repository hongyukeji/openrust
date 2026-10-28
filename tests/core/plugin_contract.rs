//! 通过公开 API 验证统一 Provider 宿主的注册、启动与覆盖契约。

use figment::{Figment, providers::Serialized};
use openrust::{
    Command,
    config::FrameworkConfig,
    core::{
        App, BootContext, BoxFuture, Error, ProviderBuilder, ProviderId, RegisterContext, Result,
        ServiceProvider,
    },
    http::{HttpServiceProvider, Inject, axum, build_router},
};
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tower::ServiceExt;

struct Greeting(&'static str);
#[derive(Deserialize)]
struct ConsumerConfig {
    label: String,
}
#[derive(Deserialize)]
struct InvalidConfig {
    port: u16,
}

async fn provider_route(Inject(greeting): Inject<Greeting>) -> String {
    greeting.0.to_owned()
}
struct GreetingProvider;
impl ServiceProvider for GreetingProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(Greeting("provided")));
        Ok(())
    }
}
struct ConsumerProvider;
impl ServiceProvider for ConsumerProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.singleton::<ConsumerConfig, _>(|resolver| {
            Box::pin(async move { Ok(Arc::new(resolver.section::<ConsumerConfig>("consumer")?)) })
        });
        app.instance(Arc::new(AtomicBool::new(false)));
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.resolve::<Greeting>().await?.0 != "provided" {
                return Err(Error::custom(
                    "unexpected_provider",
                    "provider value differs",
                ));
            }
            if app.resolve::<ConsumerConfig>().await?.label != "independent" {
                return Err(Error::custom("invalid_consumer_config", "label differs"));
            }
            app.command(PingCommand);
            app.routes(|routes| routes.get("/plugin-contract", provider_route))?;
            Ok(())
        })
    }
}
struct PingCommand;
impl Command for PingCommand {
    fn name(&self) -> &'static str {
        "plugin ping"
    }
    fn about(&self) -> &'static str {
        "标记测试命令已经执行"
    }
    fn run<'a>(&'a self, app: &'a App, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.state()
                .resolve::<AtomicBool>()
                .await?
                .store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}
fn sources(source: Figment) -> Figment {
    Figment::from(Serialized::defaults(FrameworkConfig::default())).merge(source)
}
async fn start(builder: ProviderBuilder, source: Figment) -> Result<App> {
    builder
        .start(sources(source))
        .await
        .and_then(App::from_provider_runtime)
}
#[tokio::test]
async fn independent_provider_registers_config_services_command_and_route()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut builder = ProviderBuilder::new();
    // consumer 位于 greeting 之前，证明 register 屏障后 boot 可解析后序绑定。
    builder
        .provider(ConsumerProvider)
        .provider(HttpServiceProvider)
        .provider(GreetingProvider);
    let app = start(
        builder,
        Figment::new().merge(("consumer.label", "independent")),
    )
    .await?;
    let response = build_router(&app)?
        .oneshot(
            axum::http::Request::builder()
                .uri("/plugin-contract")
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        app.state().resolve::<ConsumerConfig>().await?.label,
        "independent"
    );
    app.run_command("plugin", vec!["ping".into()]).await?;
    assert!(
        app.state()
            .resolve::<AtomicBool>()
            .await?
            .load(Ordering::SeqCst)
    );
    Ok(())
}
struct MissingProvider;
impl ServiceProvider for MissingProvider {
    fn dependencies(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<GreetingProvider>()]
    }
}
#[tokio::test]
async fn missing_provider_dependency_fails_before_boot() {
    let mut builder = ProviderBuilder::new();
    builder.provider(MissingProvider);
    let error = start(builder, Figment::new()).await.err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("provider_dependency_missing")
    );
}
struct FirstValue;
impl ServiceProvider for FirstValue {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(String::from("first")));
        Ok(())
    }
}
struct SecondValue;
impl ServiceProvider for SecondValue {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(String::from("second")));
        Ok(())
    }
}
#[tokio::test]
async fn later_provider_instance_replaces_previous_binding() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(FirstValue).provider(SecondValue);
    let app = start(builder, Figment::new()).await?;
    assert_eq!(&*app.state().resolve::<String>().await?, "second");
    Ok(())
}
struct RepeatedValue;
impl ServiceProvider for RepeatedValue {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(String::from("first")));
        app.instance(Arc::new(String::from("second")));
        Ok(())
    }
}
#[tokio::test]
async fn repeated_instance_registration_replaces_by_type_deterministically() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(RepeatedValue);
    let app = start(builder, Figment::new()).await?;
    assert_eq!(&*app.state().resolve::<String>().await?, "second");
    Ok(())
}
struct RepeatedCommand;
impl ServiceProvider for RepeatedCommand {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(AtomicBool::new(false)));
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(PingCommand);
            app.command(PingCommand);
            Ok(())
        })
    }
}
#[tokio::test]
async fn repeated_command_registration_keeps_one_callable_command() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(RepeatedCommand);
    let app = start(builder, Figment::new()).await?;
    assert_eq!(
        app.commands()
            .filter(|command| command.name() == "plugin ping")
            .count(),
        1
    );
    app.run_command("plugin", vec!["ping".into()]).await?;
    assert!(
        app.state()
            .resolve::<AtomicBool>()
            .await?
            .load(Ordering::SeqCst)
    );
    Ok(())
}
struct InvalidConfigProvider;
impl ServiceProvider for InvalidConfigProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config: InvalidConfig = app.section("consumer")?;
            let _ = config.port;
            Ok(())
        })
    }
}
#[tokio::test]
async fn invalid_provider_config_reports_key_without_echoing_value() {
    let secret = "must-not-appear-in-error";
    let mut builder = ProviderBuilder::new();
    builder.provider(InvalidConfigProvider);
    let error = start(builder, Figment::new().merge(("consumer.port", secret)))
        .await
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        error.contains("consumer.port"),
        "configuration error was: {error}"
    );
    assert!(
        !error.contains(secret),
        "configuration error leaked its value: {error}"
    );
}
async fn duplicate_route() -> &'static str {
    "duplicate"
}
struct RepeatedRoutes;
impl ServiceProvider for RepeatedRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| routes.get("/duplicate", duplicate_route))?;
            app.routes(|routes| routes.get("/duplicate", duplicate_route))?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn duplicate_provider_routes_return_startup_error_instead_of_panicking() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(RepeatedRoutes);
    let app = start(builder, Figment::new()).await?;
    let error = build_router(&app).err().ok_or_else(|| {
        Error::custom(
            "test_expected_error",
            "duplicate routes unexpectedly succeeded",
        )
    })?;
    assert_eq!(error.code(), "http_route_registration_failed");
    Ok(())
}
struct MarkCommand(bool);
impl Command for MarkCommand {
    fn name(&self) -> &'static str {
        "replace me"
    }
    fn about(&self) -> &'static str {
        "测试命令替换"
    }
    fn run<'a>(&'a self, _: &'a App, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.0 {
                Ok(())
            } else {
                Err(Error::custom("old_command_ran", "old command ran"))
            }
        })
    }
}
struct ReplacedCommand;
impl ServiceProvider for ReplacedCommand {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(MarkCommand(false));
            app.command(MarkCommand(true));
            Ok(())
        })
    }
}
#[tokio::test]
async fn provider_command_registration_replaces_by_name_deterministically() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(ReplacedCommand);
    let app = start(builder, Figment::new()).await?;
    app.run_command("replace", vec!["me".into()]).await
}

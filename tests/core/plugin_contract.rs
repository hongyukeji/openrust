//! 通过公开 `openrust` API 验证独立插件宿主的注册和启动契约。

use std::sync::atomic::{AtomicBool, Ordering};

use figment::Figment;
use openrust::config::FrameworkConfig;
use openrust::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};
use openrust::http::{AppBuilderHttpExt, HttpPlugin, Inject, build_router};
use openrust::lifecycle::finalize;
use openrust::{Command, http::axum};
use serde::Deserialize;
use tower::ServiceExt;

#[derive(Clone)]
struct Greeting(&'static str);

#[derive(Debug, Deserialize)]
struct ConsumerConfig {
    label: String,
}

#[derive(Deserialize)]
struct InvalidConfig {
    port: u16,
}

async fn plugin_route(Inject(greeting): Inject<Greeting>) -> String {
    greeting.0.to_owned()
}

struct ProviderPlugin;

impl Plugin for ProviderPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.try_insert(Greeting("provided"))?;
        Ok(())
    }
}

struct ConsumerPlugin;

impl Plugin for ConsumerPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app.section::<ConsumerConfig>("consumer")?;
        let greeting = app.try_get::<Greeting>()?;
        app.try_insert(config)?;
        app.try_insert(AtomicBool::new(false))?;
        app.try_command(PingCommand)?;
        app.routes(|routes| routes.get("/plugin-contract", plugin_route));
        if greeting.0 != "provided" {
            return Err(Error::custom(
                "unexpected_provider",
                "provider value differs",
            ));
        }
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.state().try_get::<ConsumerConfig>()?;
            if config.label != "independent" {
                return Err(Error::custom("invalid_consumer_config", "label differs"));
            }
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
        "Mark the consumer command as executed"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.state()
                .try_get::<AtomicBool>()?
                .store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}

fn builder(figment: Figment) -> AppBuilder {
    AppBuilder::new(FrameworkConfig::default(), figment)
}

#[tokio::test]
async fn independent_plugin_registers_config_services_command_and_route()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let figment = Figment::new().merge(("consumer.label", "independent"));
    let mut builder = builder(figment);
    builder
        .plugin(ProviderPlugin)
        .plugin(HttpPlugin)
        .plugin(ConsumerPlugin);
    let app = finalize(builder).await?;

    let router = build_router(&app)?;
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/plugin-contract")
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        app.state().try_get::<ConsumerConfig>()?.label,
        "independent"
    );

    app.run_command("plugin", vec!["ping".into()]).await?;
    assert!(app.state().try_get::<AtomicBool>()?.load(Ordering::SeqCst));
    Ok(())
}

struct MissingProviderPlugin;

impl Plugin for MissingProviderPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let _ = app.try_get::<Greeting>()?;
        Ok(())
    }
}

#[tokio::test]
async fn missing_plugin_dependency_fails_during_build() {
    let mut builder = builder(Figment::new());
    builder.plugin(MissingProviderPlugin);
    let error = finalize(builder).await.err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("plugin_dependency_missing")
    );
}

struct DuplicateServicePlugin;

impl Plugin for DuplicateServicePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.try_insert(String::from("first"))?;
        app.try_insert(String::from("second"))?;
        Ok(())
    }
}

#[tokio::test]
async fn duplicate_service_registration_fails_without_replacing_first_value() -> Result<()> {
    let mut builder = builder(Figment::new());
    builder.plugin(DuplicateServicePlugin);
    let error = finalize(builder).await.err().ok_or_else(|| {
        Error::custom(
            "test_expected_error",
            "duplicate service unexpectedly succeeded",
        )
    })?;
    assert_eq!(error.code(), "duplicate_service_registration");
    Ok(())
}

struct LegacyServiceRegistration;

impl Plugin for LegacyServiceRegistration {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.insert(String::from("first"));
        app.insert(String::from("second"));
        Ok(())
    }
}

#[tokio::test]
async fn legacy_service_registration_replaces_by_type_deterministically() -> Result<()> {
    let mut builder = builder(Figment::new());
    builder.plugin(LegacyServiceRegistration);
    let app = finalize(builder).await?;
    assert_eq!(&*app.state().try_get::<String>()?, "second");
    Ok(())
}

struct DuplicateCommandPlugin;

impl Plugin for DuplicateCommandPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.try_command(PingCommand)?;
        app.try_command(PingCommand)?;
        Ok(())
    }
}

#[tokio::test]
async fn duplicate_command_registration_fails_during_build() {
    let mut builder = builder(Figment::new());
    builder.plugin(DuplicateCommandPlugin);
    let error = finalize(builder).await.err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("duplicate_command_registration")
    );
}

struct InvalidConfigPlugin;

impl Plugin for InvalidConfigPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config: InvalidConfig = app.section("consumer")?;
        let _ = config.port;
        Ok(())
    }
}

#[tokio::test]
async fn invalid_plugin_config_reports_key_without_echoing_value() {
    let secret = "must-not-appear-in-error";
    let figment = Figment::new().merge(("consumer.port", secret));
    let mut builder = builder(figment);
    builder.plugin(InvalidConfigPlugin);
    let error = finalize(builder)
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

#[tokio::test]
async fn duplicate_plugin_routes_return_startup_error_instead_of_panicking() -> Result<()> {
    let mut builder = builder(Figment::new());
    builder
        .plugin(HttpPlugin)
        .routes(|routes| routes.get("/duplicate", duplicate_route));
    builder.routes(|routes| routes.get("/duplicate", duplicate_route));
    let app = finalize(builder).await?;
    let error = build_router(&app).err().ok_or_else(|| {
        Error::custom(
            "test_expected_error",
            "duplicate routes unexpectedly succeeded",
        )
    })?;
    assert_eq!(error.code(), "http_route_registration_failed");
    Ok(())
}

#[tokio::test]
async fn legacy_command_registration_replaces_by_name_deterministically() -> Result<()> {
    struct MarkCommand(bool);

    impl Command for MarkCommand {
        fn name(&self) -> &'static str {
            "replace me"
        }
        fn about(&self) -> &'static str {
            "test replacement behavior"
        }
        fn run<'a>(&'a self, _app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                if self.0 {
                    Ok(())
                } else {
                    Err(Error::custom("old_command_ran", "old command ran"))
                }
            })
        }
    }

    let mut builder = builder(Figment::new());
    builder
        .command(MarkCommand(false))
        .command(MarkCommand(true));
    let app = builder.build();
    app.run_command("replace", vec!["me".into()]).await
}

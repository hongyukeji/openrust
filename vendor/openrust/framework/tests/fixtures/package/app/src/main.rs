use std::{collections::BTreeMap, env, error::Error, path::Path, process::ExitCode};

use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::foundation::Application as _,
    foundation::{Application, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{ServiceProvider, facades::Route},
};
use serde_json::json;

mod packages;

fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let database = env::var("PROBE_MYSQL_URL").ok().map_or_else(
        || {
            json!({
                "default": "sqlite",
                "connections": {"sqlite": {"driver": "sqlite", "database": "probe.sqlite"}},
            })
        },
        |url| {
            json!({
                "default": "mysql",
                "connections": {"mysql": {"driver": "mysql", "url": url}},
            })
        },
    );
    Ok(BTreeMap::from([
        (
            "app".to_owned(),
            json!({"host": "127.0.0.1", "port": 0, "env": "testing"}),
        ),
        ("database".to_owned(), database),
        (
            "probe".to_owned(),
            json!({
                "shallow": {"options": {"app": 2}},
                "recursive": {"options": {"app": 2}, "indices": ["override"]},
            }),
        ),
    ]))
}

struct HostProvider;

impl ServiceProvider for HostProvider {
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            let config = app.config()?;
            if app
                .package_manifest()
                .packages()
                .iter()
                .any(|p| p.name == "package-probe")
            {
                assert_eq!(
                    config.get("probe"),
                    Some(json!({
                        "shallow": {"enabled": true, "options": {"app": 2}},
                        "recursive": {"options": {"package": 1, "app": 2}, "indices": ["override", "second"]},
                    })),
                );
            }
            self.load_routes_from(app, health)
        })
    }
}

fn health() -> Result<(), RoutingError> {
    Route::get("/health", ready)?;
    Ok(())
}

fn ready(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::new(StatusCode::OK, "ready".into())) })
}

fn application() -> Result<Application, Box<dyn Error + Send + Sync>> {
    Ok(Application::configure(env::current_dir()?)
        .with_config(config)
        .with_packages(packages::manifest())
        .with_providers(vec![Box::new(HostProvider)])
        .create()?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    run_application(application).await
}

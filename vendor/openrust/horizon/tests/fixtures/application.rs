//! Dedicated process acceptance application; never registered in the default app.
use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::foundation::Application as _,
    contracts::queue::JobHandler,
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request},
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueManager, QueueResult},
    support::{ProviderResult, ServiceProvider},
};
use openrust_horizon::{Horizon, HorizonServiceProvider};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
fn config(base: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing","debug":false})),
        (
            "database".into(),
            json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}},"redis":{"options":{"prefix":format!("{}:",base.file_name().unwrap().to_str().unwrap())},"default":{"url":std::env::var("HORIZON_TEST_REDIS_URL").unwrap()}}}),
        ),
        (
            "cache".into(),
            json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache")}}}),
        ),
        (
            "queue".into(),
            json!({"default":"redis","connections":{"redis":{"driver":"redis","connection":"default","queue":"default","retry_after":30}},"failed":{"driver":"database-uuids","database":"sqlite","table":"failed_jobs"}}),
        ),
        (
            "horizon".into(),
            json!({"retention":3600,"fast_termination":std::env::var("HORIZON_PROBE_FAST_TERMINATION").as_deref()==Ok("1"),"memory_limit":std::env::var("HORIZON_PROBE_MEMORY_LIMIT").ok().and_then(|value|value.parse::<u64>().ok()).unwrap_or(64),"defaults":{"supervisor-1":{"timeout":5,"sleep":1,"balanceCooldown":1,"memory":1024,"maxProcesses":2,"maxJobs":3,"workersName":"horizon-probe","tries":2,"backoff":[1,2],"rest":1,"force":true,"json":true,"nice":1}},"environments":{"testing":{}}}),
        ),
    ]))
}
struct Probe(std::path::PathBuf);
impl JobHandler for Probe {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let data = payload.data();
            if data.get("fail").and_then(Value::as_bool) == Some(true) {
                return Err(QueueError::InvalidPayload);
            }
            tokio::time::sleep(Duration::from_millis(
                data.get("sleep_ms").and_then(Value::as_u64).unwrap_or(20),
            ))
            .await;
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.0)
                    .unwrap(),
                "{}",
                payload.uuid()
            )
            .unwrap();
            Ok(())
        })
    }
}
struct ProbeProvider;
impl ServiceProvider for ProbeProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<QueueManager>().await?.register(
                "probe",
                Arc::new(Probe(app.base_path().join("completed.log"))),
            )?;
            Ok(())
        })
    }
}
fn authorize(request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    let allowed = request
        .headers()
        .get("x-test-operator")
        .is_some_and(|v| v == "operator");
    Box::pin(async move { Ok(allowed) })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::auth(authorize).unwrap();
    Horizon::routes()
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    let base = std::env::var("APP_BASE_PATH")?;
    Ok(Application::configure(base.into())
        .with_config(config)
        .with_providers(vec![
            Box::new(HorizonServiceProvider),
            Box::new(ProbeProvider),
        ])
        .with_routing(Routing::default().web(routes))
        .create()?)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Controlled process-inspector fixture. It keeps the real executable and
    // Horizon argv visible after the launcher exits, without worker self-reaping.
    if std::env::var("HORIZON_PROBE_HOLD").as_deref() == Ok("1") {
        #[cfg(unix)]
        {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .unwrap()
                .recv()
                .await;
            return std::process::ExitCode::SUCCESS;
        }
        #[cfg(not(unix))]
        return std::process::ExitCode::FAILURE;
    }
    if let Ok(data) = std::env::var("HORIZON_PROBE_DISPATCH") {
        let mut app = create().unwrap();
        app.boot().await.unwrap();
        let manager = app.make::<QueueManager>().await.unwrap();
        let payload = Payload::new("probe", serde_json::from_str::<Value>(&data).unwrap()).unwrap();
        let id = manager
            .connection(None)
            .unwrap()
            .push(&payload, None)
            .await
            .unwrap()
            .expect("Redis transport ID");
        println!(
            "{}",
            if std::env::var("HORIZON_PROBE_DISPATCH_UUID").as_deref() == Ok("1") {
                payload.uuid()
            } else {
                &id
            }
        );
        return std::process::ExitCode::SUCCESS;
    }
    run_application(create).await
}

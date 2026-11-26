//! Isolated HTTP acceptance application, never registered in the default skeleton.
use openrust::{
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response},
    queue::{
        DispatchOptions, JobContext, Payload, QueueFacade, QueueFuture, QueueManager, QueueResult,
    },
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
};
use openrust_telescope::{IncomingEntry, Telescope, TelescopeServiceProvider};
use serde_json::json;
use std::sync::Arc;
struct PendingProbe;

impl openrust::contracts::queue::JobHandler for PendingProbe {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        _: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct ProbeProvider;
impl ServiceProvider for ProbeProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Telescope>().await?.auth(|request| {
                let allowed = request
                    .headers()
                    .get("x-test-operator")
                    .is_some_and(|value| value == "operator");
                Box::pin(async move { Ok(allowed) })
            });
            let queue = app.make::<QueueManager>().await?;
            queue
                .register("TelescopePendingDesktop", Arc::new(PendingProbe))
                .unwrap();
            queue
                .register("TelescopePendingMobile", Arc::new(PendingProbe))
                .unwrap();
            Ok(())
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/probe", probe)?;
    Route::get("/dump/{label}", dump_probe)?;
    Route::get("/family/{kind}/{sample}/{label}", family_probe)?;
    Route::get("/pending/{sample}", pending_probe)?;
    Ok(())
}
fn pending_probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let name = if request.route("sample") == Some("mobile") {
        "TelescopePendingMobile"
    } else {
        "TelescopePendingDesktop"
    };
    Box::pin(async move {
        QueueFacade::dispatch(
            Payload::new(name, json!({})).unwrap(),
            DispatchOptions {
                connection: Some("async".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        Ok(Response::from("queued"))
    })
}

fn family_probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let job = request.route("kind") == Some("job");
    let tag = format!("family-{}", request.route("sample").unwrap_or_default());
    let label = request.route("label").unwrap_or_default().to_owned();
    Box::pin(async move {
        let family = (label != "standalone").then(|| "fixture-family".to_owned());
        let entry = IncomingEntry::make(
            json!({"name":label,"status":"processed","listeners":[]})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap()
        .with_family_hash(family)
        .tags([tag]);
        let telescope = openrust_telescope::facades::Telescope::get_facade_root().unwrap();
        if job {
            telescope.record_job(entry);
        } else {
            telescope.record_event(entry);
        }
        Ok(Response::from("recorded"))
    })
}
fn dump_probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let label = request.route("label").unwrap_or_default().to_owned();
    Box::pin(async move {
        openrust::support::dump(&format!(
            "live telescope {label} <img src=x onerror=window.dumpCompromised=true>"
        ))
        .await
        .unwrap();
        Ok(Response::from("dumped"))
    })
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        openrust_telescope::facades::Telescope::get_facade_root().unwrap().record_event(IncomingEntry::make(json!({"name":"<img src=x onerror=window.compromised=true>","payload":{"actual":"observed"}}).as_object().unwrap().clone()).unwrap());
        Ok(Response::from("observed"))
    })
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(std::env::var("APP_BASE_PATH")?.into())
        .with_config(|base, _| Ok(std::collections::BTreeMap::from([
            ("app".into(),json!({"env":"local","debug":false,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
            ("database".into(),json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}}})),
            ("cache".into(),json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache")}}})),
            ("session".into(),json!({"driver":"file","files":base.join("sessions"),"cookie":"telescope-acceptance","secure":false})),
            ("queue".into(),json!({"default":"async","connections":{"async":{"driver":"database","connection":"sqlite","table":"jobs","queue":"default","retry_after":30}}})),
            ("telescope".into(),json!({"path":"diagnostics/telescope","domain":"127.0.0.1","enabled":std::env::var("TELESCOPE_PROBE_DISABLED").as_deref()!=Ok("1")})),
        ])))
        .with_providers(vec![Box::new(TelescopeServiceProvider),Box::new(ProbeProvider)])
        .with_routing(Routing::default().api(routes))
        .create()?)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_application(create).await
}

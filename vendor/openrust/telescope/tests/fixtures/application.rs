//! Isolated HTTP acceptance application, never registered in the default skeleton.
use openrust::{
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
};
use openrust_telescope::{IncomingEntry, Telescope, TelescopeServiceProvider};
use serde_json::json;
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
            Ok(())
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/probe", probe)?;
    Ok(())
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        openrust_telescope::facades::Telescope::getFacadeRoot().unwrap().recordEvent(IncomingEntry::make(json!({"name":"<img src=x onerror=window.compromised=true>","payload":{"actual":"observed"}}).as_object().unwrap().clone()).unwrap());
        Ok(Response::from("observed"))
    })
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(std::env::var("APP_BASE_PATH")?.into())
        .withConfig(|base, _| Ok(std::collections::BTreeMap::from([
            ("app".into(),json!({"env":"local","debug":false,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
            ("database".into(),json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}}})),
            ("cache".into(),json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache")}}})),
            ("session".into(),json!({"driver":"file","files":base.join("sessions"),"cookie":"telescope-acceptance","secure":false})),
            ("telescope".into(),json!({"path":"diagnostics/telescope","domain":"127.0.0.1","enabled":std::env::var("TELESCOPE_PROBE_DISABLED").as_deref()!=Ok("1")})),
        ])))
        .withProviders(vec![Box::new(TelescopeServiceProvider),Box::new(ProbeProvider)])
        .withRouting(Routing::default().api(routes))
        .create()?)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_application(create).await
}

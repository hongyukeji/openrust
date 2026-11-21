//! Native HTTP acceptance host; never installed in the application skeleton.
use openrust::{
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response},
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use serde_json::json;
struct Operator;
impl openrust::contracts::auth::Authenticatable for Operator {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        42_u64.into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
struct Access;
impl ServiceProvider for Access {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<openrust::auth::AuthManager>()
                .await?
                .viaRequest("pulse-test", |request| {
                    let user = request
                        .headers()
                        .get("x-test-operator")
                        .is_some_and(|v| v == "operator")
                        .then(|| {
                            Box::new(Operator)
                                as Box<dyn openrust::contracts::auth::Authenticatable>
                        });
                    Box::pin(async move { Ok(user) })
                });
            let gate = app.make::<openrust::auth::access::GateRegistry>().await?;
            gate.defineGuest("viewPulse", |user, _| {
                Box::pin(async move {
                    Ok(if user.is_some() {
                        openrust::auth::access::Response::allow()
                    } else {
                        openrust::auth::access::Response::deny("Forbidden")
                    })
                })
            });
            Ok(())
        })
    }
}
fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust_pulse::facades::Pulse;
        let unsafe_text = "<img src=x onerror=window.compromised=true>";
        Pulse::record(
            "slow_request",
            json!(["GET", unsafe_text, "fixture::probe"]).to_string(),
            Some(1234),
            None,
        )
        .await
        .map_err(internal)?
        .count()
        .max();
        Pulse::record("user_request", "42", None, None)
            .await
            .map_err(internal)?
            .count();
        Pulse::record("cache_hit", unsafe_text, None, None)
            .await
            .map_err(internal)?
            .count();
        Pulse::record("queued", "redis:default", None, None)
            .await
            .map_err(internal)?
            .count()
            .onlyBuckets();
        Pulse::set("system","acceptance",json!({"name":unsafe_text,"cpu":25,"memory_used":2048,"memory_total":8192,"storage":[{"directory":"/","used":100,"total":200}]}).to_string(),None).await.map_err(internal)?;
        Ok(Response::from("observed"))
    })
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(std::env::var("APP_BASE_PATH")?.into()).withConfig(|base,_|Ok(std::collections::BTreeMap::from([
        ("app".into(),json!({"env":"production","debug":false,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
        ("database".into(),json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}}})),
        ("cache".into(),json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache"),"lock_path":base.join("locks")}}})),
        ("session".into(),json!({"driver":"array"})),
        ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"pulse-test"}}})),
        ("pulse".into(),json!({"path":"operations/pulse","domain":"127.0.0.1","recorders":{"Servers":false,"SlowQueries":false},"ingest":{"driver":"storage","buffer":5000,"trim":{"lottery":[0,1]}}})),
    ]))).withProviders(vec![Box::new(openrust_pulse::PulseServiceProvider),Box::new(Access)]).withRouting(Routing::default().api(||{Route::get("/probe",probe)?;Ok(())})).create()?)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_application(create).await
}

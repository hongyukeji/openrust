//! Native HTTP acceptance host; never installed in the application skeleton.
use futures_util::{StreamExt as _, stream};
use openrust::{
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response},
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use serde_json::json;

#[path = "../../config/pulse.rs"]
mod published_pulse;
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
                .via_request("pulse-test", |request| {
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
            gate.define_guest("viewPulse", |user, _| {
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
        .await?
        .count()
        .max();
        for _ in 0..2 {
            Pulse::record(
                "slow_request",
                json!(["GET", "/frequent", "fixture::probe"]).to_string(),
                Some(600),
                None,
            )
            .await?
            .count()
            .max();
        }
        Pulse::record("user_request", "42", None, None)
            .await?
            .count();
        Pulse::record("slow_user_request", "99", None, None)
            .await?
            .count();
        Pulse::record("cache_hit", unsafe_text, None, None)
            .await?
            .count();
        for _ in 0..2 {
            Pulse::record("cache_miss", unsafe_text, None, None)
                .await?
                .count();
        }
        let queued_at = jiff::Timestamp::now().as_second() - 30;
        for _ in 0..3 {
            Pulse::record("queued", "redis:default", None, Some(queued_at))
                .await?
                .count()
                .only_buckets();
        }
        for (metric, value) in [("cpu", 25), ("memory", 2048)] {
            Pulse::record(metric, "acceptance", Some(value), None)
                .await?
                .avg()
                .only_buckets();
        }
        Pulse::record(
            "cpu",
            "acceptance",
            Some(13),
            Some(jiff::Timestamp::now().as_second() - 120),
        )
        .await?
        .avg()
        .only_buckets();
        Pulse::set("system","acceptance",json!({"name":unsafe_text,"cpu":25,"memory_used":2048,"memory_total":8192,"storage":[{"directory":"/","used":100,"total":200}]}).to_string(),None).await?;
        Pulse::set(
            "system",
            "stale",
            json!({"name":"ZZ Stale node","cpu":0,"memory_used":0,"memory_total":1024,"storage":[]}).to_string(),
            Some(jiff::Timestamp::now().as_second() - 60),
        )
        .await?;
        Ok(Response::from("observed"))
    })
}

fn interrupted_stream(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        openrust::support::facades::Auth::user(request).await?;
        Ok(Response::stream(
            openrust::http::StatusCode::OK,
            stream::once(async { Ok::<_, std::io::Error>("pulse-stream-started") })
                .chain(stream::pending()),
        ))
    })
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(std::env::var("APP_BASE_PATH")?.into()).with_config(|base,env| {
        let mut config = std::collections::BTreeMap::from([
        ("app".into(),json!({"env":"production","debug":false,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
        ("database".into(),json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}}})),
        ("cache".into(),json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache"),"lock_path":base.join("locks")}}})),
        ("session".into(),json!({"driver":"array"})),
        ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"pulse-test"}}})),
        ("pulse".into(),json!({"path":"operations/pulse","domain":"127.0.0.1","recorders":if std::env::var("PULSE_PROBE_TRANSPORT").as_deref() == Ok("1") {
            json!({"Servers":false,"SlowQueries":false,"UserRequests":{},"SlowRequests":{"threshold":{"#^/api/stream$#":100}}})
        } else {
            json!({"Servers":false,"SlowQueries":false,"CacheInteractions":{"enabled":false,"sample_rate":0.5},"Queues":{"enabled":false,"sample_rate":0.4}})
        },"ingest":{"driver":"storage","buffer":5000,"trim":{"lottery":[0,1]}}})),
    ]);
        match env.get("PULSE_PROBE_CONFIG") {
            Some("provider") => { config.remove("pulse"); }
            Some("published") => { config.insert("pulse".into(), published_pulse::load(env)?); }
            _ => {}
        }
        Ok(config)
    }).with_providers(vec![Box::new(openrust_pulse::PulseServiceProvider),Box::new(Access)]).with_routing(Routing::default().api(||{Route::get("/probe",probe)?;Route::get("/stream",interrupted_stream)?;Ok(())})).create()?)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_application(create).await
}

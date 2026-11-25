//! 敏感诊断的独立 HTTP 验收应用，不接入默认应用骨架。
use openrust::{
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Db, Mail, Route},
};
use serde_json::json;
#[path = "../../config/telescope.rs"]
mod telescope_config;
struct SecretMail;
impl openrust::mail::Mailable for SecretMail {
    fn envelope(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Envelope> {
        Ok(openrust::mail::mailables::Envelope {
            to: vec!["user@example.org".into()],
            subject: "PRIVATE-SUBJECT".into(),
            ..Default::default()
        })
    }
    fn content(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Content> {
        Ok(openrust::mail::mailables::Content {
            html: Some("<a href='https://example.org/reset/PRIVATE-MAIL'>Reset</a>".into()),
            ..Default::default()
        })
    }
}
fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Db::connection(None)
            .unwrap()
            .select("SELECT ? AS secret", &["PRIVATE-SQL".into()])
            .await?;
        Mail::send(&SecretMail).await?;
        if request.uri().path().ends_with("/fail") {
            return Err(HttpError::AuthorizationDenied("PRIVATE-ERROR".into()));
        }
        Ok(
            Response::json(StatusCode::OK, json!({"token":"PRIVATE-TOKEN"}))?.header(
                "set-cookie".parse().unwrap(),
                "session=PRIVATE-COOKIE".parse().unwrap(),
            ),
        )
    })
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(std::env::var("APP_BASE_PATH")?.into())
        .with_config(|base, env| Ok(std::collections::BTreeMap::from([
            ("app".into(),json!({"env":"testing","locale":"en","key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
            ("database".into(),json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}}})),
            ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
            ("mail".into(),json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"sender@example.org","name":"Sender"}})),
            ("telescope".into(), { let mut config=telescope_config::load(env)?; config["watchers"]["QueryWatcher"]["ignore_packages"]=false.into(); config }),
        ])))
        .with_providers(vec![Box::new(openrust_telescope::TelescopeServiceProvider)])
        .with_routing(Routing::default().api(|| {
            Route::group(openrust::routing::RouteGroup::new(), |router| {
                router.post("/reset/{token}", probe)?.middleware_name("sensitive"); Ok(())
            })?;
            Route::get("/ordinary", probe)
        }))
        .create()?)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_application(create).await
}

//! 敏感诊断的独立 HTTP 验收应用，不接入默认应用骨架。
use openrust::{
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Db, Mail, Route},
};
use serde_json::json;
#[path = "../../../../../database/migrations/0001_01_01_000002_create_jobs_table.rs"]
mod jobs;

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
    static MIGRATIONS: &[openrust::database::migrations::MigrationEntry] =
        &[openrust::database::migrations::MigrationEntry::new(
            "database/migrations/0001_01_01_000002_create_jobs_table.rs",
            &jobs::Definition,
        )];
    Ok(Application::configure(std::env::var("APP_BASE_PATH")?.into())
        .with_config(|base, env| {
            use openrust::config::ConfigError;
            let database = match env.get_utf8("USER_STARTER_DIAG_DATABASE")?.unwrap_or("sqlite") {
                "sqlite" => json!({"driver":"sqlite","database":base.join("database.sqlite")}),
                "mysql" if env.get_utf8("OPENRUST_REQUIRE_MYSQL")? == Some("1") => {
                    let url=env.get_utf8("OPENRUST_TEST_MYSQL_URL")?.ok_or(ConfigError::InvalidType { key:"OPENRUST_TEST_MYSQL_URL".into(), expected:"独立MySQL夹具URL" })?;
                    json!({"driver":"mysql","url":url})
                }
                _ => return Err(ConfigError::InvalidType { key:"USER_STARTER_DIAG_DATABASE".into(), expected:"sqlite或独立MySQL夹具" }),
            };
            let queue=env.get_utf8("USER_STARTER_DIAG_QUEUE")?.unwrap_or("database");
            let redis=env.get_utf8("USER_STARTER_DIAG_REDIS_URL")?;
            if !matches!(queue,"database"|"redis") || (queue=="redis" && redis.is_none()) {
                return Err(ConfigError::InvalidType { key:"USER_STARTER_DIAG_QUEUE".into(), expected:"database或显式指定独立URL的redis" });
            }
            let smtp=env.get_utf8("USER_STARTER_DIAG_SMTP_PORT")?.map(|port| port.parse::<u16>()).transpose().map_err(|_|ConfigError::InvalidType { key:"USER_STARTER_DIAG_SMTP_PORT".into(), expected:"SMTP夹具端口" })?;
            let mut telescope=telescope_config::load(env)?;
            telescope["storage"]["database"]["connection"]="diagnostics".into();
            telescope["watchers"]["QueryWatcher"]["ignore_packages"]=false.into();
            Ok(std::collections::BTreeMap::from([
                ("app".into(),json!({"env":"testing","locale":"en","key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
                ("database".into(),json!({"default":"diagnostics","connections":{"diagnostics":database},"redis":{"default":{"url":redis},"options":{"prefix":"user-starter-diagnostics:"}}})),
                ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
                ("queue".into(),json!({"default":queue,"connections":{"database":{"driver":"database","connection":"diagnostics","table":"jobs","queue":"user-starter-diagnostics","retry_after":60},"redis":{"driver":"redis","connection":"default","queue":"user-starter-diagnostics","retry_after":60}},"failed":{"driver":"database-uuids","database":"diagnostics","table":"failed_jobs"}})),
                ("mail".into(),json!({"default":if smtp.is_some(){"smtp"}else{"array"},"mailers":{"array":{"transport":"array"},"smtp":{"transport":"smtp","scheme":"smtp","host":"127.0.0.1","port":smtp}},"from":{"address":"sender@example.org","name":"Sender"}})),
                ("telescope".into(),telescope),
            ]))
        })
        .with_providers(vec![Box::new(openrust_telescope::TelescopeServiceProvider)])
        .with_migrations(MIGRATIONS)
        .with_commands(|artisan| artisan.add_command(Box::new(Stats(openrust::console::CommandDefinition::new(clap::Command::new("probe:diagnostic-stats"))))))
        .with_routing(Routing::default().api(|| {
            Route::group(openrust::routing::RouteGroup::new(), |router| {
                router.post("/reset/{token}", probe)?.middleware_name("sensitive"); Ok(())
            })?;
            Route::get("/queue/{kind}", enqueue)?;
            Route::get("/ordinary", probe)
        }))
        .create()?)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_application(create).await
}

struct QueueMail(&'static str);
impl openrust::mail::Mailable for QueueMail {
    fn envelope(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Envelope> {
        Ok(openrust::mail::mailables::Envelope {
            to: vec!["user@example.org".into()],
            subject: if self.0 == "control" {
                "CONTROL-SUBJECT"
            } else {
                "PRIVATE-SUBJECT"
            }
            .into(),
            ..Default::default()
        })
    }
    fn content(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Content> {
        Ok(if self.0 == "failure" {
            openrust::mail::mailables::Content {
                view: Some("PRIVATE-MISSING-VIEW".into()),
                ..Default::default()
            }
        } else {
            openrust::mail::mailables::Content {
                html: Some(
                    if self.0 == "control" {
                        "<p>CONTROL-MAIL</p>"
                    } else {
                        "<a href='https://example.org/reset/PRIVATE-MAIL'>Reset</a>"
                    }
                    .into(),
                ),
                ..Default::default()
            }
        })
    }
}
struct QueuedNotice(&'static str);
impl openrust::notifications::Notification for QueuedNotice {
    fn via(
        &self,
        _: &dyn openrust::notifications::Notifiable,
    ) -> openrust::notifications::NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }
    fn to_mail(
        &self,
        _: &dyn openrust::notifications::Notifiable,
    ) -> openrust::notifications::NotificationResult<
        Option<openrust::notifications::messages::MailDelivery>,
    > {
        Ok(Some(
            openrust::notifications::messages::MailDelivery::Mailable(Box::new(QueueMail(self.0))),
        ))
    }
    fn queue_options(
        &self,
        _: &dyn openrust::notifications::Notifiable,
        _: &str,
    ) -> Option<openrust::queue::DispatchOptions> {
        Some(Default::default())
    }
    fn should_encrypt(&self, _: &dyn openrust::notifications::Notifiable, _: &str) -> bool {
        self.0 != "control"
    }
}
fn enqueue(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = match request.route("kind") {
            Some("private") => "private",
            Some("failure") => "failure",
            Some("control") => "control",
            _ => return Err(HttpError::NotFound),
        };
        let recipient = openrust::notifications::AnonymousNotifiable::default().route(
            "mail",
            openrust::notifications::NotificationRoute::Mail(vec!["user@example.org".into()]),
        )?;
        openrust::support::facades::Notification::send(&[&recipient], &QueuedNotice(kind)).await?;
        Response::json(StatusCode::OK, json!({"queued":true}))
    })
}
struct Stats(openrust::console::CommandDefinition);
impl openrust::console::Command for Stats {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a openrust::console::Input,
        output: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            use openrust::{console::ConsoleError, database::Value};
            let read=async {
                let connection=Db::connection(None)?;
                let private=connection.table("telescope_entries").r#where("content","like","%PRIVATE-%").count("*").await?;
                let control=connection.table("telescope_entries").r#where("type","=","mail").r#where("content","like","%CONTROL-MAIL%").count("*").await?;
                let mail=connection.table("telescope_entries").r#where("type","=","mail").count("*").await?;
                let failed=connection.table("failed_jobs").count("*").await?;
                let rows=connection.table("jobs").get().await?;
                let mut payloads=Vec::new();
                for row in &rows {if let Some(Value::Text(payload))=row.get("payload") { payloads.push(serde_json::from_str::<serde_json::Value>(payload).map_err(|_|openrust::database::DatabaseError::InvalidQuery("验收作业JSON"))?); }}
                Ok::<_,openrust::database::DatabaseError>(json!({"private_entries":private,"control_mail":control,"mail_entries":mail,"failed_jobs":failed,"jobs":rows.len(),"payloads":payloads}))
            }.await.map_err(|error|ConsoleError::new(error.to_string()))?;
            let text = read.to_string();
            output
                .write_raw(&format!("{text}\n"))
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            Ok(0)
        })
    }
}

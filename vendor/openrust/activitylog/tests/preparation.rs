use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::http::Kernel as _,
    database::{Connection, DatabaseError, DatabaseManager},
    foundation::{Application, configuration::Routing},
    http::HttpFuture,
    support::facades::Artisan,
};
use openrust_activitylog::{Activity, ActivityRepository, ActivitylogServiceProvider, Redaction};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

fn entry() -> Activity {
    let mut entry = Activity::new("preparation.probe")
        .with_properties(json!({"context":{"api_key":"secret","safe":"kept"},"ignored":1}))
        .with_changes(
            json!({"name":"before","password":"old"}),
            json!({"name":"after","password":"new"}),
        );
    entry.created_at = Some(1_700_000_000_000);
    entry
}
async fn probe(connection: Connection) {
    let repository = ActivityRepository::new(connection.clone()).with_redaction(
        Redaction::new()
            .log_properties(&["context"])
            .log_only(&["name", "password"]),
    );
    let count = repository.query().get().await.unwrap().len();
    let prepared = repository.prepare(entry()).unwrap().unwrap();
    assert_eq!(prepared.record().created_at, 1_700_000_000_000);
    assert_eq!(
        prepared.record().properties,
        json!({"context":{"api_key":"[REDACTED]","safe":"kept"}})
    );
    assert_eq!(
        prepared.record().attribute_changes["attributes"]["password"],
        "[REDACTED]"
    );
    let saved = repository.log(entry()).await.unwrap().unwrap();
    assert_eq!(saved.properties, prepared.record().properties);
    assert_eq!(repository.query().get().await.unwrap().len(), count + 1);
    let result = connection
        .transaction(
            move |tx| {
                let repository = repository.clone();
                Box::pin(async move {
                    let invalid = Activity::new("bad").with_properties(json!([]));
                    assert!(matches!(
                        repository.log_in(tx, invalid).await,
                        Err(DatabaseError::InvalidConfiguration("Invalid activity"))
                    ));
                    repository.log_in(tx, entry()).await?;
                    repository
                        .prepare(entry())
                        .unwrap()
                        .unwrap()
                        .log_in(tx)
                        .await?;
                    Err::<(), _>(DatabaseError::InvalidQuery("probe rollback"))
                })
            },
            1,
        )
        .await;
    assert!(result.is_err());
    let plain = ActivityRepository::new(connection.clone());
    assert_eq!(plain.query().get().await.unwrap().len(), count + 1);
    assert_eq!(
        plain.prepare(entry()).unwrap().unwrap().record().properties["context"]["api_key"],
        "secret"
    );
    let disabled = plain.enabled(false);
    assert!(
        disabled
            .prepare(Activity::new("bad").with_properties(json!([])))
            .is_err()
    );
    assert!(disabled.log(entry()).await.unwrap().is_none());
    let limited = ActivityRepository::new(connection)
        .with_redaction(Redaction::new().log_properties(&["note"]));
    assert!(
        limited
            .prepare(Activity::new("large").with_properties(json!({"note":"x".repeat(17000)})))
            .is_err()
    );
    let mut invalid_time = entry();
    invalid_time.created_at = Some(i64::MAX);
    assert!(limited.prepare(invalid_time).is_err());
}
struct PrepareCommand(CommandDefinition);
impl Command for PrepareCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            probe(
                app.make::<DatabaseManager>()
                    .await
                    .unwrap()
                    .connection(None)
                    .unwrap(),
            )
            .await;
            Ok(0)
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(PrepareCommand(CommandDefinition::new(
        clap::Command::new("preparation:probe"),
    ))))
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/preparation", handler)
}
fn handler(
    _: &mut openrust::http::Request,
) -> HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        probe(openrust::database::Db::connection(None).unwrap()).await;
        Ok(openrust::http::Response::from("preparation passed"))
    })
}
fn application(root: &std::path::Path) -> Application {
    Application::configure(root.into())
        .with_config(|root, _| {
            let driver = std::env::var("ACTIVITYLOG_TEST_DRIVER").unwrap_or("sqlite".into());
            let connection = match driver.as_str() {
                "sqlite" => json!({"driver":"sqlite","database":root.join("probe.sqlite")}),
                "mysql" => {
                    assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
                    json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()})
                },
                "pgsql" => {
                    assert_eq!(std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(), Ok("1"));
                    json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()})
                },
                _ => panic!("unsupported isolated driver"),
            };
            Ok(BTreeMap::from([("database".into(), json!({"default":"testing","connections":{"testing":connection}}))]))
        })
        .with_providers(vec![Box::new(ActivitylogServiceProvider)])
        .with_routing(Routing::default().api(routes).commands(commands))
        .create().unwrap()
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn repository_preparation_keeps_policies_and_transactions_over_http_and_artisan() {
    use axum::handler::Handler as _;
    let root = tempfile::tempdir().unwrap();
    std::fs::File::create(root.path().join("probe.sqlite")).unwrap();
    assert_eq!(
        application(root.path())
            .handle_command(&["migrate".into(), "--force".into()])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        application(root.path())
            .handle_command(&["preparation:probe".into()])
            .await
            .unwrap(),
        0
    );
    let mut kernel = openrust::foundation::http::Kernel::new(application(root.path()));
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let response = openrust::http::client::Factory::new()
        .unwrap()
        .pending_request()
        .get(&format!("http://{address}/api/preparation"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.body().unwrap().as_ref(), b"preparation passed");
    server.abort();
    let _ = server.await;
    assert_eq!(
        application(root.path())
            .handle_command(&["migrate:rollback".into(), "--force".into()])
            .await
            .unwrap(),
        0
    );
}

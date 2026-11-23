use axum::{extract::State, handler::Handler as _};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::http::Kernel as _,
    database::Db,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    notifications::{
        DatabaseKey, DatabaseNotification, DatabaseRoute, Notifiable, Notification,
        NotificationResult, NotificationRoute,
        channels::{Channel, DatabaseChannel},
        messages::DatabaseMessage,
    },
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::net::TcpListener;

struct Recipient(DatabaseRoute);
impl Notifiable for Recipient {
    fn route_notification_for(
        &self,
        _: &str,
        _: &dyn Notification,
    ) -> NotificationResult<Option<NotificationRoute>> {
        Ok(Some(NotificationRoute::Database(self.0.clone())))
    }
}
struct Notice(String);
impl Notification for Notice {
    fn id(&self) -> Option<&str> {
        Some(&self.0)
    }
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["database".into()])
    }
    fn to_database(&self, _: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(Some(DatabaseMessage {
            data: json!({"saved":true}),
            kind: Some("store.probe".into()),
            read_at: None,
        }))
    }
}
async fn probe() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for numeric in [false, true] {
        // 两类主键使用独立连接与表前缀，避免同名表改列类型污染预编译参数类型。
        let name = if numeric {
            "numeric_records"
        } else {
            "records"
        };
        let connection = Db::connection(Some(name)).unwrap();
        connection
            .get_schema_builder()
            .create("notifications", |table| {
                DatabaseNotification::schema(table, |table| {
                    if numeric {
                        table.big_integer("notifiable_id");
                    } else {
                        table.string("notifiable_id", 100);
                    }
                });
            })
            .await
            .unwrap();
        let route = DatabaseRoute {
            connection: Some(name.into()),
            model_type: "Accounts".into(),
            key: if numeric {
                DatabaseKey::Integer(7)
            } else {
                DatabaseKey::String("member-7".into())
            },
        };
        let recipient = Recipient(route.clone());
        let notice = Notice(format!(
            "00000000-0000-4000-8000-{:012}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DatabaseChannel.send(&recipient, &notice).await.unwrap();
        let mut stored = route
            .find(&notice.0)
            .await
            .unwrap_or_else(|error| {
                let mut cause: &dyn std::error::Error = &error;
                loop {
                    eprintln!("通知查询错误: {cause}");
                    if let Some(next) = cause.source() {
                        cause = next;
                    } else {
                        break;
                    }
                }
                panic!("通知查询失败")
            })
            .unwrap();
        assert_eq!(stored.data, json!({"saved":true}));
        assert_eq!(stored.kind, "store.probe");
        let mut foreign = route.clone();
        foreign.model_type = "OtherAccounts".into();
        assert!(foreign.find(&notice.0).await.unwrap().is_none());
        foreign = route.clone();
        foreign.key = if numeric {
            DatabaseKey::Integer(8)
        } else {
            DatabaseKey::String("member-8".into())
        };
        assert!(foreign.find(&notice.0).await.unwrap().is_none());
        stored.mark_as_read().await.unwrap();
        let read_at = stored.read_at;
        stored.mark_as_read().await.unwrap();
        assert_eq!(stored.read_at, read_at);
        assert!(!route.find(&notice.0).await.unwrap().unwrap().unread());
        stored.mark_as_unread().await.unwrap();
        stored.mark_as_unread().await.unwrap();
        assert!(route.find(&notice.0).await.unwrap().unwrap().unread());
        let queued = Notice(format!(
            "00000000-0000-4000-8000-{:012}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let payload = DatabaseChannel
            .queue_payload(&recipient, &queued)
            .await
            .unwrap()
            .unwrap();
        DatabaseChannel.send_queued(payload).await.unwrap();
        assert!(route.find(&queued.0).await.unwrap().is_some());
        route
            .query()
            .unwrap()
            .r#where("id", "=", notice.0)
            .delete()
            .await
            .unwrap();
        stored.mark_as_read().await.unwrap();
        assert!(stored.unread());
        connection
            .get_schema_builder()
            .drop("notifications")
            .await
            .unwrap();
        assert!(stored.mark_as_read().await.is_err());
        assert!(stored.unread());
    }
}
fn endpoint(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        probe().await;
        Response::json(StatusCode::OK, json!({"verified":true}))
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/probe", endpoint)?;
    Ok(())
}
struct ProbeCommand(CommandDefinition);
impl Command for ProbeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async {
            probe().await;
            Ok(0)
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(ProbeCommand(CommandDefinition::new(
        clap::Command::new("notification:probe"),
    ))))
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn named_prefixed_notification_store_over_real_http_and_artisan() {
    let root = tempfile::tempdir().unwrap();
    let make_app = || {
        Application::configure(root.path().to_path_buf()).with_config(|_,_| {
        let driver=std::env::var("LAYERING_TEST_DRIVER").unwrap_or("sqlite".into());
        let mut database=match driver.as_str(){
            "sqlite"=>json!({"driver":"sqlite","database":":memory:"}),
            "mysql"=>json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}),
            "pgsql"=>json!({"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()}),
            _=>panic!("未知验收数据库"),
        };
        database["prefix"]=json!("layer_");
        let mut numeric_database = database.clone();
        numeric_database["prefix"] = json!("layer_numeric_");
        Ok([("database".into(),json!({"default":"unrelated","connections":{"unrelated":{"driver":"sqlite","database":":memory:"},"records":database,"numeric_records":numeric_database}}))].into())
    }).with_routing(Routing::default().api(routes).commands(commands)).create().unwrap()
    };
    let mut kernel = Kernel::new(make_app());
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let response = reqwest::get(format!("http://{address}/api/probe"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"verified":true})
    );
    server.abort();
    let _ = server.await;
    assert_eq!(
        make_app()
            .handle_command(&["notification:probe".into()])
            .await
            .unwrap(),
        0
    );
}

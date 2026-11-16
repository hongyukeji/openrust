use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use openrust::{
    container::Callback,
    contracts::{
        container::Container as _,
        events::{Event as PayloadEvent, Listener},
        foundation::Application as _,
    },
    database::{DB, DatabaseManager, Value},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    mail::{
        MailResult, Mailable,
        mailables::{Attachment, Content, Envelope, Headers},
    },
    notifications::{
        DatabaseKey, DatabaseNotification, DatabaseRoute, Notifiable, Notification as Notice,
        NotificationEvent, NotificationResult, NotificationRoute,
        messages::{DatabaseMessage, MailDelivery, MailMessage},
    },
    queue::{DispatchOptions, QueueManager},
    support::facades::{Event, Facade, Lang, Mail, Notification},
};
use serde_json::{Value as Json, json};

struct Receipt;
impl Mailable for Receipt {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            subject: "OpenRust receipt".into(),
            ..Default::default()
        })
    }
    fn content(&self) -> MailResult<Content> {
        Ok(Content {
            text: Some("plain receipt".into()),
            view: Some("receipt".into()),
            with: [("name".into(), json!("<Sam>"))].into_iter().collect(),
            ..Default::default()
        })
    }
    fn headers(&self) -> MailResult<Headers> {
        Ok(Headers {
            text: [("X-Receipt".into(), "R123".into())].into(),
            ..Default::default()
        })
    }
    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Ok(vec![Attachment {
            name: "receipt.txt".into(),
            mime: "text/plain".into(),
            bytes: b"file receipt".to_vec(),
        }])
    }
}

struct User {
    address: &'static str,
    route: DatabaseRoute,
}
impl Notifiable for User {
    fn route_notification_for(
        &self,
        channel: &str,
        _: &dyn Notice,
    ) -> NotificationResult<Option<NotificationRoute>> {
        Ok(match channel {
            "mail" => Some(NotificationRoute::Mail(vec![self.address.into()])),
            "database" => Some(NotificationRoute::Database(self.route.clone())),
            _ => None,
        })
    }
    fn preferred_locale(&self) -> Option<&str> {
        Some("fr")
    }
}

struct Payment {
    queued: bool,
}
impl Notice for Payment {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["mail".into(), "database".into()])
    }

    fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        Ok(Some(MailDelivery::Message(Box::new(MailMessage {
            envelope: Envelope {
                subject: format!("OpenRust payment ({})", Lang::getLocale().unwrap()),
                ..Default::default()
            },
            content: Content {
                text: Some("Payment received".into()),
                ..Default::default()
            },
            ..Default::default()
        }))))
    }

    fn to_array(&self, _: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(Some(DatabaseMessage::new(json!({"paid":true}))))
    }

    fn queue_options(&self, _: &dyn Notifiable, channel: &str) -> Option<DispatchOptions> {
        self.queued.then(|| DispatchOptions {
            queue: Some(channel.into()),
            after_commit: Some(true),
            ..Default::default()
        })
    }

    fn kind(&self) -> &str {
        "payment.received"
    }
}

type EventRecord = (String, String, String);

struct ListenerProbe {
    records: Arc<Mutex<Vec<EventRecord>>>,
    deny: bool,
}
impl Listener for ListenerProbe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let delivery = event.downcast_ref::<NotificationEvent>().unwrap();
            self.records.lock().unwrap().push((
                event.name().into(),
                delivery.channel.clone(),
                delivery.id.clone(),
            ));
            Ok(
                (self.deny && event.name() == "NotificationSending" && delivery.channel == "mail")
                    .then_some(ListenerResponse::Stop),
            )
        })
    }
}

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture(redis: Option<u16>, mysql: Option<&str>, smtp: Option<u16>) -> Root {
    let id = format!("{:016x}", fastrand::u64(..));
    let root = Root(std::env::temp_dir().join(format!("openrust-mail-{id}")));
    std::fs::create_dir_all(root.0.join("resources/views")).unwrap();
    std::fs::File::create(root.0.join("fixture.sqlite")).unwrap();
    std::fs::write(
        root.0.join("resources/views/receipt.blade.html"),
        "<p>Receipt for {{ $name }}</p>",
    )
    .unwrap();
    let db = mysql.map_or_else(
        || json!({"driver":"sqlite","database":"fixture.sqlite"}),
        |url| json!({"url":url}),
    );
    let config = json!({
        "app":{"locale":"en"},
        "mail":{"default":if smtp.is_some() {"smtp"} else {"array"},
            "from":{"address":"sender@example.test","name":"OpenRust"},
            "mailers":{"array":{"transport":"array"},
                "log":{"transport":"log"},
                "smtp":{"transport":"smtp","url":format!("smtp://127.0.0.1:{}",smtp.unwrap_or(1)), "timeout":2},
                "invalid":{"transport":"smtp","host":"127.0.0.1","url":"not a url secret-marker"}}},
        "database":{"default":"delivery","connections":{"delivery":db},
            "redis":{"options":{"prefix":format!("openrust-mail:{id}:")},"default":{"port":redis.unwrap_or(6379)}}},
        "cache":{"default":"array","stores":{"array":{"driver":"array"}}},
        "queue":{"default":if redis.is_some() {"redis"} else {"database"},
            "connections":{"redis":{"driver":"redis","retry_after":30}, "database":{"driver":"database","retry_after":30}},
            "failed":{"driver":"database-uuids","database":"delivery","table":"failed_jobs"}}
    });
    std::fs::write(root.0.join("config.json"), config.to_string()).unwrap();
    root
}

fn app(root: &Path) -> Application {
    Application::configure(root.into())
        .withConfig(|base, _| {
            let config: serde_json::Map<String, Json> =
                serde_json::from_slice(&std::fs::read(base.join("config.json")).unwrap()).unwrap();
            Ok(config.into_iter().collect())
        })
        .create()
        .unwrap()
}

#[tokio::test]
async fn mail_provider_adapters_and_composites_use_the_configured_transports() {
    use axum::{Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    type Requests = Arc<Mutex<Vec<(String, HeaderMap, String)>>>;
    async fn record(
        State(requests): State<Requests>,
        uri: http::Uri,
        headers: HeaderMap,
        body: String,
    ) -> axum::response::Response {
        let path = uri.path().to_owned();
        requests.lock().unwrap().push((path.clone(), headers, body));
        match path.as_str() {
            "/postmark" => ([("Content-Type", "application/json")], r#"{"ErrorCode":0,"MessageID":"postmark-id"}"#).into_response(),
            "/resend" => ([("Content-Type", "application/json")], r#"{"id":"resend-id"}"#).into_response(),
            "/" => ([("Content-Type", "text/xml")],
                r#"<SendRawEmailResponse xmlns="http://email.amazonaws.com/doc/2010-12-01/"><SendRawEmailResult><MessageId>ses-id</MessageId></SendRawEmailResult><ResponseMetadata><RequestId>request-1</RequestId></ResponseMetadata></SendRawEmailResponse>"#)
                .into_response(),
            _ => (http::StatusCode::UNAUTHORIZED, "fixture credential marker").into_response(),
        }
    }

    let requests = Requests::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/", post(record))
                .route("/{provider}", post(record))
                .with_state(requests.clone()),
        )
        .into_future(),
    );
    let root = fixture(None, None, None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["services"] = json!({"postmark":{"token":"test-postmark"},"resend":{"key":"test-resend"},
        "ses":{"region":"us-east-1","key":"test-key","secret":"test-secret","token":"test-session"}});
    config["mail"]["mailers"] = json!({
        "postmark":{"transport":"postmark","endpoint":format!("{base}/postmark"),"message_stream_id":"transactional"},
        "resend":{"transport":"resend","endpoint":format!("{base}/resend")},
        "ses":{"transport":"ses","endpoint":base,"options":{"ConfigurationSetName":"fixture","Tags":[{"Name":"kind","Value":"test"}]}},
        "reject":{"transport":"postmark","token":"fixture-override","endpoint":format!("{base}/reject")},
        "failover":{"transport":"failover","mailers":["reject","resend"],"retry_after":60},
        "roundrobin":{"transport":"roundrobin","mailers":["postmark","resend"]},
        "cycle":{"transport":"failover","mailers":["cycle"]},
    });
    std::fs::write(config_path, config.to_string()).unwrap();
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    application
        .call(
            Callback::new(&[], |_| async {
                struct NamedReceipt(&'static str);
                impl Mailable for NamedReceipt {
                    fn mailer(&self) -> Option<&str> {
                        Some(self.0)
                    }
                    fn envelope(&self) -> MailResult<Envelope> {
                        Receipt.envelope()
                    }
                    fn content(&self) -> MailResult<Content> {
                        Receipt.content()
                    }
                    fn headers(&self) -> MailResult<Headers> {
                        Receipt.headers()
                    }
                    fn attachments(&self) -> MailResult<Vec<Attachment>> {
                        Receipt.attachments()
                    }
                }
                let send = |name: &'static str| async move {
                    Mail::to("to@example.test")
                        .unwrap()
                        .bcc("blind@example.test")
                        .sendNow(&NamedReceipt(name))
                        .await
                };
                let ses = send("ses").await.unwrap();
                assert!(
                    String::from_utf8(ses.message.formatted())
                        .unwrap()
                        .contains("X-SES-Message-ID: ses-id")
                );
                for name in ["failover", "failover", "roundrobin", "roundrobin"] {
                    send(name).await.unwrap();
                }
                let rejected = send("reject").await.err().unwrap().to_string();
                assert!(rejected.contains("401") && !rejected.contains("marker"));
                assert!(
                    Mail::mailer(Some("cycle"))
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("Circular")
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 7);
    let paths: Vec<_> = requests.iter().map(|(path, _, _)| path.as_str()).collect();
    assert_eq!(&paths[..4], ["/", "/reject", "/resend", "/resend"]);
    assert!(matches!(
        &paths[4..6],
        ["/postmark", "/resend"] | ["/resend", "/postmark"]
    ));
    assert_eq!(paths[6], "/reject");
    let (_, headers, ses) = &requests[0];
    assert_eq!(headers["x-amz-security-token"], "test-session");
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .contains("Credential=test-key/")
    );
    let fields: std::collections::HashMap<_, _> = url::form_urlencoded::parse(ses.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(fields["Action"], "SendRawEmail");
    assert_eq!(fields["ConfigurationSetName"], "fixture");
    assert!(fields.values().any(|value| value == "blind@example.test"));
    let mime = String::from_utf8(STANDARD.decode(&fields["RawMessage.Data"]).unwrap()).unwrap();
    assert!(mime.contains("OpenRust receipt") && !mime.contains("blind@example.test"));
    let (_, headers, resend) = &requests[2];
    assert_eq!(headers["authorization"], "Bearer test-resend");
    let resend: Json = serde_json::from_str(resend).unwrap();
    assert_eq!(resend["bcc"], json!(["blind@example.test"]));
    assert_eq!(
        resend["attachments"][0]["content"],
        STANDARD.encode(b"file receipt")
    );
    let (_, headers, postmark) = requests
        .iter()
        .find(|(path, _, _)| path == "/postmark")
        .unwrap();
    assert_eq!(headers["x-postmark-server-token"], "test-postmark");
    let postmark: Json = serde_json::from_str(postmark).unwrap();
    assert_eq!(postmark["MessageStream"], "transactional");
    assert_eq!(postmark["Headers"][0]["Name"], "X-Receipt");
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn sendmail_uses_a_process_envelope_and_preserves_smtp_acceptance() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = fixture(None, None, None);
    let script = root.0.join("send mail");
    let transcript = root.0.join("transcript");
    std::fs::write(
        &script,
        r#"#!/bin/sh
transcript=$1
shift
if [ "$1" = "-bs" ]; then
    printf '220 local ESMTP\r\n'
    while IFS= read -r line; do
        printf '%s\n' "$line" >> "$transcript"
        case "$line" in
            EHLO*) printf '250-local\r\n250 OK\r\n' ;;
            'MAIL FROM:'*|'RCPT TO:'*) printf '250 OK\r\n' ;;
            DATA*) printf '354 Start mail input\r\n' ;;
            ".$(printf '\r')") printf '250 accepted\r\n' ;;
            QUIT*) exit 0 ;;
        esac
    done
else
    printf '%s\n' "$@" > "$transcript"
    cat >> "$transcript"
fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config_path = root.0.join("config.json");
    for mode in ["-t", "-bs"] {
        let mut config: Json =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        config["mail"]["default"] = json!("sendmail");
        config["mail"]["mailers"]["sendmail"] = json!({
            "transport":"sendmail",
            "path": format!("\"{}\" \"{}\" {mode}", script.display(), transcript.display())
        });
        std::fs::write(&config_path, config.to_string()).unwrap();
        let mut application = app(&root.0);
        application.boot().await.unwrap();
        application
            .call(
                Callback::new(&[], |_| async {
                    Mail::to("to@example.test")
                        .unwrap()
                        .bcc("blind@example.test")
                        .sendNow(&Receipt)
                        .await
                        .unwrap();
                    Ok(())
                }),
                Default::default(),
            )
            .await
            .unwrap();
        let delivered = std::fs::read_to_string(&transcript).unwrap();
        if mode == "-bs" {
            let (_, mime) = delivered.split_once("DATA\r\n").unwrap();
            assert!(delivered.contains("RCPT TO:<blind@example.test>"));
            assert!(!mime.contains("blind@example.test"));
        } else {
            let (envelope, mime) = delivered.split_once("Subject:").unwrap();
            assert!(envelope.contains("--\nto@example.test\nblind@example.test"));
            assert!(!mime.contains("blind@example.test"));
            std::fs::remove_file(&transcript).unwrap();
        }
        assert!(delivered.contains("OpenRust receipt"));
    }
}

#[tokio::test]
#[ignore = "Requires a dedicated SMTP server configured for this test"]
async fn smtp_delivery_fixture() {
    let url = std::env::var("OPENRUST_SMTP_FIXTURE_URL").expect("SMTP fixture URL");
    let rejection = std::env::var_os("OPENRUST_SMTP_FIXTURE_REJECT").is_some();
    let root = fixture(None, None, None);
    let path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["mail"]["default"] = json!("smtp");
    config["mail"]["mailers"]["smtp"] = json!({"transport":"smtp","url":url,"timeout":3});
    std::fs::write(&path, config.to_string()).unwrap();
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    application
        .call(
            Callback::new(&[], move |_| async move {
                let result = Mail::to("recipient@example.test")
                    .unwrap()
                    .sendNow(&Receipt)
                    .await;
                if rejection {
                    let error = result
                        .err()
                        .expect("SMTP delivery must be rejected")
                        .to_string();
                    assert!(
                        error.contains("SMTP connection or protocol failed"),
                        "{error}"
                    );
                    assert!(!error.contains("fixture-secret"));
                } else {
                    result.unwrap();
                }
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Spawned by the notification queue tests"]
async fn artisan_mail_fixture() {
    let root = PathBuf::from(std::env::var_os("OPENRUST_MAIL_TEST_ROOT").unwrap());
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("OPENRUST_MAIL_TEST_ARGS").unwrap()).unwrap();
    let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
    assert_eq!(app(&root).handleCommand(&args).await.unwrap(), 0);
}

async fn artisan(root: &Path, args: &[&str]) -> String {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "artisan_mail_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("OPENRUST_MAIL_TEST_ROOT", root)
            .env(
                "OPENRUST_MAIL_TEST_ARGS",
                serde_json::to_string(args).unwrap(),
            )
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn user() -> User {
    User {
        address: "notify@example.test",
        route: DatabaseRoute {
            connection: None,
            model_type: "users".into(),
            key: DatabaseKey::String("01-user".into()),
        },
    }
}

async fn setup(app: &Application) -> openrust::database::Connection {
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let mut schema = db.getSchemaBuilder();
    for table in ["notifications", "jobs", "failed_jobs"] {
        schema.dropIfExists(table).await.unwrap();
    }
    schema
        .create("notifications", |table| {
            DatabaseNotification::schema(table, |table| {
                table.string("notifiable_id", 100);
            })
        })
        .await
        .unwrap();
    schema
        .create("jobs", |table| {
            table.id();
            table.string("queue", 255).index();
            table.longText("payload");
            table.unsignedInteger("attempts");
            table.unsignedInteger("reserved_at").nullable();
            table.unsignedInteger("available_at");
            table.unsignedInteger("created_at");
        })
        .await
        .unwrap();
    schema
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", 255).unique();
            for column in ["connection", "queue", "payload", "exception"] {
                table.longText(column);
            }
            table.timestamp("failed_at", 0);
        })
        .await
        .unwrap();
    db
}

#[tokio::test]
async fn mail_composes_views_mime_headers_and_keeps_application_capture_isolated() {
    let root = fixture(None, None, None);
    let mut first = app(&root.0);
    let mut second = app(&root.0);
    first.boot().await.unwrap();
    second.boot().await.unwrap();
    first
        .call(
            Callback::new(&[], |_| async {
                let sent = Mail::to("to@example.test")
                    .unwrap()
                    .cc("cc@example.test")
                    .bcc("blind@example.test")
                    .sendNow(&Receipt)
                    .await
                    .unwrap();
                assert_eq!(sent.message.recipients().len(), 3);
                let mime = String::from_utf8(sent.message.formatted()).unwrap();
                for value in [
                    "OpenRust receipt",
                    "plain receipt",
                    "&lt;Sam&gt;",
                    "multipart/alternative",
                    "attachment",
                    "receipt.txt",
                    "X-Receipt: R123",
                ] {
                    assert!(mime.contains(value), "{value}: {mime}");
                }
                assert!(!mime.contains("blind@example.test"));
                assert_eq!(Mail::mailer(None).unwrap().sent().unwrap().len(), 1);
                assert!(Mail::mailer(Some("log")).is_ok());
                let error = Mail::mailer(Some("invalid")).err().unwrap().to_string();
                assert!(error.contains("Invalid SMTP URL") && !error.contains("secret-marker"));
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    second
        .call(
            Callback::new(&[], |_| async {
                assert!(Mail::mailer(None).unwrap().sent().unwrap().is_empty());
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn database_notification_is_polymorphic_localized_and_cancellable() {
    let root = fixture(None, None, None);
    let mut app = app(&root.0);
    app.boot().await.unwrap();
    setup(&app).await;
    app.call(
        Callback::new(&[], |_| async {
            let user = user();
            let records = Arc::new(Mutex::new(Vec::new()));
            let listener = Arc::new(ListenerProbe {
                records: records.clone(),
                deny: false,
            });
            for name in ["NotificationSending", "NotificationSent"] {
                Event::listen(name, listener.clone(), Delivery::Sync).unwrap();
            }
            Notification::send(&[&user], &Payment { queued: false })
                .await
                .unwrap();
            let id = {
                let records = records.lock().unwrap();
                assert_eq!(records.len(), 4);
                assert!(records.iter().all(|record| record.2 == records[0].2));
                records[0].2.clone()
            };
            let mut stored = user.route.find(&id).await.unwrap().unwrap();
            assert_eq!(stored.data, json!({"paid":true}));
            assert_eq!(stored.kind, "payment.received");
            assert!(stored.unread());
            stored.markAsRead().await.unwrap();
            assert!(!user.route.find(&id).await.unwrap().unwrap().unread());
            stored.markAsUnread().await.unwrap();
            assert!(user.route.find(&id).await.unwrap().unwrap().unread());
            let sent = Mail::mailer(None).unwrap().sent().unwrap();
            assert!(
                String::from_utf8_lossy(&sent[0].message.formatted())
                    .contains("OpenRust payment (fr)")
            );
            assert_eq!(Lang::getLocale().unwrap(), "en");
            Event::listen(
                "NotificationSending",
                Arc::new(ListenerProbe {
                    records,
                    deny: true,
                }),
                Delivery::Sync,
            )
            .unwrap();
            Notification::sendNow(&[&user], &Payment { queued: true }, Some(&["mail"]))
                .await
                .unwrap();
            assert_eq!(Mail::mailer(None).unwrap().sent().unwrap().len(), 1);
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
}

async fn queued(redis: Option<u16>, mysql: Option<&str>, smtp: Option<u16>) {
    let root = fixture(redis, mysql, smtp);
    let mut app = app(&root.0);
    app.boot().await.unwrap();
    let db = setup(&app).await;
    let manager = app.container().make::<QueueManager>().unwrap();
    let queue = manager.connection(None).unwrap();
    app.call(
        Callback::new(&[], |_| async {
            let user = user();
            let notifications = Notification::getFacadeRoot().unwrap();
            let mut transaction = DB::beginTransaction().await.unwrap();
            notifications
                .sendInTransaction(&[&user], &Payment { queued: true }, Some(&mut transaction))
                .await
                .unwrap();
            transaction.rollBack(None).await.unwrap();
            let mut transaction = DB::beginTransaction().await.unwrap();
            notifications
                .sendInTransaction(&[&user], &Payment { queued: true }, Some(&mut transaction))
                .await
                .unwrap();
            Mail::to("receipt@example.test")
                .unwrap()
                .queue(
                    &Receipt,
                    DispatchOptions {
                        queue: Some("mail".into()),
                        after_commit: Some(true),
                        ..Default::default()
                    },
                    Some(&mut transaction),
                )
                .await
                .unwrap();
            transaction.commit().await.unwrap();
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(queue.size(Some("mail")).await.unwrap(), 2);
    assert_eq!(queue.size(Some("database")).await.unwrap(), 1);
    let connection = if redis.is_some() { "redis" } else { "database" };
    artisan(
        &root.0,
        &[
            "queue:work",
            connection,
            "--queue=mail,database",
            "--stop-when-empty",
            "--sleep=0",
        ],
    )
    .await;
    assert_eq!(queue.size(Some("mail")).await.unwrap(), 0);
    assert_eq!(queue.size(Some("database")).await.unwrap(), 0);
    assert_eq!(db.table("notifications").count("*").await.unwrap(), 1);
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    assert_eq!(
        db.table("notifications")
            .value("notifiable_id")
            .await
            .unwrap(),
        Value::Text("01-user".into())
    );
    if mysql.is_some() {
        for table in ["notifications", "jobs", "failed_jobs"] {
            db.getSchemaBuilder().dropIfExists(table).await.unwrap();
        }
    }
}

#[tokio::test]
async fn artisan_worker_delivers_mail_and_notification_after_commit() {
    queued(None, None, None).await;
}

#[tokio::test]
async fn artisan_worker_uses_real_smtp_mysql_and_redis() {
    let redis = std::env::var("OPENRUST_TEST_REDIS_PORT")
        .ok()
        .map(|value| value.parse().unwrap());
    let mysql = std::env::var("OPENRUST_TEST_DELIVERY_MYSQL_URL").ok();
    let smtp = std::env::var("OPENRUST_TEST_SMTP_PORT")
        .ok()
        .map(|value| value.parse().unwrap());
    if std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_some() {
        assert!(redis.is_some() && mysql.is_some() && smtp.is_some());
    }
    if let (Some(redis), Some(mysql), Some(smtp)) = (redis, mysql, smtp) {
        queued(Some(redis), Some(&mysql), Some(smtp)).await;
    }
}

#[tokio::test]
async fn artisan_worker_records_a_failed_smtp_delivery_after_retry() {
    struct FailingReceipt;

    impl Mailable for FailingReceipt {
        fn envelope(&self) -> MailResult<Envelope> {
            Receipt.envelope()
        }
        fn content(&self) -> MailResult<Content> {
            Receipt.content()
        }
        fn mailer(&self) -> Option<&str> {
            Some("smtp")
        }
    }

    let root = fixture(None, None, None);
    let mut app = app(&root.0);
    app.boot().await.unwrap();
    let db = setup(&app).await;
    app.call(
        Callback::new(&[], |_| async {
            let error = Mail::to("failure@example.test")
                .unwrap()
                .sendNow(&FailingReceipt)
                .await
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains("SMTP connection or protocol failed."),
                "{error}"
            );
            assert!(!error.contains("failure@example.test"));
            Mail::to("failure@example.test")
                .unwrap()
                .queue(&FailingReceipt, DispatchOptions::default(), None)
                .await
                .unwrap();
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    artisan(&root.0, &["queue:work", "database", "--once", "--tries=2"]).await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Integer(1)
    );
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--stop-when-empty",
            "--sleep=0",
            "--tries=2",
        ],
    )
    .await;
    assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
    let failure = db.table("failed_jobs").value("exception").await.unwrap();
    let Value::Text(failure) = failure else {
        panic!("missing failed job");
    };
    assert_eq!(failure, "queue job execution failed");
    assert!(!failure.contains("failure@example.test"));
}

#[tokio::test]
async fn artisan_generates_the_notifications_table_migration() {
    let root = fixture(None, None, None);
    let output = artisan(&root.0, &["make:notifications-table"]).await;
    assert!(output.contains("created successfully"));
    let directory = root.0.join("database/migrations");
    let files = std::fs::read_dir(directory)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let code = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(code.contains("DatabaseNotification::schema") && code.contains("notifiable_id"));
}

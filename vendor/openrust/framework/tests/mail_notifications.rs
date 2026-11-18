use std::{
    ffi::OsString,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use openrust::{
    broadcasting::{BroadcastChannel, PrivateChannel},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    container::Callback,
    contracts::{
        config::Repository,
        container::Container as _,
        events::Dispatcher as _,
        events::{Event as PayloadEvent, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{DB, DatabaseManager, Value},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
        http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response},
    mail::{
        MailFuture, MailResult, Mailable, Message, SentMessage,
        events::{MessageSending, MessageSent},
        mailables::{Attachment, Content, Envelope, Headers},
        transport::Transport,
    },
    notifications::{
        ChannelManager, DatabaseKey, DatabaseNotification, DatabaseRoute, Notifiable,
        Notification as Notice, NotificationError, NotificationEvent, NotificationFuture,
        NotificationQueueGuard, NotificationResult, NotificationRoute, NotificationTarget,
        SendQueuedNotifications,
        messages::{BroadcastMessage, DatabaseMessage, MailDelivery, MailMessage},
    },
    queue::{DispatchOptions, JobOptions, Payload, QueueManager, middleware::Skip},
    support::{
        Date, ProviderResult, ServiceProvider,
        facades::{Artisan, Config, Event, Facade, Lang, Mail, Notification, Route},
    },
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

struct MarkdownReceipt {
    plain_view: bool,
}

impl Mailable for MarkdownReceipt {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            subject: "Markdown receipt fixture".into(),
            ..Default::default()
        })
    }

    fn content(&self) -> MailResult<Content> {
        Ok(Content {
            markdown: Some("markdown-receipt".into()),
            text_view: self.plain_view.then(|| "plain-receipt".into()),
            with: [
                ("name".into(), json!("<Sam>")),
                (
                    "portal".into(),
                    json!("https://example.test/review?step=1&code=2"),
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        })
    }
}

struct NamedReceipt<'a>(&'a str);
impl Mailable for NamedReceipt<'_> {
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

    fn should_send(&self, _: &dyn Notifiable, channel: &str) -> bool {
        channel != "blocked"
    }

    fn fake_data(&self) -> Json {
        json!({"paid":true})
    }

    fn kind(&self) -> &str {
        "payment.received"
    }
}

struct RoutedNotice;

impl Notice for RoutedNotice {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["mail".into(), "database".into(), "broadcast".into()])
    }

    fn to_mail(&self, recipient: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        Payment { queued: false }.to_mail(recipient)
    }

    fn to_broadcast(&self, _: &dyn Notifiable) -> NotificationResult<Option<BroadcastMessage>> {
        let mut message =
            BroadcastMessage::new([("payment".into(), json!(42))].into_iter().collect());
        message.connection = Some("database".into());
        message.queue = Some("broadcast-notifications".into());
        Ok(Some(message))
    }

    fn broadcast_on(&self) -> Vec<BroadcastChannel> {
        vec![BroadcastChannel::Private(PrivateChannel {
            name: "notice-fallback".into(),
        })]
    }

    fn broadcast_as(&self) -> Option<&str> {
        Some("payment.notification")
    }

    fn broadcast_type(&self) -> &str {
        "payment.accepted"
    }
}

fn notify_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let target = Notification::route(
            "mail",
            NotificationRoute::Mail(vec!["ondemand@example.test".into()]),
        )
        .unwrap()
        .route(
            "broadcast",
            NotificationRoute::Broadcast(vec![BroadcastChannel::Private(PrivateChannel {
                name: "payment.42".into(),
            })]),
        )
        .unwrap();
        target
            .notifyNow(&RoutedNotice, None)
            .await
            .map_err(|error| HttpError::Internal {
                source: Box::new(error),
            })?;
        Response::json(http::StatusCode::OK, json!({"accepted":true}))
    })
}

struct GuardedNotice {
    flag: PathBuf,
    id: &'static str,
}

impl Notice for GuardedNotice {
    fn id(&self) -> Option<&str> {
        Some(self.id)
    }

    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["database".into()])
    }

    fn to_array(&self, _: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(Some(DatabaseMessage::new(json!({"paid":true}))))
    }

    fn queue_options(&self, _: &dyn Notifiable, _: &str) -> Option<DispatchOptions> {
        Some(DispatchOptions {
            queue: Some("guarded".into()),
            ..Default::default()
        })
    }

    fn should_send(&self, _: &dyn Notifiable, _: &str) -> bool {
        self.flag.is_file()
    }

    fn queue_middleware(&self, _: &dyn Notifiable, _: &str) -> Option<&str> {
        (self.id == SKIP_NOTICE).then_some("skip")
    }

    fn queue_job_options(&self, _: &dyn Notifiable, _: &str) -> JobOptions {
        JobOptions {
            max_tries: Some(2),
            ..Default::default()
        }
    }

    fn queue_guard(
        &self,
        _: &dyn Notifiable,
        _: &str,
    ) -> NotificationResult<Option<NotificationQueueGuard>> {
        Ok(Some(NotificationQueueGuard {
            name: "fixture.delivery-flag".into(),
            data: json!(self.flag),
        }))
    }
}

const ALLOW_NOTICE: &str = "e6ed5cc9-ce3a-45cc-9010-c2f77a23f289";
const DENY_NOTICE: &str = "16ee6ea2-59b9-424f-9cab-2ea7bab6026a";
const SKIP_NOTICE: &str = "a308ce32-a693-4367-ab13-bd99cc4179b0";

fn guarded_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let root = PathBuf::from(Config::string("fixture.guard_path").unwrap());
        for (file, id) in [
            ("allow", ALLOW_NOTICE),
            ("deny", DENY_NOTICE),
            ("allow", SKIP_NOTICE),
        ] {
            user()
                .notify(&GuardedNotice {
                    flag: root.join(file),
                    id,
                })
                .await
                .map_err(|error| HttpError::Internal {
                    source: Box::new(error),
                })?;
        }
        Response::json(http::StatusCode::ACCEPTED, json!({"queued":true}))
    })
}

fn guard_file(data: &Json) -> NotificationFuture<'_, NotificationResult<bool>> {
    Box::pin(async move {
        let flag = data.as_str().expect("serialized flag path");
        assert_eq!(Lang::getLocale().unwrap(), "fr");
        tokio::fs::try_exists(flag)
            .await
            .map_err(|source| NotificationError::Delivery {
                source: Box::new(source),
            })
    })
}

struct FixtureGuardProvider;

impl ServiceProvider for FixtureGuardProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<ChannelManager>()?
                .extendQueueGuard("fixture.delivery-flag", guard_file);
            let queue = app.container().make::<QueueManager>()?;
            SendQueuedNotifications::registerMiddleware(
                &queue,
                "skip",
                vec![Arc::new(Skip::when(true))],
            )?;
            Ok(())
        })
    }
}

struct FixtureMailTransport(PathBuf);

fn record_mail(path: &Path, stage: &str, message: &Message) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "{stage} {}", message.recipients()[0]).unwrap();
}

impl Transport for FixtureMailTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            record_mail(&self.0, "transport", message);
            Ok(SentMessage {
                message: message.clone(),
            })
        })
    }
}

struct MailLifecycleProbe(PathBuf);

impl Listener for MailLifecycleProbe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(sending) = event.downcast_ref::<MessageSending>() {
                assert_eq!(sending.mailer, "fixture");
                record_mail(&self.0, "sending", &sending.message);
                let blocked = sending.message.recipients()[0] == "blocked@example.test";
                return Ok(blocked.then_some(ListenerResponse::Value(Box::new(false))));
            }
            let received = event.downcast_ref::<MessageSent>().unwrap();
            assert_eq!(received.mailer, "fixture");
            record_mail(&self.0, "sent", &received.sent.message);
            Ok(None)
        })
    }
}

struct FixtureMailProvider;

impl ServiceProvider for FixtureMailProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let configuration = container.make::<dyn Repository>()?;
            let Some(log) = configuration.get("fixture.mail_events_path") else {
                return Ok(());
            };
            let path = PathBuf::from(log.as_str().unwrap());
            let dispatcher = container.make::<openrust::events::Dispatcher>()?;
            let listener = Arc::new(MailLifecycleProbe(path));
            dispatcher.listen("MessageSending", listener.clone(), Delivery::Sync)?;
            dispatcher.listen("MessageSent", listener, Delivery::Sync)?;
            container
                .make::<openrust::mail::MailManager>()?
                .extend("fixture", |config| {
                    Ok(Arc::new(FixtureMailTransport(PathBuf::from(
                        config["path"].as_str().unwrap(),
                    ))))
                });
            Ok(())
        })
    }
}

fn mail_events_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let send = |recipient: &'static str| async move {
            Mail::to(recipient)
                .unwrap()
                .send(&NamedReceipt("fixture"))
                .await
                .map_err(|error| HttpError::Internal {
                    source: Box::new(error),
                })
        };
        let delivered = send("http@example.test").await?.is_some();
        let blocked = send("blocked@example.test").await?.is_none();
        let forced = Mail::to("blocked@example.test")
            .unwrap()
            .sendNow(&NamedReceipt("fixture"))
            .await;
        let forced_blocked = matches!(forced, Err(openrust::mail::MailError::Cancelled));
        Response::json(
            http::StatusCode::OK,
            json!({"delivered":delivered,"blocked":blocked,"forced_blocked":forced_blocked}),
        )
    })
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

struct SendFixture(CommandDefinition);

impl Command for SendFixture {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let name = input.matches().get_one::<String>("mailer").unwrap();
            let named = NamedReceipt(name);
            let markdown = MarkdownReceipt {
                plain_view: input.matches().get_flag("plain-view"),
            };
            let mailable: &dyn Mailable = if input.matches().get_flag("markdown") {
                &markdown
            } else {
                &named
            };
            let sent = Mail::to("to@example.test")
                .unwrap()
                .bcc("blind@example.test")
                .sendNow(mailable)
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let mime = String::from_utf8(sent.message.formatted()).unwrap();
            output.comment(
                mime.lines()
                    .find(|line| line.starts_with("X-Message-ID:"))
                    .unwrap_or("accepted"),
            );
            Ok(0)
        })
    }
}

struct AssertFakes(CommandDefinition);

impl Command for AssertFakes {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let cached_mailer = Mail::mailer(None).unwrap();
            let mail = Mail::fake().unwrap();
            let notifications = Notification::fake().unwrap();
            let kind = Receipt.kind();
            mail.assertNothingOutgoing();
            notifications.assertNothingSent();

            let named = Mail::mailer(Some("unconfigured-provider")).unwrap();
            let queued = Mail::to("queue@example.test")
                .unwrap()
                .locale("fr")
                .queue(
                    &Receipt,
                    DispatchOptions {
                        queue: Some("mail".into()),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .unwrap();
            assert!(queued.is_none());
            Mail::to("sent@example.test")
                .unwrap()
                .sendNow(&Receipt)
                .await
                .unwrap();
            named.sendNow(&AddressedReceipt).await.unwrap();
            cached_mailer.sendNow(&AddressedReceipt).await.unwrap();
            mail.assertSentOnce(kind)
                .assertQueuedOnce(kind)
                .assertOutgoingCount(4)
                .assertNotOutgoing("unused")
                .assertSentMatching(kind, |entry| {
                    entry.envelope.to[0].address == "sent@example.test"
                })
                .assertQueuedMatching(kind, |entry| {
                    entry.locale.as_deref() == Some("fr")
                        && entry.queue.as_ref().unwrap().queue.as_deref() == Some("mail")
                });
            assert!(cached_mailer.sent().unwrap().is_empty());

            let queued_notice = Payment { queued: true };
            let model = user();
            let target = NotificationTarget::from(model.route.clone());
            model
                .notifyNow(&queued_notice, Some(&["mail", "blocked"]))
                .await
                .unwrap();
            Notification::route(
                "mail",
                NotificationRoute::Mail(vec!["ondemand@example.test".into()]),
            )
            .unwrap()
            .notify(&queued_notice)
            .await
            .unwrap();
            notifications
                .assertSentToOnce(&target, queued_notice.kind())
                .assertSentToMatching(&target, queued_notice.kind(), |entry| {
                    entry.channels == ["mail"]
                        && entry.locale.as_deref() == Some("fr")
                        && entry.data == json!({"paid":true})
                })
                .assertSentOnDemandOnce(queued_notice.kind())
                .assertSentOnDemandMatching(queued_notice.kind(), |entry| {
                    entry.channels == ["mail"]
                        && matches!(entry.target, NotificationTarget::OnDemand(_))
                })
                .assertSentTimes(queued_notice.kind(), 2)
                .assertCount(2);
            mail.assertOutgoingCount(4);
            output.comment("mail and notification fakes verified");
            Ok(0)
        })
    }
}

struct AddressedReceipt;

impl Mailable for AddressedReceipt {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            to: vec!["cached@example.test".into()],
            ..Receipt.envelope()?
        })
    }

    fn content(&self) -> MailResult<Content> {
        Receipt.content()
    }
}

fn mail_routes() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(AssertFakes(CommandDefinition::new(
        clap::Command::new("fixture:assert-fakes"),
    ))))?;
    Artisan::add(Box::new(SendFixture(CommandDefinition::new(
        clap::Command::new("fixture:send-mail")
            .arg(clap::Arg::new("mailer").required(true))
            .arg(
                clap::Arg::new("markdown")
                    .long("markdown")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("plain-view")
                    .long("plain-view")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))
}

fn app_builder(root: &Path) -> ApplicationBuilder {
    Application::configure(root.into())
        .withConfig(|base, _| {
            let config: serde_json::Map<String, Json> =
                serde_json::from_slice(&std::fs::read(base.join("config.json")).unwrap()).unwrap();
            Ok(config.into_iter().collect())
        })
        .withRouting(Routing::default().commands(mail_routes).web(|| {
            Route::get("/notify", notify_from_http)?;
            Route::get("/guarded-notify", guarded_from_http)?;
            Route::get("/mail-events", mail_events_from_http)
        }))
        .withProviders(vec![
            Box::new(FixtureGuardProvider),
            Box::new(FixtureMailProvider),
        ])
}

fn app(root: &Path) -> Application {
    app_builder(root).create().unwrap()
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

#[tokio::test]
async fn artisan_delivers_through_ses_v2_mailgun_and_cloudflare() {
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
        let reply = match path.as_str() {
            "/v2/email/outbound-emails" => json!({"MessageId":"ses-v2-id"}),
            "/v3/mg.example.test/messages" | "/v3/mg.example.test/messages.mime" => {
                json!({"id":"mailgun-id","message":"Queued"})
            }
            "/cloudflare" => json!({"success":true, "result":{"delivered":["to@example.test"]}}),
            _ => return http::StatusCode::NOT_FOUND.into_response(),
        };
        ([("Content-Type", "application/json")], reply.to_string()).into_response()
    }

    let requests = Requests::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .fallback(post(record))
                .with_state(requests.clone()),
        )
        .into_future(),
    );
    let root = fixture(None, None, None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["services"] = json!({
        "ses":{"region":"us-east-1","key":"test-key","secret":"test-secret","token":"test-session"},
        "mailgun":{"secret":"test-mailgun","domain":"mg.example.test","endpoint":base},
        "cloudflare":{"token":"test-cloudflare","account_id":"fixture-account"},
    });
    config["mail"]["mailers"] = json!({
        "ses-v2":{"transport":"ses-v2","endpoint":base,"options":{
            "ConfigurationSetName":"ses-config", "EmailTags":[{"Name":"kind","Value":"receipt"}],
            "ListManagementOptions":{"ContactListName":"fixture-list"},"TenantName":"fixture-tenant"}},
        "mailgun":{"transport":"mailgun"},
        "mailgun-api":{"transport":"mailgun","scheme":"api","secret":"test-mailgun","domain":"mg.example.test","endpoint":base},
        "cloudflare":{"transport":"cloudflare","endpoint":format!("{base}/cloudflare")},
    });
    std::fs::write(config_path, config.to_string()).unwrap();
    for (name, receipt) in [
        ("ses-v2", "ses-v2-id"),
        ("mailgun", "mailgun-id"),
        ("mailgun-api", "mailgun-id"),
        ("cloudflare", "accepted"),
    ] {
        assert!(
            artisan(&root.0, &["fixture:send-mail", name])
                .await
                .contains(receipt)
        );
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let (path, headers, body) = &requests[0];
    assert_eq!(path, "/v2/email/outbound-emails");
    assert_eq!(headers["x-amz-security-token"], "test-session");
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .contains("Credential=test-key/")
    );
    let ses: Json = serde_json::from_str(body).unwrap();
    assert_eq!(
        ses["Destination"]["ToAddresses"],
        json!(["to@example.test", "blind@example.test"])
    );
    assert_eq!(ses["ConfigurationSetName"], "ses-config");
    assert_eq!(
        ses["ListManagementOptions"]["ContactListName"],
        "fixture-list"
    );
    assert_eq!(ses["EmailTags"][0]["Name"], "kind");
    assert_eq!(ses["TenantName"], "fixture-tenant");
    let mime = String::from_utf8(
        STANDARD
            .decode(ses["Content"]["Raw"]["Data"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(mime.contains("OpenRust receipt") && !mime.contains("blind@example.test"));
    let (path, headers, raw) = &requests[1];
    assert_eq!(path, "/v3/mg.example.test/messages.mime");
    assert_eq!(
        headers["authorization"],
        format!("Basic {}", STANDARD.encode("api:test-mailgun"))
    );
    assert!(
        raw.contains("filename=\"message.mime\"")
            && raw.contains("to@example.test,blind@example.test")
    );
    assert!(
        !raw.split_once("Subject:")
            .unwrap()
            .1
            .contains("blind@example.test")
    );
    let (path, _, body) = &requests[2];
    assert_eq!(path, "/v3/mg.example.test/messages");
    assert!(body.contains("filename=\"receipt.txt\"") && body.contains("name=\"bcc\""));
    let (path, headers, body) = &requests[3];
    assert_eq!(path, "/cloudflare");
    assert_eq!(headers["authorization"], "Bearer test-cloudflare");
    let cloudflare: Json = serde_json::from_str(body).unwrap();
    assert_eq!(cloudflare["from"]["address"], "sender@example.test");
    assert_eq!(cloudflare["bcc"], json!(["blind@example.test"]));
    assert_eq!(
        cloudflare["attachments"][0]["content"],
        STANDARD.encode(b"file receipt")
    );
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
async fn artisan_sends_themed_markdown_mail_through_smtp() {
    let ports = std::env::var("OPENRUST_TEST_SMTP_PORT")
        .ok()
        .zip(std::env::var("OPENRUST_TEST_SMTP_HTTP_PORT").ok());
    let Some((smtp, http)) = ports else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let root = fixture(None, None, Some(smtp.parse().unwrap()));
    let path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let theme_path = root.0.join("resources/views/vendor/mail");
    let publish = ["vendor:publish", "--tag=laravel-mail"];
    artisan(&root.0, &publish).await;
    let html_layout = theme_path.join("html/layout.blade.html");
    let text_layout = theme_path.join("text/layout.blade.html");
    let default_theme = theme_path.join("html/themes/default.css");
    let published_panel = theme_path.join("html/panel.blade.html");
    let published_footer = theme_path.join("html/footer.blade.html");
    let stock = std::fs::read_to_string(&html_layout).unwrap();
    for component in [
        "message", "button", "panel", "table", "header", "footer", "subcopy",
    ] {
        for format in ["html", "text"] {
            assert!(
                theme_path
                    .join(format)
                    .join(format!("{component}.blade.html"))
                    .is_file()
            );
        }
    }
    assert!(default_theme.is_file() && text_layout.is_file());
    std::fs::write(
        &published_panel,
        "<aside>Published panel {!! $slot !!}</aside>",
    )
    .unwrap();
    std::fs::write(
        &published_footer,
        "<footer data-custom=\"yes\">{!! $slot !!}</footer>",
    )
    .unwrap();
    let override_html = "<html><body><header>Published layout</header>{!! $slot !!}</body></html>";
    std::fs::write(&html_layout, override_html).unwrap();
    std::fs::write(&text_layout, "Published text\n{!! $slot !!}").unwrap();
    artisan(&root.0, &publish).await;
    assert_eq!(
        std::fs::read_to_string(&html_layout).unwrap(),
        override_html
    );
    std::fs::write(
        theme_path.join("html/themes/receipt.css"),
        "h1 { color: #126789; }",
    )
    .unwrap();
    config["mail"]["markdown"] = json!({"theme":"receipt"});
    config["app"]["name"] = json!("Mail Components");
    config["app"]["url"] = json!("https://origin.example.test");
    config["app"]["timezone"] = json!("America/Los_Angeles");
    config["app"]["locale"] = json!("fr");
    std::fs::create_dir_all(root.0.join("lang")).unwrap();
    std::fs::write(
        root.0.join("lang/fr.json"),
        json!({"All rights reserved.": "Tous droits réservés."}).to_string(),
    )
    .unwrap();
    std::fs::write(&path, config.to_string()).unwrap();
    std::fs::write(
        root.0.join("resources/views/markdown-receipt.blade.html"),
        "<x-mail::message>
# Receipt for {{ $name }}

[Open portal](https://example.test)

[Unsafe](javascript:alert%281%29)

<x-mail::button :url=\"$portal\" color=\"success\" align=\"right\">Review order</x-mail::button>

<x-mail::button url=\"javascript:alert(1)\">Untrusted action</x-mail::button>

<x-mail::panel>**Please read** the receipt.</x-mail::panel>

<x-mail::header url=\"https://help.example.test\">Support team</x-mail::header>

<x-mail::footer>**Regional** office</x-mail::footer>

<x-mail::subcopy>**Standalone** note</x-mail::subcopy>

<x-mail::table>
| Item | Price |
| --- | --- |
| Subscription | $42 |
</x-mail::table>

<script>alert(1)</script>

<x-slot:subcopy>Keep this receipt.</x-slot:subcopy>
</x-mail::message>",
    )
    .unwrap();
    assert!(
        artisan(&root.0, &["fixture:send-mail", "smtp", "--markdown"])
            .await
            .contains("accepted")
    );

    let url = format!("http://127.0.0.1:{http}/api/v1");
    let index: Json = reqwest::get(format!("{url}/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let summary = index["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["Subject"] == "Markdown receipt fixture")
        .unwrap();
    let id = summary["ID"].as_str().unwrap();
    let receipt: Json = reqwest::get(format!("{url}/message/{id}"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let html = receipt["HTML"].as_str().unwrap();
    let text = receipt["Text"].as_str().unwrap();
    for expected in [
        "<h1",
        "#126789",
        "&lt;Sam&gt;",
        "<table",
        "Subscription",
        "Published layout",
        "Mail Components",
        "https://origin.example.test",
        "button-success",
        "step=1&amp;code=2",
        "Published panel",
        "<strong>Please read</strong>",
        "Keep this receipt.",
        "© 2029 Mail Components. Tous droits réservés.",
        "https://help.example.test",
        "<strong>Regional</strong> office",
        "<strong>Standalone</strong> note",
    ] {
        assert!(html.contains(expected), "{html}");
    }
    assert!(!html.contains("<script>") && !html.contains("javascript:"));
    assert!(!html.contains("openrust-mail-"));
    assert_eq!(html.matches("data-custom=\"yes\"").count(), 2);
    assert!(text.starts_with("Published text"), "{text}");
    assert!(
        text.contains("Receipt for <Sam>") && text.contains("Open portal (https://example.test)"),
        "{text}"
    );
    assert!(
        text.contains("Review order: https://example.test/review?step=1&code=2"),
        "{text}"
    );
    assert!(
        text.contains("Keep this receipt.") && text.contains("Please read"),
        "{text}"
    );
    assert!(!text.contains("Published panel") && !text.contains("button-success"));
    assert!(!text.contains("Subscription"));
    for expected in [
        "Mail Components: https://origin.example.test",
        "Support team: https://help.example.test",
        "Regional office",
        "Standalone note",
        "© 2029 Mail Components. Tous droits réservés.",
    ] {
        assert!(text.contains(expected), "{text}");
    }

    let custom = root.0.join("custom-mail/html");
    std::fs::create_dir_all(custom.join("themes")).unwrap();
    std::fs::write(
        custom.join("layout.blade.html"),
        "<html><body><header>Configured layout</header>{!! $slot !!}</body></html>",
    )
    .unwrap();
    std::fs::write(custom.join("themes/default.css"), "h1 { color: #654321; }").unwrap();
    config["mail"]["markdown"] = json!({"paths":["custom-mail"]});
    config["app"]["locale"] = json!("en");
    std::fs::write(&path, config.to_string()).unwrap();
    std::fs::write(&text_layout, [0xff]).unwrap();
    std::fs::write(
        root.0.join("resources/views/plain-receipt.blade.html"),
        "Explicit plain text",
    )
    .unwrap();
    artisan(
        &root.0,
        &["fixture:send-mail", "smtp", "--markdown", "--plain-view"],
    )
    .await;

    let next: Json = reqwest::get(format!("{url}/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let new_id = next["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["Subject"] == "Markdown receipt fixture" && message["ID"] != id)
        .unwrap()["ID"]
        .as_str()
        .unwrap();
    let configured: Json = reqwest::get(format!("{url}/message/{new_id}"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let html = configured["HTML"].as_str().unwrap();
    assert!(
        html.contains("Configured layout") && html.contains("#654321"),
        "{html}"
    );
    assert!(
        html.contains("© 2029 Mail Components. All rights reserved."),
        "{html}"
    );
    assert!(
        configured["Text"]
            .as_str()
            .unwrap()
            .starts_with("Explicit plain text")
    );

    artisan(
        &root.0,
        &["vendor:publish", "--tag=laravel-mail", "--force"],
    )
    .await;
    assert_eq!(std::fs::read_to_string(&html_layout).unwrap(), stock);
    assert!(
        !std::fs::read_to_string(&published_panel)
            .unwrap()
            .contains("Published panel")
    );
    assert!(
        !std::fs::read_to_string(&text_layout)
            .unwrap()
            .contains("Published text")
    );
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
    let markdown = args.iter().any(|arg| arg == "--markdown");
    let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
    if markdown {
        let mut fixture = openrust::testing::TestApp::create(|_| app_builder(&root)).unwrap();
        fixture.travelTo(Date::fromUtc(
            "2030-01-01T01:00:00Z".parse().unwrap(),
            chrono_tz::UTC,
        ));
        let result = fixture.artisan(args).await;
        result.assertSuccessful();
        print!("{}", result.stdout);
    } else {
        assert_eq!(app(&root).handleCommand(&args).await.unwrap(), 0);
    }
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

async fn drop_delivery_tables(db: &openrust::database::Connection) {
    for table in ["notifications", "jobs", "failed_jobs"] {
        db.getSchemaBuilder().dropIfExists(table).await.unwrap();
    }
}

fn delivery_mysql() -> Option<String> {
    std::env::var("OPENRUST_TEST_DELIVERY_MYSQL_URL").ok()
}

async fn setup(app: &Application) -> openrust::database::Connection {
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    drop_delivery_tables(&db).await;
    let mut schema = db.getSchemaBuilder();
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
    let mysql = delivery_mysql();
    let root = fixture(None, mysql.as_deref(), None);
    let mut app = app(&root.0);
    app.boot().await.unwrap();
    let db = setup(&app).await;
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
    if mysql.is_some() {
        drop_delivery_tables(&db).await;
    }
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
        drop_delivery_tables(&db).await;
    }
}

async fn serve_fixture_http(
    kernel: Kernel,
    route: &str,
) -> (String, tokio::task::JoinHandle<Result<(), std::io::Error>>) {
    use axum::{Router, extract::State};

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        let (parts, body) = request.into_parts();
        let (parts, body) = kernel
            .handle(&mut Request::from_parts(parts, body))
            .await
            .into_parts();
        axum::response::Response::from_parts(parts, body)
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/{}", listener.local_addr().unwrap(), route);
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .fallback(transport)
                .with_state(Arc::new(kernel)),
        )
        .into_future(),
    );
    (url, server)
}

#[tokio::test]
async fn http_on_demand_notification_broadcasts_from_an_artisan_worker() {
    let Ok(mysql) = std::env::var("OPENRUST_TEST_DELIVERY_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let root = fixture(None, Some(&mysql), None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    let log_path = root.0.join("notification.log");
    config["broadcasting"] = json!({"default":"log"});
    config["logging"] = json!({"default":"single","channels":{"single":{"driver":"single","path":log_path,"level":"info"}}});
    std::fs::write(config_path, config.to_string()).unwrap();

    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let application = kernel.get_application();
    let db = setup(application).await;
    let queue = application
        .container()
        .make::<QueueManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let mailer = application
        .container()
        .make::<openrust::mail::MailManager>()
        .unwrap()
        .mailer(None)
        .unwrap();

    let (url, server) = serve_fixture_http(kernel, "notify").await;
    let response = reqwest::get(url).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Json>().await.unwrap(),
        json!({"accepted":true})
    );
    let sent = mailer.sent().unwrap();
    assert_eq!(sent[0].message.recipients(), ["ondemand@example.test"]);
    assert_eq!(db.table("notifications").count("*").await.unwrap(), 0);
    assert_eq!(
        queue.size(Some("broadcast-notifications")).await.unwrap(),
        1
    );

    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=broadcast-notifications",
            "--stop-when-empty",
            "--sleep=0",
        ],
    )
    .await;
    assert_eq!(
        queue.size(Some("broadcast-notifications")).await.unwrap(),
        0
    );
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    let log = std::fs::read_to_string(log_path).unwrap();
    for expected in [
        "payment.notification",
        "private-payment.42",
        "payment.accepted",
        "\"id\"",
    ] {
        assert!(log.contains(expected), "{log}");
    }
    assert!(!log.contains("notice-fallback"));

    drop_delivery_tables(&db).await;
    server.abort();
}

#[tokio::test]
async fn mail_and_notification_fakes_are_application_local_over_http_and_artisan() {
    let root = fixture(None, None, None);
    let output = artisan(&root.0, &["fixture:assert-fakes"]).await;
    assert!(output.contains("mail and notification fakes verified"));

    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let container = kernel.get_application().container();
    let real_mailer = container
        .make::<openrust::mail::MailManager>()
        .unwrap()
        .mailer(None)
        .unwrap();
    let mail = container
        .make::<openrust::mail::MailManager>()
        .unwrap()
        .fake();
    let notifications = container
        .make::<openrust::notifications::ChannelManager>()
        .unwrap()
        .fake();
    let (url, server) = serve_fixture_http(kernel, "notify").await;
    let response = reqwest::get(url).await.unwrap();
    assert_eq!(response.status(), 200);
    notifications
        .assertSentOnDemandOnce(RoutedNotice.kind())
        .assertSentOnDemandMatching(RoutedNotice.kind(), |entry| {
            entry.channels == ["mail", "broadcast"]
        });
    mail.assertNothingOutgoing();
    assert!(real_mailer.sent().unwrap().is_empty());

    let mut independent = app(&root.0);
    independent.boot().await.unwrap();
    independent
        .call(
            Callback::new(&[], |_| async move {
                Mail::to("real@example.test")
                    .unwrap()
                    .sendNow(&Receipt)
                    .await
                    .unwrap();
                assert_eq!(Mail::mailer(None).unwrap().sent().unwrap().len(), 1);
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    mail.assertNothingOutgoing();
    server.abort();
}

#[tokio::test]
async fn artisan_checks_live_notification_guard_after_http_dispatch() {
    let Some(mysql) = delivery_mysql() else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let root = fixture(None, Some(&mysql), None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["fixture"] = json!({"guard_path": root.0});
    std::fs::write(config_path, config.to_string()).unwrap();
    let allow = root.0.join("allow");
    let deny = root.0.join("deny");
    std::fs::write(&deny, b"enabled").unwrap();

    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let application = kernel.get_application();
    let db = setup(application).await;
    let queue = application
        .container()
        .make::<QueueManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let (url, server) = serve_fixture_http(kernel, "guarded-notify").await;
    assert_eq!(reqwest::get(url).await.unwrap().status(), 202);
    assert_eq!(queue.size(Some("guarded")).await.unwrap(), 3);
    let Value::Text(payload) = db.table("jobs").value("payload").await.unwrap() else {
        panic!("expected serialized job payload");
    };
    let payload: Payload = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload.options().max_tries, Some(2));
    assert_eq!(db.table("notifications").count("*").await.unwrap(), 0);

    std::fs::remove_file(&deny).unwrap();
    std::fs::write(&allow, b"enabled").unwrap();
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=guarded",
            "--stop-when-empty",
            "--sleep=0",
        ],
    )
    .await;
    assert_eq!(queue.size(Some("guarded")).await.unwrap(), 0);
    assert_eq!(db.table("notifications").count("*").await.unwrap(), 1);
    assert_eq!(
        db.table("notifications").value("id").await.unwrap(),
        Value::Text(ALLOW_NOTICE.into())
    );
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);

    drop_delivery_tables(&db).await;
    server.abort();
}

#[tokio::test]
async fn http_and_artisan_use_custom_mailers_and_dispatch_delivery_events() {
    let root = fixture(None, None, None);
    let config_path = root.0.join("config.json");
    let trace_path = root.0.join("trace.log");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["mail"]["mailers"]["fixture"] = json!({"transport":"fixture","path":trace_path});
    config["fixture"] = json!({"mail_events_path":trace_path});
    std::fs::write(config_path, config.to_string()).unwrap();
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "mail-events").await;
    let response = reqwest::get(url).await.unwrap().error_for_status().unwrap();
    assert_eq!(
        response.json::<Json>().await.unwrap(),
        json!({"delivered":true,"blocked":true,"forced_blocked":true})
    );

    let output = artisan(&root.0, &["fixture:send-mail", "fixture"]).await;
    assert!(output.contains("accepted"));
    let trace = std::fs::read_to_string(trace_path).unwrap();
    assert_eq!(
        trace.lines().collect::<Vec<_>>(),
        [
            "sending http@example.test",
            "transport http@example.test",
            "sent http@example.test",
            "sending blocked@example.test",
            "sending blocked@example.test",
            "sending to@example.test",
            "transport to@example.test",
            "sent to@example.test",
        ]
    );
    server.abort();
}

#[tokio::test]
async fn artisan_worker_delivers_mail_and_notification_after_commit() {
    let mysql = delivery_mysql();
    queued(None, mysql.as_deref(), None).await;
}

#[tokio::test]
async fn artisan_worker_uses_real_smtp_mysql_and_redis() {
    let redis = std::env::var("OPENRUST_TEST_REDIS_PORT")
        .ok()
        .map(|value| value.parse().unwrap());
    let mysql = delivery_mysql();
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

    let mysql = delivery_mysql();
    let root = fixture(None, mysql.as_deref(), None);
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
        if mysql.is_some() {
            Value::Unsigned(1)
        } else {
            Value::Integer(1)
        }
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
    if mysql.is_some() {
        drop_delivery_tables(&db).await;
    }
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

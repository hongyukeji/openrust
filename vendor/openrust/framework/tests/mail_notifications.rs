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
        mail::Attachable,
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
        MailFuture, MailResult, Mailable, Message, MessageCustomization, SentMessage,
        events::{MessageSending, MessageSent},
        mailables::{
            Address, Attachment, AttachmentSource, Content, Envelope, Headers, InlineAttachment,
        },
        transport::Transport,
    },
    notifications::{
        ChannelManager, DatabaseKey, DatabaseNotification, DatabaseRoute, Notifiable,
        Notification as Notice, NotificationError, NotificationEvent, NotificationFuture,
        NotificationQueueGuard, NotificationResult, NotificationRoute, NotificationTarget,
        SendQueuedNotifications,
        messages::{BroadcastMessage, DatabaseMessage, MailDelivery, MailMessage, VonageMessage},
        slack::{EventMetadata, SlackMessage, SlackRoute},
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
            tags: vec!["receipt".into()],
            metadata: [("order-id".into(), "R123".into())].into(),
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

struct AttachmentReceipt {
    path: PathBuf,
    subject: &'static str,
}

struct RemoteReport(String);

impl Attachable for RemoteReport {
    #[allow(
        non_snake_case,
        reason = "Implements the Laravel-compatible attachment contract"
    )]
    fn toMailAttachment(&self) -> MailResult<AttachmentSource> {
        Attachment::fromUrl(&self.0)
    }
}

impl Mailable for AttachmentReceipt {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            subject: self.subject.into(),
            tags: vec!["receipt".into(), "shipment".into()],
            using: vec![
                MessageCustomization::new("fixture.marker", json!({"value":"tracking-123"})),
                MessageCustomization::new("fixture.append", Json::Null),
            ],
            ..Receipt.envelope()?
        })
    }

    fn content(&self) -> MailResult<Content> {
        Receipt.content()
    }

    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Receipt.attachments()
    }

    fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
        Ok(vec![
            Attachment::fromPath(&self.path),
            Attachment::fromStorage("exports/default.csv"),
            Attachment::fromStorageDisk("archive", "data.bin")
                .r#as("archive-record.bin")
                .withMime("application/x-openrust"),
            Attachment::fromCloudStorage("nested/object.bin")?,
            Attachment::fromData(vec![0, 255, 1], "generated.bin"),
            RemoteReport(Config::string("fixture.attachment_url").unwrap()).toMailAttachment()?,
        ])
    }
}

struct UploadedReceipt(AttachmentSource);

impl Mailable for UploadedReceipt {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            subject: "Uploaded attachment HTTP".into(),
            ..Default::default()
        })
    }

    fn content(&self) -> MailResult<Content> {
        Receipt.content()
    }

    fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
        Ok(vec![self.0.clone()])
    }
}

fn uploaded_mail_from_http(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let queued = request.uri().query() == Some("queued=1");
        let file = request.file("upload").await?.expect("fixture upload");
        let receipt = UploadedReceipt(Attachment::fromUploadedFile(&file));
        let mail = Mail::to("to@example.test").unwrap();
        let result = if queued {
            mail.queue(
                &receipt,
                DispatchOptions {
                    connection: Some("redis".into()),
                    queue: Some("attachment-sources".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .map(|_| ())
        } else {
            mail.sendNow(&receipt).await.map(|_| ())
        };
        result.map_err(|error| HttpError::Internal {
            source: Box::new(error),
        })?;
        Response::json(http::StatusCode::OK, json!({"accepted":true}))
    })
}

fn attachment_mail_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let file = Config::string("fixture.attachment_file").unwrap();
        Mail::to("to@example.test")
            .unwrap()
            .sendNow(&AttachmentReceipt {
                path: file.into(),
                subject: "Source attachments HTTP",
            })
            .await
            .map_err(|error| HttpError::Internal {
                source: Box::new(error),
            })?;
        Response::json(http::StatusCode::OK, json!({"accepted":true}))
    })
}

struct MarkdownReceipt {
    plain_view: bool,
    options_fixture: bool,
    theme: Option<String>,
    inline: bool,
}

fn inline_logo() -> InlineAttachment {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    InlineAttachment::new("logo@images.example.test", Attachment {
        name: "logo.png".into(),
        mime: "image/png".into(),
        bytes: STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aWioAAAAASUVORK5CYII=").unwrap(),
    })
}

impl Mailable for MarkdownReceipt {
    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        if self.inline {
            Receipt.attachments()
        } else {
            Ok(Vec::new())
        }
    }

    fn inline_attachments(&self) -> MailResult<Vec<InlineAttachment>> {
        Ok(if self.inline {
            vec![inline_logo()]
        } else {
            vec![]
        })
    }

    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            subject: if self.options_fixture {
                "Markdown options fixture"
            } else {
                "Markdown receipt fixture"
            }
            .into(),
            ..Default::default()
        })
    }

    fn content(&self) -> MailResult<Content> {
        Ok(Content {
            markdown: Some("markdown-receipt".into()),
            theme: self.theme.clone(),
            text_view: self.plain_view.then(|| "plain-receipt".into()),
            with: [
                ("name".into(), json!("<Sam>")),
                (
                    "markdown_text".into(),
                    json!("[Literal](https://example.test/local) *marks*"),
                ),
                ("blocked_url".into(), json!("javascript:alert(1)")),
                (
                    "portal".into(),
                    json!("https://example.test/review?step=1&code=2"),
                ),
                ("logo".into(), json!("cid:logo@images.example.test")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        })
    }
}

struct NamedReceipt<'a>(&'a str, bool);
impl Mailable for NamedReceipt<'_> {
    fn mailer(&self) -> Option<&str> {
        Some(self.0)
    }
    fn envelope(&self) -> MailResult<Envelope> {
        let mut envelope = Receipt.envelope()?;
        if self.1 {
            if self.0 == "postmark" {
                envelope.using.extend([
                    MessageCustomization::new("fixture.marker", json!({"value":"api-456"})),
                    MessageCustomization::new("fixture.append", Json::Null),
                ]);
            } else {
                envelope.tags.push("shipment".into());
            }
        }
        Ok(envelope)
    }
    fn content(&self) -> MailResult<Content> {
        let mut content = Receipt.content()?;
        if self.1 {
            content.view = None;
            content.html = Some(format!(
                "<img src=\"{}\" alt=\"Receipt logo\">",
                inline_logo().cid()
            ));
        }
        Ok(content)
    }
    fn headers(&self) -> MailResult<Headers> {
        Receipt.headers()
    }
    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Receipt.attachments()
    }
    fn inline_attachments(&self) -> MailResult<Vec<InlineAttachment>> {
        Ok(self.1.then(inline_logo).into_iter().collect())
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
        Ok(Some(MailDelivery::Message(Box::new(
            MailMessage::new()
                .subject(format!("OpenRust payment ({})", Lang::getLocale().unwrap()))
                .line("Payment received")
                .action("View payment", "https://example.test/payments/1")
                .line("Thank you for your payment"),
        ))))
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
            if let Some(path) = configuration.get("fixture.mail_customizer_marker") {
                use openrust::mail::{MailError, MailManager};
                let fail = |message: &'static str| MailError::Composition {
                    source: message.into(),
                };
                let marker = PathBuf::from(path.as_str().unwrap());
                let manager = container.make::<MailManager>()?;
                manager.registerMessageCustomizer("fixture.marker", move |draft, data| {
                    if !marker.is_file() {
                        return Err(fail("Mail customizer marker is missing."));
                    }
                    let value = data["value"]
                        .as_str()
                        .ok_or_else(|| fail("Marker value is required."))?;
                    draft
                        .headers
                        .text
                        .insert("X-Custom-Marker".into(), value.into());
                    Ok(())
                })?;
                manager.registerMessageCustomizer("fixture.append", move |draft, _| {
                    if !draft.headers.text.contains_key("X-Custom-Marker") {
                        return Err(fail("Mail customizers ran out of order."));
                    }
                    let text = draft
                        .text
                        .as_mut()
                        .ok_or_else(|| fail("Rendered text is required."))?;
                    text.push_str(" - customized body");
                    draft.attachment_sources.push(
                        Attachment::fromData(b"customizer bytes".as_slice(), "customized.txt")
                            .withMime("text/plain"),
                    );
                    Ok(())
                })?;
            }
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
                .send(&NamedReceipt("fixture", false))
                .await
                .map_err(|error| HttpError::Internal {
                    source: Box::new(error),
                })
        };
        let delivered = send("http@example.test").await?.is_some();
        let blocked = send("blocked@example.test").await?.is_none();
        let forced = Mail::to("blocked@example.test")
            .unwrap()
            .sendNow(&NamedReceipt("fixture", false))
            .await;
        let forced_blocked = matches!(forced, Err(openrust::mail::MailError::Cancelled));
        Response::json(
            http::StatusCode::OK,
            json!({"delivered":delivered,"blocked":blocked,"forced_blocked":forced_blocked}),
        )
    })
}

struct AssertionMail(Content);

impl Mailable for AssertionMail {
    fn envelope(&self) -> MailResult<Envelope> {
        use openrust::mail::mailables::Address;
        Ok(Envelope {
            subject: "Receipt for Sam".into(),
            from: Some(Address {
                address: "sales@example.test".into(),
                name: Some("Sales".into()),
            }),
            to: vec!["sam@example.test".into()],
            cc: vec!["copy@example.test".into()],
            bcc: vec!["audit@example.test".into()],
            reply_to: vec!["support@example.test".into()],
            using: vec![MessageCustomization::new(
                "unregistered.fixture",
                Json::Null,
            )],
            ..Default::default()
        })
    }

    fn content(&self) -> MailResult<Content> {
        let mut content = self.0.clone();
        content
            .with
            .insert("locale".into(), json!(Lang::getLocale().unwrap()));
        Ok(content)
    }

    fn locale(&self) -> Option<&str> {
        Some("fr")
    }

    fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
        panic!("message assertions must not read attachment sources")
    }
}

fn mail_assertions_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use futures_util::FutureExt as _;
        use std::panic::AssertUnwindSafe;

        let mail = AssertionMail(Content {
            view: Some("assertion-html".into()),
            text_view: Some("assertion-text".into()),
            with: [("name".into(), json!("A & <Sam>"))].into_iter().collect(),
            ..Default::default()
        });
        mail.assertFrom("sales@example.test", Some("Sales"))
            .assertTo("sam@example.test", None)
            .assertHasTo("sam@example.test", None)
            .assertHasCc("copy@example.test", None)
            .assertHasBcc("audit@example.test", None)
            .assertHasReplyTo("support@example.test", None)
            .assertHasSubject("Receipt for Sam")
            .assertSeeInHtml("A & <Sam>", true)
            .await
            .assertSeeInHtml("<span>", false)
            .await
            .assertDontSeeInHtml("private-note", false)
            .await
            .assertSeeInOrderInHtml(&["fr", "A & <Sam>", "café", "final"], true)
            .await
            .assertSeeInText("Plain fr")
            .await
            .assertDontSeeInText("<span>")
            .await
            .assertSeeInOrderInText(&["Plain", "fr", "receipt"])
            .await;
        assert!(!mail.hasFrom("sales@example.test", Some("Support")).unwrap());
        assert!(!mail.hasTo("other@example.test", None).unwrap());
        assert!(!mail.hasSubject("Different subject").unwrap());
        for assertion in [
            mail.assertSeeInHtml("A & <Sam>", false),
            mail.assertDontSeeInText("Plain fr"),
            mail.assertSeeInOrderInHtml(&["final", "fr"], false),
            mail.assertSeeInOrderInText(&["receipt", "receipt"]),
        ] {
            assert!(AssertUnwindSafe(assertion).catch_unwind().await.is_err());
        }
        AssertionMail(Content {
            text: Some("Only text".into()),
            ..Default::default()
        })
        .assertSeeInHtml("Only text", false)
        .await;
        AssertionMail(Content {
            markdown: Some("assertion-markdown".into()),
            ..Default::default()
        })
        .assertSeeInText("Markdown proof")
        .await;
        assert_eq!(Lang::getLocale().unwrap(), "en");
        assert!(Mail::mailer(None).unwrap().sent().unwrap().is_empty());
        Response::json(http::StatusCode::OK, json!({"asserted":true}))
    })
}

struct DeliveryNotice(Option<&'static str>);

impl Notice for DeliveryNotice {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }

    fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        let message = MailMessage::new()
            .mailer("smtp")
            .subject(format!(
                "Notification envelope {}",
                self.0.unwrap_or("http")
            ))
            .priority(match self.0 {
                None => -1,
                Some("redis") => 8,
                _ => 3,
            })
            .from(Address {
                address: "finance@example.test".into(),
                name: Some("Finance Team".into()),
            })
            .replyTo("support@example.test")
            .cc("team@example.test")
            .bcc("audit@example.test")
            .line("Notice body")
            .attachMany([
                Attachment::fromPath(Config::string("fixture.notice_path").unwrap())
                    .r#as("notice.txt")
                    .withMime("text/plain"),
            ])
            .attachData(b"memory bytes".as_slice(), "memory.txt", Some("text/plain"))
            .attachFromStorage("report.txt", Some("default-report.txt"), Some("text/plain"))
            .attachFromStorageDisk(
                "archive",
                "report.txt",
                Some("archive-report.txt"),
                Some("text/plain"),
            )
            .tag("payment")
            .metadata("case", "notification")
            .withMessage(MessageCustomization::new(
                "fixture.marker",
                json!({"value":"notification-marked"}),
            ))
            .withMessage(MessageCustomization::new("fixture.append", Json::Null));
        message
            .assertHasBcc("audit@example.test", None)
            .assertHasAttachedData(b"memory bytes", "memory.txt", Some("text/plain"));
        Ok(Some(MailDelivery::Message(Box::new(message))))
    }

    fn queue_options(&self, _: &dyn Notifiable, _: &str) -> Option<DispatchOptions> {
        self.0.map(|connection| DispatchOptions {
            connection: Some(connection.into()),
            queue: Some("notification-envelope".into()),
            ..Default::default()
        })
    }
}

fn notification_envelope_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Notification::route(
            "mail",
            NotificationRoute::Mail(vec![Address {
                address: "reader@example.test".into(),
                name: Some("Notice Reader".into()),
            }]),
        )
        .unwrap()
        .notifyNow(&DeliveryNotice(None), None)
        .await
        .map_err(|error| HttpError::Internal {
            source: Box::new(error),
        })?;
        Response::json(http::StatusCode::OK, json!({"sent":true}))
    })
}

struct SmsNotice {
    mode: String,
    connection: Option<&'static str>,
}

impl Notice for SmsNotice {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["vonage".into()])
    }

    fn to_vonage(&self, _: &dyn Notifiable) -> NotificationResult<Option<VonageMessage>> {
        let message = if self.mode == "unicode" {
            VonageMessage::new()
                .content("  已付款  ")
                .unicode()
                .from("Invoices")
                .clientReference("invoice-42")
                .statusCallback("https://example.test/sms-status")
                .usingClient("partner")
        } else {
            VonageMessage::new().content(&self.mode)
        };
        Ok(Some(message))
    }

    fn queue_options(&self, _: &dyn Notifiable, _: &str) -> Option<DispatchOptions> {
        self.connection.map(|connection| DispatchOptions {
            connection: Some(connection.into()),
            queue: Some("vonage-test".into()),
            ..Default::default()
        })
    }
}

fn vonage_from_http(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let mode = request.route("mode").unwrap().to_owned();
    Box::pin(async move {
        let notice = SmsNotice {
            mode,
            connection: None,
        };
        let result = if notice.mode == "missing-route" {
            Notification::sendNow(
                &[&openrust::notifications::AnonymousNotifiable::default()],
                &notice,
                None,
            )
            .await
        } else {
            let number = if notice.mode == "invalid" {
                "free-form"
            } else {
                "+447700900000"
            };
            Notification::route("vonage", NotificationRoute::Vonage(number.into()))
                .unwrap()
                .notifyNow(&notice, None)
                .await
        };
        result.map_err(|error| HttpError::Internal {
            source: Box::new(error),
        })?;
        Response::json(http::StatusCode::OK, json!({"processed":true}))
    })
}

struct SlackNotice {
    mode: String,
    connection: Option<&'static str>,
}

impl Notice for SlackNotice {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["slack".into()])
    }

    fn to_slack(&self, _: &dyn Notifiable) -> NotificationResult<Option<SlackMessage>> {
        if self.mode == "legacy-bot" {
            return Ok(None);
        }
        if self.mode == "invalid" {
            return SlackMessage::new().usingBlockKitTemplate("[]").map(Some);
        }
        if self.mode == "invalid-image" {
            return Ok(Some(
                SlackMessage::new().imageBlockWith("https://example.test/chart.png", |_| {}),
            ));
        }
        if self.mode == "invalid-menu" {
            return SlackMessage::new()
                .tryActionsBlock(|block| {
                    block
                        .staticSelect("Choose an option")
                        .initialOption("missing")?;
                    Ok(())
                })
                .map(Some);
        }
        let mut message = SlackMessage::new().text(&self.mode);
        if self.mode == "blocks" {
            message = message
                .to("ignored-message-channel")
                .headerBlockWith("Invoice Paid", |header| {
                    header.id("invoice-heading").text().emoji();
                })
                .contextBlock(|block| {
                    block.text("Customer 42");
                    block.imageWith("https://example.test/avatar.png", |image| {
                        image.alt("Customer avatar");
                    });
                })
                .sectionBlock(|block| {
                    block.text("Payment received");
                    block.field("*Amount:* 42").markdown();
                })
                .dividerBlockWith(|divider| {
                    divider.id("invoice-divider");
                })
                .actionsBlock(|block| {
                    block.button("Acknowledge Invoice").primary().confirm(
                        "Confirm payment?",
                        |dialog| {
                            dialog.confirm("Continue").deny("Cancel");
                        },
                    );
                })
                .usingBlockKitTemplate(r#"{"blocks":[{"type":"divider"}]}"#)?
                .withMetadata(invoice_slack_metadata())
                .threadTimestamp("1710000000.001")
                .broadcastReply(false)
                .unfurlLinks(false);
            let preview = message.toBlockKitBuilderUrl()?;
            let (_, fragment) = preview.split_once('#').unwrap();
            let json: Json = serde_json::from_str(
                &percent_encoding::percent_decode_str(fragment)
                    .decode_utf8()
                    .unwrap(),
            )
            .unwrap();
            assert!(json.get("channel").is_none() && json.get("text").is_none());
            assert_eq!(json["blocks"].as_array().unwrap().len(), 6);
            assert_eq!(
                json["metadata"],
                Json::Object(invoice_slack_metadata().toArray())
            );
            assert_eq!(json["thread_ts"], "1710000000.001");
            assert_eq!(json["reply_broadcast"], false);
        } else if self.mode == "message" {
            message = message.to("message-channel");
        } else if self.mode.starts_with("webhook-") {
            message = message
                .headerBlock("Webhook notice")
                .metadata("webhook_notice", Default::default())
                .threadTimestamp("1710000000.000007");
        }
        if self.mode == "database" {
            message = message
                .withMetadata(invoice_slack_metadata())
                .threadTimestamp("1710000000.000007")
                .broadcastReply(true);
        } else if self.mode == "redis" {
            message = message
                .withMetadata(invoice_slack_metadata())
                .threadTimestamp("1710000000.000008")
                .broadcastReply(true)
                .clearThreadTimestamp()
                .broadcastReply(None);
        }
        if matches!(self.mode.as_str(), "blocks" | "database" | "redis") {
            message = message
                .imageBlockWith("https://example.test/chart.png", |image| {
                    image.id("invoice-chart").alt("Invoice chart");
                    image.title("February invoices").emoji();
                })
                .tryActionsBlock(|block| {
                    block
                        .staticSelect("Choose a status")
                        .id("invoice-status")
                        .addOption("Old", "Needs Review")
                        .addOption("Paid", "Paid")
                        .addOption("Review", "Needs Review")
                        .initialOption("Needs Review")?
                        .focus(false);
                    Ok(())
                })?
                .sectionBlock(|block| {
                    block.text("Assign invoice");
                    block.usersSelect("Select a teammate").initialUser("U123");
                });
        }
        Ok(Some(message))
    }

    fn to_legacy_slack(
        &self,
        _: &dyn Notifiable,
    ) -> NotificationResult<Option<openrust::notifications::messages::SlackMessage>> {
        if self.mode != "legacy-bot" && !self.mode.starts_with("webhook-legacy") {
            return Ok(None);
        }
        use openrust::notifications::messages::SlackMessage as LegacyMessage;

        Ok(Some(
            LegacyMessage::new()
                .error()
                .content(&self.mode)
                .from("Billing", Some(":moneybag:"))
                .from("Accounting", None)
                .linkNames()
                .unfurlLinks(false)
                .unfurlMedia(true)
                .attachment(|attachment| {
                    attachment
                        .title("Invoice", Some("https://example.test/invoices/42"))
                        .pretext("Payment status")
                        .content("*Pending*")
                        .fallback("Invoice pending")
                        .field("Status", "old")
                        .fieldWith(|field| {
                            field.title("Details").content("Payment pending").long();
                        })
                        .field("Status", "pending")
                        .markdown(&["text"])
                        .image("https://example.test/invoice.png")
                        .thumb("https://example.test/thumb.png")
                        .author("Billing", Some("https://example.test"), None)
                        .footer("Billing service")
                        .footerIcon("https://example.test/footer.png")
                        .action("Review", "https://example.test/invoices/42", None)
                        .timestamp(chrono::DateTime::from_timestamp(1_710_000_000, 0).unwrap())
                        .callbackId("invoice_status");
                })
                .attachment(|attachment| {
                    attachment.color("#123456");
                }),
        ))
    }

    fn queue_options(&self, _: &dyn Notifiable, _: &str) -> Option<DispatchOptions> {
        self.connection.map(|connection| DispatchOptions {
            connection: Some(connection.into()),
            queue: Some("slack-test".into()),
            ..Default::default()
        })
    }
}

fn invoice_slack_metadata() -> EventMetadata {
    EventMetadata::make(
        "invoice_created",
        [(
            "invoice".into(),
            json!({"id": 42, "paid": false, "tags": ["new"]}),
        )]
        .into_iter()
        .collect(),
    )
}

fn slack_from_http(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let mode = request.route("mode").unwrap().to_owned();
    Box::pin(async move {
        let notice = SlackNotice {
            mode,
            connection: None,
        };
        let result = if notice.mode.starts_with("webhook-") {
            let base = Config::get("fixture.slack_webhook").unwrap().unwrap();
            let base = base.as_str().unwrap();
            let webhook = if notice.mode == "webhook-invalid" {
                format!("{base}/route-secret?invalid=1")
            } else {
                format!("{base}/route-secret")
            };
            Notification::route("slack", NotificationRoute::Slack(webhook.into()))
                .unwrap()
                .notifyNow(&notice, None)
                .await
        } else if notice.mode == "blocks" {
            Notification::route("slack", NotificationRoute::Slack("override-channel".into()))
                .unwrap()
                .notifyNow(&notice, None)
                .await
        } else {
            Notification::sendNow(
                &[&openrust::notifications::AnonymousNotifiable::default()],
                &notice,
                None,
            )
            .await
        };
        result.map_err(|error| HttpError::Internal {
            source: Box::new(error),
        })?;
        Response::json(http::StatusCode::OK, json!({"processed":true}))
    })
}

fn notification_preview_from_http(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    let kind = request.route("kind").map(str::to_owned);
    Box::pin(async move {
        let data = || [("customer".into(), json!("Acme"))].into_iter().collect();
        let message = MailMessage::new()
            .mailer("missing-mailer")
            .attach(Attachment::fromPath("missing-attachment"))
            .withMessage(MessageCustomization::new("missing-customizer", Json::Null));
        let message = match kind.as_deref() {
            Some("html") => {
                let mail = message
                    .markdown("missing", Default::default())
                    .view("notice-html", data())
                    .text(
                        "notice-text",
                        [("customer".into(), json!("Beta"))].into_iter().collect(),
                    )
                    .template("missing");
                mail.assertSeeInText("Text Beta").await;
                mail
            }
            Some("text") => {
                let mail = message
                    .markdown("missing", Default::default())
                    .text("notice-text", data());
                assert_eq!(mail.render().await.unwrap(), "Text Acme");
                mail
            }
            Some("markdown") => {
                let mail = message
                    .view("missing", Default::default())
                    .text("missing", Default::default())
                    .markdown("notice-markdown", data())
                    .theme("notice-accent");
                mail.assertSeeInText("Markdown Acme").await;
                mail
            }
            Some("template") => message.template("notice-template"),
            _ => message.view("missing", data()),
        };
        let response = message.toResponse().await?;
        assert!(Mail::mailer(None).unwrap().sent().unwrap().is_empty());
        Ok(response)
    })
}

struct InvoicePaid<T>(std::marker::PhantomData<T>);

impl<T: Send + Sync> Notice for InvoicePaid<T> {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }

    fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        Ok(Some(MailDelivery::Message(Box::new(
            MailMessage::new().line("Invoice received"),
        ))))
    }
}

fn notification_format_from_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mail = MailMessage::new()
            .error()
            .greeting("Hello <Sam>")
            .line(" First line \r\n continued ")
            .lineIf(false, "hidden")
            .linesIf(true, ["Second line", "Third line"])
            .action(
                "View invoice",
                "https://example.test/invoice?src=mail&ref=1",
            )
            .lines(["After action", "Final line"])
            .salutation("Sincerely, Finance");
        let html = Mail::render(&mail).await.unwrap();
        mail.assertSeeInOrderInText(&[
            "Hello <Sam>",
            "First line continued",
            "Second line",
            "Third line",
            "View invoice",
            "After action",
            "Final line",
            "Sincerely, Finance",
            "https://example.test/invoice?src=mail&ref=1",
        ])
        .await;
        Notification::route(
            "mail",
            NotificationRoute::Mail(vec!["reader@example.test".into()]),
        )
        .unwrap()
        .notifyNow(&InvoicePaid(std::marker::PhantomData::<Envelope>), None)
        .await
        .unwrap();
        let sent = Mail::mailer(None).unwrap().sent().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].message.envelope().subject, "Invoice Paid");
        let error_html = Lang::withLocale("fr", Mail::render(&MailMessage::new().error()))
            .await
            .unwrap()
            .unwrap();
        let success_html = Mail::render(
            &MailMessage::new()
                .success()
                .action("Details", "https://example.test/details"),
        )
        .await
        .unwrap();
        Response::json(
            http::StatusCode::OK,
            json!({ "html": html, "localized": error_html, "success": success_html }),
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
            let named = NamedReceipt(name, input.matches().get_flag("inline-image"));
            let markdown = MarkdownReceipt {
                plain_view: input.matches().get_flag("plain-view"),
                options_fixture: input.matches().get_flag("markdown-options"),
                theme: None,
                inline: false,
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
                clap::Arg::new("inline-image")
                    .long("inline-image")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("markdown")
                    .long("markdown")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("markdown-options")
                    .long("markdown-options")
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
            Route::get("/attachment-mail", attachment_mail_from_http)?;
            Route::r#match(
                &[http::Method::POST],
                "/attachment-upload",
                uploaded_mail_from_http,
            )?;
            Route::get("/mail-assertions", mail_assertions_from_http)?;
            Route::get("/notification-format", notification_format_from_http)?;
            Route::get("/notification-envelope", notification_envelope_from_http)?;
            Route::get("/vonage/{mode}", vonage_from_http)?;
            Route::get("/slack/{mode}", slack_from_http)?;
            Route::get(
                "/notification-preview/{kind}",
                notification_preview_from_http,
            )?;
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
    let marker = root.0.join("customization-enabled");
    std::fs::write(&marker, b"enabled").unwrap();
    config["fixture"]["mail_customizer_marker"] = json!(marker);
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
                Receipt
                    .assertHasTag("receipt")
                    .assertHasMetadata("order-id", "R123");
                assert!(!Receipt.hasMetadata("order-id", "other").unwrap());
                let mut envelope = Receipt.envelope().unwrap();
                envelope.to = vec!["to@example.test".into()];
                envelope.tags.push("shipment".into());
                struct TrackingFixture(Envelope);
                impl Mailable for TrackingFixture {
                    fn content(&self) -> MailResult<Content> {
                        Receipt.content()
                    }
                    fn envelope(&self) -> MailResult<Envelope> {
                        Ok(self.0.clone())
                    }
                }
                let postmark = Mail::mailer(Some("postmark")).unwrap();
                assert!(
                    postmark
                        .sendNow(&TrackingFixture(envelope.clone()))
                        .await
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("single tag")
                );
                let mut invalid = envelope.clone();
                invalid
                    .metadata
                    .insert("key\r\nInjected".into(), "value".into());
                assert!(
                    postmark
                        .sendNow(&TrackingFixture(invalid))
                        .await
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("tracking header name")
                );
                envelope.tags = vec!["receipt\nInjected: value".into()];
                assert!(
                    postmark
                        .sendNow(&TrackingFixture(envelope))
                        .await
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("control characters")
                );
                let send = |name: &'static str, inline| async move {
                    Mail::to("to@example.test")
                        .unwrap()
                        .bcc("blind@example.test")
                        .sendNow(&NamedReceipt(name, inline))
                        .await
                };
                let ses = send("ses", false).await.unwrap();
                assert!(
                    String::from_utf8(ses.message.formatted())
                        .unwrap()
                        .contains("X-SES-Message-ID: ses-id")
                );
                for name in ["failover", "failover", "roundrobin", "roundrobin"] {
                    send(name, false).await.unwrap();
                }
                let rejected = send("reject", false).await.err().unwrap().to_string();
                assert!(rejected.contains("401") && !rejected.contains("marker"));
                for name in ["postmark", "resend"] {
                    send(name, true).await.unwrap();
                }
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
    assert_eq!(requests.len(), 9);
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
    assert_eq!(fields["Tags.member.1.Name"], "kind");
    assert_eq!(fields["Tags.member.2.Name"], "order-id");
    assert_eq!(fields["Tags.member.2.Value"], "R123");
    assert!(fields.values().any(|value| value == "blind@example.test"));
    let mime = String::from_utf8(STANDARD.decode(&fields["RawMessage.Data"]).unwrap()).unwrap();
    assert!(mime.contains("OpenRust receipt") && !mime.contains("blind@example.test"));
    let (_, headers, resend) = &requests[2];
    assert_eq!(headers["authorization"], "Bearer test-resend");
    let resend: Json = serde_json::from_str(resend).unwrap();
    assert_eq!(resend["bcc"], json!(["blind@example.test"]));
    assert_eq!(resend["tags"], json!([{"name":"X-Tag","value":"receipt"}]));
    assert_eq!(resend["headers"]["X-Metadata-order-id"], "R123");
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
    assert_eq!(postmark["Tag"], "receipt");
    assert_eq!(postmark["Metadata"], json!({"order-id":"R123"}));
    assert_eq!(postmark["Headers"][0]["Name"], "X-Receipt");
    assert_eq!(&paths[7..], ["/postmark", "/resend"]);
    let image = inline_logo();
    let image_base64 = STANDARD.encode(&image.attachment.bytes);
    let postmark_inline: Json = serde_json::from_str(&requests[7].2).unwrap();
    assert!(
        postmark_inline["Headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|header| header["Name"] == "X-Custom-Marker" && header["Value"] == "api-456")
    );
    assert_eq!(
        postmark_inline["TextBody"],
        "plain receipt - customized body"
    );
    assert!(
        postmark_inline["HtmlBody"]
            .as_str()
            .unwrap()
            .contains(&image.cid())
    );
    assert_eq!(postmark_inline["Attachments"][0]["Name"], "customized.txt");
    assert_eq!(
        postmark_inline["Attachments"][0]["Content"],
        STANDARD.encode(b"customizer bytes")
    );
    assert!(postmark_inline["Attachments"][1].get("ContentID").is_none());
    assert_eq!(postmark_inline["Attachments"][2]["ContentID"], image.cid());
    assert_eq!(postmark_inline["Attachments"][2]["Content"], image_base64);
    let resend_inline: Json = serde_json::from_str(&requests[8].2).unwrap();
    assert_eq!(
        resend_inline["tags"],
        json!([
            {"name":"X-Tag","value":"receipt"}, {"name":"X-Tag","value":"shipment"}
        ])
    );
    assert!(
        resend_inline["html"]
            .as_str()
            .unwrap()
            .contains(&image.cid())
    );
    assert!(resend_inline["attachments"][0].get("content_id").is_none());
    assert_eq!(
        resend_inline["attachments"][1]["content_id"],
        image.content_id
    );
    assert_eq!(resend_inline["attachments"][1]["content"], image_base64);
    server.abort();
}

#[tokio::test]
async fn artisan_delivers_through_ses_v2_mailgun_and_cloudflare() {
    use axum::{
        Router, body::Bytes, extract::State, http::HeaderMap, response::IntoResponse, routing::post,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    type Requests = Arc<Mutex<Vec<(String, HeaderMap, Bytes)>>>;
    async fn record(
        State(requests): State<Requests>,
        uri: http::Uri,
        headers: HeaderMap,
        body: Bytes,
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
    for name in ["mailgun-api", "cloudflare"] {
        artisan(&root.0, &["fixture:send-mail", name, "--inline-image"]).await;
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    let (path, headers, body) = &requests[0];
    assert_eq!(path, "/v2/email/outbound-emails");
    assert_eq!(headers["x-amz-security-token"], "test-session");
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .contains("Credential=test-key/")
    );
    let ses: Json = serde_json::from_slice(body).unwrap();
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
    assert_eq!(
        ses["EmailTags"][1],
        json!({"Name":"order-id","Value":"R123"})
    );
    assert_eq!(ses["TenantName"], "fixture-tenant");
    let mime = String::from_utf8(
        STANDARD
            .decode(ses["Content"]["Raw"]["Data"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(mime.contains("OpenRust receipt") && !mime.contains("blind@example.test"));
    let (path, headers, raw) = &requests[1];
    let raw = std::str::from_utf8(raw).unwrap();
    assert_eq!(path, "/v3/mg.example.test/messages.mime");
    assert!(raw.contains("X-Tag: receipt\r\n") && raw.contains("X-Metadata-order-id: R123\r\n"));
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
    let body = std::str::from_utf8(body).unwrap();
    assert!(body.contains("filename=\"receipt.txt\"") && body.contains("name=\"bcc\""));
    assert!(body.contains("name=\"o:tag\"\r\n\r\nreceipt\r\n"));
    assert!(body.contains("name=\"v:order-id\"\r\n\r\nR123\r\n"));
    let (path, headers, body) = &requests[3];
    assert_eq!(path, "/cloudflare");
    assert_eq!(headers["authorization"], "Bearer test-cloudflare");
    let cloudflare: Json = serde_json::from_slice(body).unwrap();
    assert_eq!(cloudflare["from"]["address"], "sender@example.test");
    assert_eq!(cloudflare["bcc"], json!(["blind@example.test"]));
    assert_eq!(cloudflare["headers"]["X-Tag"], "receipt");
    assert_eq!(cloudflare["headers"]["X-Metadata-order-id"], "R123");
    assert_eq!(
        cloudflare["attachments"][0]["content"],
        STANDARD.encode(b"file receipt")
    );
    let image = inline_logo();
    let (path, _, mailgun_inline) = &requests[4];
    assert_eq!(path, "/v3/mg.example.test/messages");
    for field in [
        format!("name=\"inline\"; filename=\"{}\"", image.content_id),
        format!("src=\"{}\"", image.cid()),
        "name=\"attachment\"; filename=\"receipt.txt\"".into(),
    ] {
        assert!(
            mailgun_inline
                .windows(field.len())
                .any(|part| part == field.as_bytes())
        );
    }
    assert!(
        mailgun_inline
            .windows(image.attachment.bytes.len())
            .any(|part| part == image.attachment.bytes.as_slice())
    );
    let (path, _, cloudflare_inline) = &requests[5];
    assert_eq!(path, "/cloudflare");
    let cloudflare_inline: Json = serde_json::from_slice(cloudflare_inline).unwrap();
    assert_eq!(cloudflare_inline["headers"]["X-Tag"], "shipment");
    assert!(
        cloudflare_inline["html"]
            .as_str()
            .unwrap()
            .contains(&image.cid())
    );
    assert_eq!(
        cloudflare_inline["attachments"][0]["disposition"],
        "attachment"
    );
    assert!(
        cloudflare_inline["attachments"][0]
            .get("content_id")
            .is_none()
    );
    assert_eq!(
        cloudflare_inline["attachments"][1],
        json!({
            "filename":"logo.png", "type":"image/png", "disposition":"inline",
            "content_id":image.content_id, "content":STANDARD.encode(&image.attachment.bytes),
        })
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
    config["mail"]["markdown"] = json!({"theme":"default"});
    config["app"]["locale"] = json!("fr");
    std::fs::write(&path, config.to_string()).unwrap();
    let markdown_view = root.0.join("resources/views/markdown-receipt.blade.html");
    std::fs::write(
        &markdown_view,
        r#"<x-mail::layout>
<x-slot:head><meta name="mail-layout-fixture" content="{{ $name }}"></x-slot:head>
<x-slot:header><x-mail::header :url="$portal">Account header</x-mail::header></x-slot:header>

# Layout receipt

<x-mail::button :url="$portal" color="green">View receipt</x-mail::button>

<x-slot:subcopy><x-mail::subcopy>Delivery terms.</x-mail::subcopy></x-slot:subcopy>
<x-slot:footer><x-mail::footer>Account footer</x-mail::footer></x-slot:footer>
</x-mail::layout>"#,
    )
    .unwrap();
    artisan(&root.0, &["fixture:send-mail", "smtp", "--markdown"]).await;
    let index: Json = reqwest::get(format!("{url}/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let layout_id = index["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| {
            message["Subject"] == "Markdown receipt fixture"
                && message["ID"] != id
                && message["ID"] != new_id
        })
        .unwrap()["ID"]
        .as_str()
        .unwrap();
    let layout: Json = reqwest::get(format!("{url}/message/{layout_id}"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let html = layout["HTML"].as_str().unwrap();
    for expected in [
        "<html lang=\"fr\"",
        "<title>Mail Components</title>",
        "mail-layout-fixture",
        "content=\"<Sam>\"",
        "max-width: 600px",
        "max-width: 500px",
        "class=\"inner-body\"",
        "width=\"570\"",
        "#16a34a",
        "box-sizing: border-box",
        "Account header",
        "Layout receipt",
        "Delivery terms.",
        "Account footer",
    ] {
        assert!(html.contains(expected), "missing {expected}: {html}");
    }
    assert_eq!(html.matches("<html").count(), 1, "{html}");
    assert!(!html.contains("openrust-mail-"));
    let text = layout["Text"].as_str().unwrap();
    let sections = [
        "Account header",
        "Layout receipt",
        "Delivery terms.",
        "Account footer",
    ];
    assert!(
        sections
            .windows(2)
            .all(|pair| { text.find(pair[0]).unwrap() < text.find(pair[1]).unwrap() }),
        "{text}"
    );
    assert!(!text.contains("mail-layout-fixture"));

    std::fs::write(
        &markdown_view,
        "Outside\n\n<x-mail::layout>Inside</x-mail::layout>",
    )
    .unwrap();
    let mut fixture = openrust::testing::TestApp::create(|_| app_builder(&root.0)).unwrap();
    let result = fixture
        .artisan(["fixture:send-mail", "smtp", "--markdown"])
        .await;
    result.assertExitCode(1);
    assert!(
        result
            .stderr
            .contains("The mail layout must wrap the entire view."),
        "{result:?}"
    );
}

#[tokio::test]
async fn artisan_mail_markdown_extensions_and_secured_echo_are_application_local() {
    let ports = std::env::var("OPENRUST_TEST_SMTP_PORT")
        .ok()
        .zip(std::env::var("OPENRUST_TEST_SMTP_HTTP_PORT").ok());
    let Some((smtp, http)) = ports else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let smtp = smtp.parse().unwrap();
    for secured in [true, false] {
        let root = fixture(None, None, Some(smtp));
        let path = root.0.join("config.json");
        let mut config: Json = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if secured {
            config["mail"]["markdown"] = json!({
                "secured_encoding":true,
                "extensions":["strikethrough","task_lists","footnotes","smart_punctuation"]
            });
        }
        std::fs::write(&path, config.to_string()).unwrap();
        let label = if secured {
            "Secured echo"
        } else {
            "Default echo"
        };
        let view = root.0.join("resources/views/markdown-receipt.blade.html");
        std::fs::write(
            &view,
            format!(
                r#"<x-mail::layout>
<x-slot:head><meta name="markdown-secured" content="{{{{ $name }}}}"></x-slot:head>

# {label}

{{{{ $markdown_text }}}}

`{{{{ $markdown_text }}}}`

"Quoted" ~~obsolete~~ [Open]({{{{ $portal }}}}) [Blocked]({{{{ $blocked_url }}}})[^terms]

- [x] Completed

[^terms]: Terms apply.
</x-mail::layout>"#
            ),
        )
        .unwrap();
        let mut fixture = openrust::testing::TestApp::create(|_| app_builder(&root.0)).unwrap();
        fixture
            .artisan([
                "fixture:send-mail",
                "smtp",
                "--markdown",
                "--markdown-options",
            ])
            .await
            .assertSuccessful();
    }
    let client = reqwest::Client::new();
    let api = format!("http://127.0.0.1:{http}/api/v1");
    let listing: Json = client
        .get(format!("{api}/messages"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut verified = 0;
    for row in listing["messages"].as_array().unwrap() {
        if row["Subject"] != "Markdown options fixture" {
            continue;
        }
        let id = row["ID"].as_str().unwrap();
        let receipt: Json = client
            .get(format!("{api}/message/{id}"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let html = receipt["HTML"].as_str().unwrap();
        let text = receipt["Text"].as_str().unwrap();
        if html.contains("Secured echo") {
            assert_eq!(
                html.matches("[Literal](https://example.test/local) *marks*")
                    .count(),
                2
            );
            assert!(!html.contains("href=\"https://example.test/local"));
            assert!(html.contains("<del") && html.contains("obsolete"));
            assert!(html.contains("checkbox") && html.contains("footnote-reference"));
            assert!(html.contains("“Quoted”") && html.contains("step=1&amp;code=2"));
            assert!(html.contains("content=\"<Sam>\"") && !html.contains("orecho"));
            assert!(text.contains("[Literal](https://example.test/local) *marks*"));
            assert!(text.contains("[x] Completed") && text.contains("[^terms]: Terms apply."));
        } else if html.contains("Default echo") {
            assert!(html.contains("href=\"https://example.test/local"));
            assert!(html.contains("~~obsolete~~") && !html.contains("checkbox"));
            assert!(html.contains("[^terms]") && !html.contains("footnote-reference"));
        } else {
            continue;
        }
        assert!(!html.contains("javascript:"));
        verified += 1;
    }
    assert_eq!(verified, 2);

    let root = fixture(None, None, None);
    let path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["mail"]["markdown"] = json!({"extensions":["unknown"]});
    std::fs::write(&path, config.to_string()).unwrap();
    let mut fixture = openrust::testing::TestApp::create(|_| app_builder(&root.0)).unwrap();
    let result = fixture
        .artisan(["fixture:send-mail", "array", "--markdown"])
        .await;
    result.assertExitCode(1);
    assert!(
        result
            .stderr
            .contains("Unsupported mail.markdown.extensions"),
        "{result:?}"
    );
}

#[tokio::test]
async fn markdown_mailable_selects_theme_for_immediate_and_queued_delivery() {
    let services = (
        std::env::var("OPENRUST_TEST_DELIVERY_MYSQL_URL"),
        std::env::var("OPENRUST_TEST_REDIS_PORT"),
        std::env::var("OPENRUST_TEST_SMTP_PORT"),
        std::env::var("OPENRUST_TEST_SMTP_HTTP_PORT"),
    );
    let (Ok(mysql), Ok(redis), Ok(smtp), Ok(http)) = services else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let root = fixture(
        Some(redis.parse().unwrap()),
        Some(&mysql),
        Some(smtp.parse().unwrap()),
    );
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["mail"]["markdown"] = json!({"theme":"application"});
    std::fs::write(&config_path, config.to_string()).unwrap();
    let theme_directory = root.0.join("resources/views/vendor/mail/html/themes");
    std::fs::create_dir_all(&theme_directory).unwrap();
    for (name, color) in [("application", "#179abc"), ("receipt", "#8934ab")] {
        std::fs::write(
            theme_directory.join(format!("{name}.css")),
            format!("h1 {{color: {color}}}"),
        )
        .unwrap();
    }
    std::fs::write(
        root.0.join("resources/views/markdown-receipt.blade.html"),
        "# Mailable theme contract\n\n![Receipt logo]({{ $logo }})",
    )
    .unwrap();
    std::fs::write(
        root.0.join("resources/views/plain-receipt.blade.html"),
        "Unstyled receipt text",
    )
    .unwrap();
    let mut app = app(&root.0);
    app.boot().await.unwrap();
    let db = setup(&app).await;
    let queue = app.container().make::<QueueManager>().unwrap();
    let connections = ["database", "redis"];
    app.call(
        Callback::new(&[], move |_| async move {
            for theme in [Some("receipt"), None, Some("")] {
                Mail::to("theme@example.test")
                    .unwrap()
                    .sendNow(&MarkdownReceipt {
                        plain_view: true,
                        options_fixture: true,
                        theme: theme.map(str::to_owned),
                        inline: true,
                    })
                    .await
                    .unwrap();
            }
            let missing = Mail::to("theme@example.test")
                .unwrap()
                .sendNow(&MarkdownReceipt {
                    plain_view: true,
                    options_fixture: true,
                    theme: Some("missing".into()),
                    inline: true,
                })
                .await
                .err()
                .unwrap();
            assert!(
                missing
                    .to_string()
                    .contains("Markdown mail theme was not found")
            );
            for connection in connections {
                let dispatch = Mail::to("theme@example.test")
                    .unwrap()
                    .queue(
                        &MarkdownReceipt {
                            plain_view: true,
                            options_fixture: true,
                            theme: Some("receipt".into()),
                            inline: true,
                        },
                        DispatchOptions {
                            connection: Some(connection.into()),
                            queue: Some("mail-theme".into()),
                            ..Default::default()
                        },
                        None,
                    )
                    .await
                    .unwrap();
                assert!(dispatch.is_some());
            }
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    for connection in connections {
        let connection_queue = queue.connection(Some(connection)).unwrap();
        assert_eq!(connection_queue.size(Some("mail-theme")).await.unwrap(), 1);
        artisan(
            &root.0,
            &[
                "queue:work",
                connection,
                "--queue=mail-theme",
                "--stop-when-empty",
                "--sleep=0",
            ],
        )
        .await;
        assert_eq!(connection_queue.size(Some("mail-theme")).await.unwrap(), 0);
    }
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);

    let api = format!("http://127.0.0.1:{http}/api/v1");
    let listing: Json = reqwest::get(format!("{api}/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut overrides = 0;
    let mut defaults = 0;
    for row in listing["messages"].as_array().unwrap() {
        if row["Subject"] != "Markdown options fixture" {
            continue;
        }
        let id = row["ID"].as_str().unwrap();
        let message: Json = reqwest::get(format!("{api}/message/{id}"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let html = message["HTML"].as_str().unwrap();
        if !html.contains("Mailable theme contract") {
            continue;
        }
        let raw = reqwest::get(format!("{api}/message/{id}/raw"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        for marker in [
            "multipart/mixed",
            "multipart/alternative",
            "multipart/related",
            "Content-ID: <logo@images.example.test>",
            "Content-Disposition: inline",
            "image/png",
            "cid:logo@images.example.test",
            "receipt.txt",
        ] {
            assert!(raw.contains(marker), "{marker}: {raw}");
        }
        assert_eq!(
            message["Text"].as_str().unwrap().trim(),
            "Unstyled receipt text"
        );
        match (html.contains("#8934ab"), html.contains("#179abc")) {
            (true, false) => overrides += 1,
            (false, true) => defaults += 1,
            _ => panic!("Expected exactly one theme: {html}"),
        }
    }
    assert_eq!((overrides, defaults), (3, 2));
    drop_delivery_tables(&db).await;
}

#[tokio::test]
async fn notification_templates_preview_over_http_without_delivering() {
    let root = fixture(None, None, None);
    let views = root.0.join("resources/views");
    for (name, body) in [
        ("notice-html", "<strong>HTML {{ $customer }}</strong>"),
        ("notice-text", "Text {{ $customer }}"),
        ("notice-markdown", "# Markdown {{ $customer }}"),
        ("notice-template", "# Custom template"),
    ] {
        std::fs::write(views.join(format!("{name}.blade.html")), body).unwrap();
    }
    let themes = views.join("mail");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("notice-accent.blade.html"),
        "h1 { color: #123abc; }",
    )
    .unwrap();
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "notification-preview").await;
    for (kind, media_type, body) in [
        (
            "html",
            "text/html; charset=utf-8",
            "<strong>HTML Beta</strong>",
        ),
        ("text", "text/plain; charset=utf-8", "Text Acme"),
        ("markdown", "text/html; charset=utf-8", "Markdown Acme"),
        ("template", "text/html; charset=utf-8", "Custom template"),
    ] {
        let response = reqwest::get(format!("{url}/{kind}"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        assert_eq!(response.headers()[http::header::CONTENT_TYPE], media_type);
        let rendered = response.text().await.unwrap();
        assert!(rendered.contains(body), "{rendered}");
        if kind == "markdown" {
            assert!(rendered.contains("#123abc"), "{rendered}");
        }
    }
    assert_eq!(
        reqwest::get(format!("{url}/missing"))
            .await
            .unwrap()
            .status(),
        http::StatusCode::INTERNAL_SERVER_ERROR
    );
    server.abort();
}

#[tokio::test]
async fn formatted_notification_mail_renders_over_http_and_publishes_the_view() {
    let root = fixture(None, None, None);
    std::fs::create_dir_all(root.0.join("lang")).unwrap();
    std::fs::write(
        root.0.join("lang/fr.json"),
        r#"{"Whoops!":"Oups !","Regards,":"Cordialement,"}"#,
    )
    .unwrap();
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "notification-format").await;
    let values: Json = reqwest::get(url)
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let html = values["html"].as_str().unwrap();
    for marker in [
        "Hello &lt;Sam&gt;",
        "First line continued",
        "button-error",
        "https://example.test/invoice?src=mail&amp;ref=1",
        "If you're having trouble",
        "Sincerely, Finance",
    ] {
        assert!(html.contains(marker), "{marker}: {html}");
    }
    assert!(!html.contains("hidden"));
    let localized = values["localized"].as_str().unwrap();
    assert!(localized.contains("Oups !") && localized.contains("Cordialement,"));
    assert!(!localized.contains("&lt;br&gt;"));
    assert!(
        values["success"]
            .as_str()
            .unwrap()
            .contains("button-success")
    );
    server.abort();

    let published = root
        .0
        .join("resources/views/vendor/notifications/email.blade.html");
    assert!(!published.exists());
    artisan(&root.0, &["vendor:publish", "--tag=laravel-notifications"]).await;
    assert!(
        std::fs::read_to_string(&published)
            .unwrap()
            .contains("$introLines")
    );
    std::fs::write(&published, "# Overridden notification email").unwrap();
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    application
        .call(
            Callback::new(&[], |_| async {
                let html = Mail::render(&MailMessage::new()).await.unwrap();
                assert!(html.contains("Overridden notification email"));
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn mailable_content_assertions_render_without_delivering_mail() {
    let root = fixture(None, None, None);
    let views = root.0.join("resources/views");
    for (name, body) in [
        (
            "assertion-html",
            "<span>{{ $locale }}</span> {{ $name }} caf&eacute; final",
        ),
        ("assertion-text", "Plain {{ $locale }} receipt"),
        ("assertion-markdown", "# Markdown proof"),
    ] {
        std::fs::write(views.join(format!("{name}.blade.html")), body).unwrap();
    }
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "mail-assertions").await;
    let response = reqwest::get(url).await.unwrap().error_for_status().unwrap();
    assert_eq!(
        response.json::<Json>().await.unwrap(),
        json!({"asserted":true})
    );
    server.abort();
}

#[tokio::test]
async fn mailable_preview_renders_body_over_http_without_resolving_delivery() {
    struct PreviewMail(Content);

    impl Mailable for PreviewMail {
        fn inline_attachments(&self) -> MailResult<Vec<InlineAttachment>> {
            assert!(self.0.view.is_some() || self.0.markdown.is_some() || self.0.html.is_some());
            Ok(vec![inline_logo()])
        }

        fn envelope(&self) -> MailResult<Envelope> {
            panic!("Preview must not prepare an envelope");
        }

        fn content(&self) -> MailResult<Content> {
            assert_eq!(Lang::getLocale().unwrap(), "fr");
            Ok(self.0.clone())
        }

        fn locale(&self) -> Option<&str> {
            Some("fr")
        }
    }

    fn markdown() -> PreviewMail {
        PreviewMail(Content {
            markdown: Some("preview".into()),
            text_view: Some("unavailable-text-view".into()),
            theme: Some("preview".into()),
            with: [("name".into(), json!("<Client>"))].into_iter().collect(),
            ..Default::default()
        })
    }

    fn html_preview(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { markdown().toResponse().await })
    }

    fn text_preview(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            PreviewMail(Content {
                text: Some("<Client>".into()),
                ..Default::default()
            })
            .toResponse()
            .await
        })
    }

    fn html_cid_preview(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            PreviewMail(Content {
                html: Some(concat!(
                    "<!-- <img src='cid:logo@images.example.test'> -->",
                    "<textarea><img src='cid:logo@images.example.test'></textarea>",
                    "<img alt=logo data-src='cid:logo@images.example.test' SRC=CID:logo@images.example.test>",
                    "<img alt=missing src='cid:missing@images.example.test'>",
                ).into()),
                ..Default::default()
            }).toResponse().await
        })
    }

    fn missing_preview(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            PreviewMail(Content {
                view: Some("unavailable-view".into()),
                ..Default::default()
            })
            .toResponse()
            .await
        })
    }

    let root = fixture(None, None, None);
    let path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["mail"]["default"] = json!("no-preview-transport");
    std::fs::write(path, config.to_string()).unwrap();
    std::fs::create_dir(root.0.join("lang")).unwrap();
    std::fs::write(
        root.0.join("lang/fr.json"),
        r#"{"All rights reserved.":"Tous droits réservés."}"#,
    )
    .unwrap();
    let themes = root.0.join("resources/views/vendor/mail/html/themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(themes.join("preview.css"), "h1 { color: #217a4b; }").unwrap();
    std::fs::write(
        root.0.join("resources/views/preview.blade.html"),
        "<x-mail::message>\n# Preview {{ $name }}\n\n![Preview logo](cid:logo@images.example.test)\n</x-mail::message>",
    )
    .unwrap();
    let mut kernel = Kernel::new(
        app_builder(&root.0)
            .withRouting(Routing::default().web(|| {
                Route::get("/preview", html_preview)?;
                Route::get("/preview-text", text_preview)?;
                Route::get("/preview-cid", html_cid_preview)?;
                Route::get("/preview-missing", missing_preview)
            }))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    kernel
        .get_application()
        .call(
            Callback::new(&[], |_| async {
                let html = markdown().render().await.unwrap();
                assert!(html.contains("Tous droits réservés."));
                assert_eq!(Lang::getLocale().unwrap(), "en");
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();

    let (url, server) = serve_fixture_http(kernel, "preview").await;
    let html = reqwest::get(&url)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        html.headers()[http::header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let html = html.text().await.unwrap();
    for expected in [
        "lang=\"fr\"",
        "#217a4b",
        "&lt;Client&gt;",
        "Tous droits réservés.",
        "src=\"data:image/png;base64,iVBORw0KGgo",
    ] {
        assert!(html.contains(expected), "{html}");
    }
    let text = reqwest::get(format!("{url}-text"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        text.headers()[http::header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
    assert_eq!(text.text().await.unwrap(), "<Client>");
    let cid_preview = reqwest::get(format!("{url}-cid"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(cid_preview.matches("data:image/png;base64,").count(), 1);
    assert!(cid_preview.contains("SRC=\"data:image/png;base64,"));
    assert!(cid_preview.contains("data-src='cid:logo@images.example.test'"));
    assert!(cid_preview.contains("src='cid:missing@images.example.test'"));
    let missing = reqwest::get(format!("{url}-missing")).await.unwrap();
    assert_eq!(missing.status(), 500);
    server.abort();
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
    for table in ["notifications", "jobs", "failed_jobs", "sms_deliveries"] {
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
    if smtp.is_some()
        && let Ok(port) = std::env::var("OPENRUST_TEST_SMTP_HTTP_PORT")
    {
        let api = format!("http://127.0.0.1:{port}/api/v1");
        let listing: Json = reqwest::get(format!("{api}/messages"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let payment = listing["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["Subject"] == "OpenRust payment (fr)")
            .expect("queued notification delivered to Mailpit");
        let id = payment["ID"].as_str().unwrap();
        let message: Json = reqwest::get(format!("{api}/message/{id}"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(message["HTML"].as_str().unwrap().contains("button-primary"));
        assert!(
            message["HTML"]
                .as_str()
                .unwrap()
                .contains("Thank you for your payment")
        );
        assert!(
            message["Text"]
                .as_str()
                .unwrap()
                .contains("https://example.test/payments/1")
        );
    }
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
async fn slack_bot_api_delivers_blocks_and_keeps_workspace_tokens_out_of_queue_plaintext() {
    use axum::response::IntoResponse as _;
    use std::sync::atomic::{AtomicBool, Ordering};

    let (Some(mysql), Ok(redis)) = (delivery_mysql(), std::env::var("OPENRUST_TEST_REDIS_PORT"))
    else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let records = Arc::new(Mutex::new(Vec::new()));
    let fail_once = Arc::new(AtomicBool::new(true));
    let endpoint = axum::Router::new().route(
        "/api/{method}",
        axum::routing::post({
            let records = Arc::clone(&records);
            move |axum::extract::Path(method): axum::extract::Path<String>,
                  headers: http::HeaderMap,
                  body: axum::body::Bytes| {
                let records = Arc::clone(&records);
                let fail_once = Arc::clone(&fail_once);
                async move {
                    assert_eq!(method, "chat.postMessage");
                    assert_eq!(headers[http::header::CONTENT_TYPE], "application/json");
                    let auth = headers[http::header::AUTHORIZATION]
                        .to_str()
                        .unwrap()
                        .to_owned();
                    let payload: Json = serde_json::from_slice(&body).unwrap();
                    let text = payload["text"].as_str().unwrap().to_owned();
                    records.lock().unwrap().push((auth, payload));
                    if text == "redirect" {
                        return axum::response::Redirect::temporary("/api/unexpected")
                            .into_response();
                    }
                    let body = if text == "reject"
                        || (text == "database" && fail_once.swap(false, Ordering::Relaxed))
                    {
                        json!({"ok":false,"error":"fixture provider detail"})
                    } else if text == "malformed" {
                        json!({"unexpected":true})
                    } else {
                        json!({"ok":true,"channel":"C123","ts":"1710000000.001"})
                    };
                    (
                        [(http::header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    )
                        .into_response()
                }
            }
        }),
    );
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let provider = tokio::spawn(async move { axum::serve(socket, endpoint).await });
    let root = fixture(Some(redis.parse().unwrap()), Some(&mysql), None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["app"]["key"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    config["services"] = json!({"slack":{"notifications":{
        "bot_user_oauth_token":"fixture-default-token",
        "channel":"configured-channel",
        "endpoint":format!("http://{address}/api/chat.postMessage")
    }}});
    std::fs::write(&config_path, config.to_string()).unwrap();
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "slack").await;
    for (mode, status) in [
        ("default", http::StatusCode::OK),
        ("message", http::StatusCode::OK),
        ("blocks", http::StatusCode::OK),
        ("invalid", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("invalid-menu", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("invalid-image", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("reject", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("malformed", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("redirect", http::StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let response = reqwest::get(format!("{url}/{mode}")).await.unwrap();
        assert_eq!(response.status(), status, "{mode}");
        assert!(
            !response
                .text()
                .await
                .unwrap()
                .contains("fixture provider detail")
        );
    }
    server.abort();
    assert_eq!(records.lock().unwrap().len(), 6);
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    let db = setup(&application).await;
    application
        .call(
            Callback::new(&[], |_| async {
                for (connection, token) in [
                    ("database", Some("workspace-token".into())),
                    ("redis", None),
                ] {
                    let route = SlackRoute::make(Some(format!("{connection}-channel")), token);
                    Notification::route("slack", NotificationRoute::Slack(route))
                        .unwrap()
                        .notify(&SlackNotice {
                            mode: connection.into(),
                            connection: Some(connection),
                        })
                        .await
                        .unwrap();
                }
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let Value::Text(payload) = db.table("jobs").value("payload").await.unwrap() else {
        panic!("expected a JSON queue payload");
    };
    assert!(payload.contains("encrypted_token") && !payload.contains("workspace-token"));
    assert_eq!(records.lock().unwrap().len(), 6);
    config["services"]["slack"]["notifications"]["bot_user_oauth_token"] =
        json!("fixture-rotated-token");
    std::fs::write(&config_path, config.to_string()).unwrap();
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=slack-test",
            "--once",
            "--tries=2",
        ],
    )
    .await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Unsigned(1)
    );
    assert_eq!(records.lock().unwrap().len(), 7);
    for connection in ["database", "redis"] {
        artisan(
            &root.0,
            &[
                "queue:work",
                connection,
                "--queue=slack-test",
                "--stop-when-empty",
                "--sleep=0",
                "--tries=2",
            ],
        )
        .await;
    }
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    drop_delivery_tables(&db).await;
    provider.abort();
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 9);
    assert_eq!(records[0].0, "Bearer fixture-default-token");
    assert_eq!(records[0].1["channel"], "configured-channel");
    assert_eq!(records[1].1["channel"], "message-channel");
    let block_message = &records[2].1;
    assert_eq!(block_message["channel"], "override-channel");
    assert_eq!(
        block_message["blocks"][0],
        json!({"type":"header","block_id":"invoice-heading","text":{"type":"plain_text","text":"Invoice Paid","emoji":true}})
    );
    assert_eq!(
        block_message["blocks"][1]["elements"][1],
        json!({"type":"image","image_url":"https://example.test/avatar.png","alt_text":"Customer avatar"})
    );
    assert_eq!(block_message["blocks"][2]["fields"][0]["type"], "mrkdwn");
    assert_eq!(
        block_message["blocks"][3],
        json!({"type":"divider","block_id":"invoice-divider"})
    );
    let button = &block_message["blocks"][4]["elements"][0];
    assert_eq!(button["action_id"], "button_acknowledge-invoice");
    assert_eq!(button["style"], "primary");
    assert_eq!(button["confirm"]["confirm"]["text"], "Continue");
    assert_eq!(block_message["thread_ts"], "1710000000.001");
    assert_eq!(block_message["reply_broadcast"], false);
    assert_eq!(
        block_message["metadata"],
        json!({"event_type":"invoice_created","event_payload":{"invoice":{"id":42,"paid":false,"tags":["new"]}}})
    );
    assert_eq!(block_message["unfurl_links"], false);
    assert!(
        records[6..8]
            .iter()
            .all(|(token, message)| token == "Bearer workspace-token"
                && message["channel"] == "database-channel")
    );
    assert_eq!(records[8].0, "Bearer fixture-rotated-token");
    assert_eq!(records[8].1["channel"], "redis-channel");
    for (_, message) in &records[6..8] {
        assert_eq!(message["thread_ts"], "1710000000.000007");
        assert_eq!(message["reply_broadcast"], true);
        assert_eq!(message["metadata"], block_message["metadata"]);
    }
    let redis_message = &records[8].1;
    assert!(redis_message.get("thread_ts").is_none());
    assert!(redis_message.get("reply_broadcast").is_none());
    assert_eq!(redis_message["metadata"], block_message["metadata"]);
    for message in [&records[2].1, &records[6].1, &records[7].1, &records[8].1] {
        let blocks = message["blocks"].as_array().unwrap();
        assert_eq!(
            blocks[blocks.len() - 3],
            json!({
                "type":"image","block_id":"invoice-chart","image_url":"https://example.test/chart.png",
                "alt_text":"Invoice chart","title":{"type":"plain_text","text":"February invoices","emoji":true}
            })
        );
        let menu = &blocks[blocks.len() - 2]["elements"][0];
        assert_eq!(
            menu,
            &json!({
                "type":"static_select", "action_id":"invoice-status", "focus_on_load":false,
                "placeholder":{"type":"plain_text","text":"Choose a status"},
                "options":[
                    {"text":{"type":"plain_text","text":"Review"},"value":"needsreview"},
                    {"text":{"type":"plain_text","text":"Paid"},"value":"paid"}
                ],
                "initial_option":{"text":{"type":"plain_text","text":"Review"},"value":"needsreview"}
            })
        );
        let users = &blocks.last().unwrap()["accessory"];
        assert!(
            users["action_id"]
                .as_str()
                .unwrap()
                .starts_with("users_select_")
        );
        assert_eq!(users["initial_user"], "U123");
        assert_eq!(users["placeholder"]["text"], "Select a teammate");
    }
}

#[test]
fn slack_visual_blocks_validate_identifiers_alt_text_and_image_bounds() {
    let blocks = SlackMessage::new()
        .dividerBlockWith(|divider| {
            divider.id("0");
        })
        .imageBlockWith("u".repeat(3000), |image| {
            image.id("i".repeat(255)).alt("a".repeat(2000));
            image.title("界".repeat(667));
        })
        .toArray()
        .unwrap();
    assert_eq!(blocks["blocks"][0], json!({"type":"divider"}));
    assert_eq!(
        blocks["blocks"][1]["title"]["text"],
        format!("{}...", "界".repeat(665))
    );
    assert_eq!(
        blocks["blocks"][1]["alt_text"].as_str().unwrap().len(),
        2000
    );

    for invalid in [
        SlackMessage::new().headerBlockWith("Header", |block| {
            block.id("界".repeat(86));
        }),
        SlackMessage::new().dividerBlockWith(|block| {
            block.id("a".repeat(256));
        }),
        SlackMessage::new().imageBlock("u".repeat(3001), "Alt"),
        SlackMessage::new().imageBlock("url", "a".repeat(2001)),
        SlackMessage::new().imageBlockWith("url", |_| {}),
        SlackMessage::new().imageBlockWith("url", |image| {
            image.alt("Alt").title("");
        }),
        SlackMessage::new().contextBlock(|block| {
            block.imageWith("url", |_| {});
        }),
    ] {
        assert!(invalid.toArray().is_err());
    }
}

#[test]
fn slack_block_kit_matches_text_and_identifier_boundaries() {
    let payload = SlackMessage::new()
        .headerBlock("界".repeat(51))
        .sectionBlock(|block| {
            block.text("plain").verbatim().emoji();
            block.field("a".repeat(2001)).emoji().markdown().verbatim();
        })
        .actionsBlock(|block| {
            block
                .button("a".repeat(250))
                .emoji()
                .confirm("prompt", |dialog| {
                    dialog.title("T".repeat(101)).confirm("Y".repeat(31));
                    dialog.text("**Confirm**").markdown().verbatim();
                });
        })
        .toArray()
        .unwrap();
    let blocks = &payload["blocks"];
    assert_eq!(blocks[0]["text"]["text"], format!("{}...", "界".repeat(49)));
    assert_eq!(
        blocks[1]["text"],
        json!({"type":"plain_text","text":"plain","emoji":true})
    );
    assert_eq!(
        blocks[1]["fields"][0],
        json!({
            "type":"mrkdwn","text":format!("{}...", "a".repeat(1997)),"verbatim":true
        })
    );
    let button = &blocks[2]["elements"][0];
    assert_eq!(button["text"]["text"], format!("{}...", "a".repeat(72)));
    assert_eq!(button["text"]["emoji"], true);
    assert_eq!(button["action_id"].as_str().unwrap().len(), 255);
    assert_eq!(
        button["confirm"]["title"]["text"],
        format!("{}...", "T".repeat(97))
    );
    assert_eq!(
        button["confirm"]["confirm"]["text"],
        format!("{}...", "Y".repeat(27))
    );
    assert_eq!(
        button["confirm"]["text"],
        json!({
            "type":"mrkdwn","text":"**Confirm**","verbatim":true
        })
    );

    for invalid in [
        SlackMessage::new().headerBlock(""),
        SlackMessage::new().actionsBlock(|block| {
            block.button("Click").url("a".repeat(3001));
        }),
        SlackMessage::new().actionsBlock(|block| {
            block.button("Click").accessibilityLabel("a".repeat(76));
        }),
        SlackMessage::new().actionsBlock(|block| {
            block.usersSelect("Choose").id("界".repeat(86));
        }),
        SlackMessage::new().actionsBlock(|block| {
            block.staticSelect("");
        }),
    ] {
        assert!(invalid.toArray().is_err());
    }
}

#[tokio::test]
async fn slack_webhook_routes_send_blocks_and_legacy_attachments_with_encrypted_urls() {
    use axum::response::IntoResponse as _;
    use std::sync::atomic::{AtomicBool, Ordering};

    let (Some(mysql), Ok(redis)) = (delivery_mysql(), std::env::var("OPENRUST_TEST_REDIS_PORT"))
    else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let records = Arc::new(Mutex::new(Vec::new()));
    let fail_once = Arc::new(AtomicBool::new(true));
    let endpoint = axum::Router::new().route(
        "/hook/{secret}",
        axum::routing::post({
            let records = Arc::clone(&records);
            move |axum::extract::Path(secret): axum::extract::Path<String>,
                  headers: http::HeaderMap,
                  body: axum::body::Bytes| {
                let records = Arc::clone(&records);
                let fail_once = Arc::clone(&fail_once);
                async move {
                    assert_eq!(secret, "route-secret");
                    assert_eq!(headers[http::header::CONTENT_TYPE], "application/json");
                    assert!(!headers.contains_key(http::header::AUTHORIZATION));
                    let payload: Json = serde_json::from_slice(&body).unwrap();
                    let text = payload["text"].as_str().unwrap().to_owned();
                    if text.starts_with("webhook-legacy") {
                        assert_eq!(
                            payload,
                            json!({
                                "text": text,
                                "username": "Accounting",
                                "icon_emoji": ":moneybag:",
                                "link_names": 1,
                                "unfurl_media": true,
                                "attachments": [{
                                    "title": "Invoice",
                                    "title_link": "https://example.test/invoices/42",
                                    "pretext": "Payment status",
                                    "text": "*Pending*",
                                    "fallback": "Invoice pending",
                                    "color": "danger",
                                    "fields": [
                                        {"title":"Status","value":"pending","short":true},
                                        {"title":"Details","value":"Payment pending","short":false}
                                    ],
                                    "mrkdwn_in": ["text"],
                                    "image_url": "https://example.test/invoice.png",
                                    "thumb_url": "https://example.test/thumb.png",
                                    "author_name": "Billing",
                                    "author_link": "https://example.test",
                                    "footer": "Billing service",
                                    "footer_icon": "https://example.test/footer.png",
                                    "actions": [{
                                        "type": "button", "text": "Review",
                                        "url": "https://example.test/invoices/42", "style": ""
                                    }],
                                    "ts": 1_710_000_000,
                                    "callback_id": "invoice_status"
                                }, {"color": "#123456"}]
                            })
                        );
                    } else {
                        assert!(payload.get("channel").is_none());
                        assert_eq!(payload["blocks"][0]["text"]["text"], "Webhook notice");
                        assert_eq!(
                            payload["metadata"],
                            json!({"event_type":"webhook_notice","event_payload":{}})
                        );
                        assert_eq!(payload["thread_ts"], "1710000000.000007");
                        assert!(payload.get("reply_broadcast").is_none());
                    }
                    records.lock().unwrap().push(text.clone());
                    if text == "webhook-redirect" {
                        return axum::response::Redirect::temporary("/hook/unexpected")
                            .into_response();
                    }
                    if text == "webhook-reject"
                        || (text == "webhook-legacy-database"
                            && fail_once.swap(false, Ordering::Relaxed))
                    {
                        return (http::StatusCode::FORBIDDEN, "fixture provider detail")
                            .into_response();
                    }
                    "ok".into_response()
                }
            }
        }),
    );
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/hook", socket.local_addr().unwrap());
    let provider = tokio::spawn(async move { axum::serve(socket, endpoint).await });
    let root = fixture(Some(redis.parse().unwrap()), Some(&mysql), None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["app"]["key"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    config["fixture"]["slack_webhook"] = json!(base);
    std::fs::write(&config_path, config.to_string()).unwrap();

    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "slack").await;
    for (mode, expected) in [
        ("webhook-blocks", 200),
        ("webhook-invalid", 500),
        ("webhook-reject", 500),
        ("webhook-redirect", 500),
        ("webhook-legacy", 200),
        ("legacy-bot", 200),
    ] {
        let response = reqwest::get(format!("{url}/{mode}")).await.unwrap();
        assert_eq!(response.status().as_u16(), expected, "{mode}");
        let body = response.text().await.unwrap();
        assert!(!body.contains("route-secret") && !body.contains("fixture provider detail"));
    }
    server.abort();
    assert_eq!(
        *records.lock().unwrap(),
        [
            "webhook-blocks",
            "webhook-reject",
            "webhook-redirect",
            "webhook-legacy"
        ]
    );

    let mut application = app(&root.0);
    application.boot().await.unwrap();
    let db = setup(&application).await;
    application
        .call(
            Callback::new(&[], move |_| {
                let base = base.clone();
                async move {
                    for connection in ["database", "redis"] {
                        let route = NotificationRoute::Slack(format!("{base}/route-secret").into());
                        Notification::route("slack", route)
                            .unwrap()
                            .notify(&SlackNotice {
                                mode: format!("webhook-legacy-{connection}"),
                                connection: Some(connection),
                            })
                            .await
                            .unwrap();
                    }
                    Ok(())
                }
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let Value::Text(payload) = db.table("jobs").value("payload").await.unwrap() else {
        panic!("expected a JSON queue payload");
    };
    let mut redis = ::redis::Client::open(format!("redis://127.0.0.1:{redis}"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let prefix = config["database"]["redis"]["options"]["prefix"]
        .as_str()
        .unwrap();
    let redis_payload: String = ::redis::cmd("LINDEX")
        .arg(format!("{prefix}queues:slack-test"))
        .arg(0)
        .query_async(&mut redis)
        .await
        .unwrap();
    for payload in [payload, redis_payload] {
        assert!(payload.contains("encrypted_webhook") && !payload.contains("route-secret"));
        assert!(payload.contains("attachments") && payload.contains("invoice_status"));
    }
    assert_eq!(records.lock().unwrap().len(), 4);

    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=slack-test",
            "--once",
            "--tries=2",
        ],
    )
    .await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Unsigned(1)
    );
    for connection in ["database", "redis"] {
        artisan(
            &root.0,
            &[
                "queue:work",
                connection,
                "--queue=slack-test",
                "--stop-when-empty",
                "--sleep=0",
                "--tries=2",
            ],
        )
        .await;
    }
    assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    drop_delivery_tables(&db).await;
    provider.abort();
    assert_eq!(
        records.lock().unwrap()[4..],
        [
            "webhook-legacy-database",
            "webhook-legacy-database",
            "webhook-legacy-redis"
        ]
    );
}

#[tokio::test]
async fn vonage_protocol_and_worker_retries_use_the_notification_route() {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicBool, Ordering},
    };

    let (Some(mysql), Ok(redis)) = (delivery_mysql(), std::env::var("OPENRUST_TEST_REDIS_PORT"))
    else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let records = Arc::new(Mutex::new(Vec::new()));
    let fail_once = Arc::new(AtomicBool::new(true));
    let endpoint = axum::Router::new().route(
        "/sms/{account}",
        axum::routing::post({
            let records = Arc::clone(&records);
            move |axum::extract::Path(account): axum::extract::Path<String>,
                  headers: http::HeaderMap,
                  body: axum::body::Bytes| {
                let records = Arc::clone(&records);
                let fail_once = Arc::clone(&fail_once);
                async move {
                    assert_eq!(
                        headers[http::header::CONTENT_TYPE],
                        "application/x-www-form-urlencoded"
                    );
                    let auth = headers[http::header::AUTHORIZATION]
                        .to_str()
                        .unwrap()
                        .to_owned();
                    let fields: HashMap<_, _> =
                        url::form_urlencoded::parse(&body).into_owned().collect();
                    let text = fields["text"].clone();
                    records.lock().unwrap().push((account, auth, fields));
                    if text == "redirect" {
                        return axum::response::Redirect::temporary("/sms/unexpected")
                            .into_response();
                    }
                    if text == "malformed" {
                        return (
                            [(http::header::CONTENT_TYPE, "application/json")],
                            r#"{"messages":[]}"#,
                        )
                            .into_response();
                    }
                    let response = if text == "database" && fail_once.swap(false, Ordering::Relaxed)
                    {
                        json!({"messages":[{"status":"1","error-text":"fixture throttled"}]})
                    } else if text == "reject" {
                        json!({"messages":[{"status":"29","error-text":"fixture provider detail"}]})
                    } else {
                        json!({"messages":[{"status":"0","message-id":"fixture-id"}]})
                    };
                    (
                        [(http::header::CONTENT_TYPE, "application/json")],
                        response.to_string(),
                    )
                        .into_response()
                }
            }
        }),
    );
    use axum::response::IntoResponse as _;
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", socket.local_addr().unwrap());
    let provider = tokio::spawn(async move { axum::serve(socket, endpoint).await });
    let root = fixture(Some(redis.parse().unwrap()), Some(&mysql), None);
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["vonage"] = json!({
        "api_key":"fixture-default","api_secret":"default-secret","sms_from":"447700900001",
        "endpoint":format!("{base}/sms/default"),
        "clients":{"partner":{
            "api_key":"fixture-partner","api_secret":"partner-secret",
            "endpoint":format!("{base}/sms/partner"),
        }},
    });
    std::fs::write(&config_path, config.to_string()).unwrap();
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "vonage").await;
    for (mode, status) in [
        ("plain", http::StatusCode::OK),
        ("unicode", http::StatusCode::OK),
        ("missing-route", http::StatusCode::OK),
        ("invalid", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("reject", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("malformed", http::StatusCode::INTERNAL_SERVER_ERROR),
        ("redirect", http::StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let response = reqwest::get(format!("{url}/{mode}")).await.unwrap();
        assert_eq!(response.status(), status, "{mode}");
        assert!(
            !response
                .text()
                .await
                .unwrap()
                .contains("fixture provider detail")
        );
    }
    server.abort();
    assert_eq!(records.lock().unwrap().len(), 5);
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    let db = setup(&application).await;
    db.getSchemaBuilder()
        .create("sms_deliveries", |table| {
            table.string("id", 36).primary();
            table.string("status", 16);
            table.text("submission").nullable();
            table.bigInteger("created_at");
            table.bigInteger("updated_at");
        })
        .await
        .unwrap();

    application
        .call(
            Callback::new(&[], |_| async {
                for connection in ["database", "redis"] {
                    Notification::route("vonage", NotificationRoute::Vonage("447700900000".into()))
                        .unwrap()
                        .notify(&SmsNotice {
                            mode: connection.into(),
                            connection: Some(connection),
                        })
                        .await
                        .unwrap();
                }
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let payload = db.table("jobs").value("payload").await.unwrap();
    let Value::Text(payload) = payload else {
        panic!("expected a JSON queue payload")
    };
    assert!(payload.contains("447700900000") && !payload.contains("default-secret"));
    assert_eq!(records.lock().unwrap().len(), 5);
    config["vonage"]["api_secret"] = json!("rotated-secret");
    std::fs::write(&config_path, config.to_string()).unwrap();
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=vonage-test",
            "--once",
            "--tries=2",
        ],
    )
    .await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Unsigned(1)
    );
    assert_eq!(records.lock().unwrap().len(), 6);
    for connection in ["database", "redis"] {
        artisan(
            &root.0,
            &[
                "queue:work",
                connection,
                "--queue=vonage-test",
                "--stop-when-empty",
                "--sleep=0",
                "--tries=2",
            ],
        )
        .await;
    }
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    drop_delivery_tables(&db).await;
    provider.abort();

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 8);
    assert!(
        records
            .iter()
            .all(|(_, _, fields)| fields["to"] == "447700900000")
    );
    assert!(
        records
            .iter()
            .all(|(account, _, _)| account != "unexpected")
    );
    let (account, authorization, fields) = &records[0];
    assert_eq!(account, "default");
    assert_eq!(
        authorization,
        &format!(
            "Basic {}",
            STANDARD.encode("fixture-default:default-secret")
        )
    );
    assert_eq!(fields["from"], "447700900001");
    assert_eq!(fields["type"], "text");
    let (account, authorization, fields) = &records[1];
    assert_eq!(account, "partner");
    assert_eq!(
        authorization,
        &format!(
            "Basic {}",
            STANDARD.encode("fixture-partner:partner-secret")
        )
    );
    assert_eq!(fields["from"], "Invoices");
    assert_eq!(fields["type"], "unicode");
    assert_eq!(fields["text"], "已付款");
    assert_eq!(fields["client-ref"], "invoice-42");
    assert_eq!(fields["callback"], "https://example.test/sms-status");
    let rotated = format!(
        "Basic {}",
        STANDARD.encode("fixture-default:rotated-secret")
    );
    assert!(
        records[5..]
            .iter()
            .all(|(_, authorization, _)| authorization == &rotated)
    );
    assert_eq!(
        records
            .iter()
            .filter(|(_, _, fields)| fields["text"] == "database")
            .count(),
        2
    );
    assert_eq!(
        records
            .iter()
            .filter(|(_, _, fields)| fields["text"] == "redis")
            .count(),
        1
    );
}

#[tokio::test]
async fn notification_envelope_and_attachment_sources_survive_queue_delivery() {
    let (Some(mysql), Ok(redis), Ok(smtp), Ok(mailpit)) = (
        delivery_mysql(),
        std::env::var("OPENRUST_TEST_REDIS_PORT"),
        std::env::var("OPENRUST_TEST_SMTP_PORT"),
        std::env::var("OPENRUST_TEST_SMTP_HTTP_PORT"),
    ) else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let root = fixture(
        Some(redis.parse().unwrap()),
        Some(&mysql),
        Some(smtp.parse().unwrap()),
    );
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    let source = root.0.join("notice.txt");
    let marker = root.0.join("customizer-ready");
    std::fs::write(&marker, b"ok").unwrap();
    std::fs::write(&source, b"notice file").unwrap();
    config["mail"]["default"] = json!("array");
    config["fixture"] = json!({"notice_path":source,"mail_customizer_marker":marker});
    config["filesystems"] = json!({"default":"reports","disks":{
        "reports":{"driver":"local","root":root.0.join("reports")},
        "archive":{"driver":"local","root":root.0.join("archive")}
    }});
    std::fs::write(&config_path, config.to_string()).unwrap();
    for folder in ["reports", "archive"] {
        let disk = root.0.join(folder);
        std::fs::create_dir(&disk).unwrap();
        std::fs::write(disk.join("report.txt"), folder).unwrap();
    }
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "notification-envelope").await;
    assert_eq!(
        reqwest::get(&url).await.unwrap().status(),
        http::StatusCode::OK
    );
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    let db = setup(&application).await;
    std::fs::remove_file(&source).unwrap();
    assert_eq!(
        reqwest::get(&url).await.unwrap().status(),
        http::StatusCode::INTERNAL_SERVER_ERROR
    );
    application
        .call(
            Callback::new(&[], |_| async {
                for connection in ["database", "redis"] {
                    Notification::route(
                        "mail",
                        NotificationRoute::Mail(vec![Address {
                            address: "reader@example.test".into(),
                            name: Some("Notice Reader".into()),
                        }]),
                    )
                    .unwrap()
                    .notify(&DeliveryNotice(Some(connection)))
                    .await
                    .unwrap();
                }
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    server.abort();
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=notification-envelope",
            "--once",
            "--tries=2",
        ],
    )
    .await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Unsigned(1)
    );
    std::fs::write(&source, b"late notice file").unwrap();
    for connection in ["database", "redis"] {
        artisan(
            &root.0,
            &[
                "queue:work",
                connection,
                "--queue=notification-envelope",
                "--stop-when-empty",
                "--sleep=0",
                "--tries=2",
            ],
        )
        .await;
    }
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    let api = format!("http://127.0.0.1:{mailpit}/api/v1");
    let listing: Json = reqwest::get(format!("{api}/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut delivered = 0;
    for row in listing["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["Subject"]
                .as_str()
                .is_some_and(|subject| subject.starts_with("Notification envelope "))
        })
    {
        let id = row["ID"].as_str().unwrap();
        let raw = reqwest::get(format!("{api}/message/{id}/raw"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        for fragment in [
            "From: \"Finance Team\" <finance@example.test>",
            "To: \"Notice Reader\" <reader@example.test>",
            "Cc: team@example.test",
            "Bcc: audit@example.test",
            "Reply-To: support@example.test",
            "X-Tag: payment",
            "X-Metadata-case: notification",
            "X-Custom-Marker: notification-marked",
            "notice.txt",
            "memory.txt",
            "default-report.txt",
            "archive-report.txt",
            "customized.txt",
            "customized body",
            "Regards,\r\nOpenRust",
        ] {
            assert!(raw.contains(fragment), "{fragment}: {raw}");
        }
        if row["Subject"] != "Notification envelope http" {
            use base64::{Engine as _, engine::general_purpose::STANDARD};
            assert!(
                raw.contains(&STANDARD.encode(b"late notice file"))
                    || raw.contains("late notice file"),
                "{raw}"
            );
        }
        let priority = match row["Subject"].as_str().unwrap() {
            "Notification envelope http" => "1 (Highest)",
            "Notification envelope redis" => "5 (Lowest)",
            _ => "3 (Normal)",
        };
        assert!(raw.contains(&format!("X-Priority: {priority}")), "{raw}");
        delivered += 1;
    }
    assert_eq!(delivered, 3);
    drop_delivery_tables(&db).await;
}

#[tokio::test]
async fn attachment_sources_resolve_over_http_and_in_artisan_workers() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    for invalid in [
        "file:///private/report",
        "ftp://example.test/a",
        "/files/report",
        "http:example.test/report",
        "https:///report",
    ] {
        assert!(Attachment::fromUrl(invalid).is_err());
    }

    let (Some(mysql), Ok(redis), Ok(smtp), Ok(mailpit)) = (
        delivery_mysql(),
        std::env::var("OPENRUST_TEST_REDIS_PORT"),
        std::env::var("OPENRUST_TEST_SMTP_PORT"),
        std::env::var("OPENRUST_TEST_SMTP_HTTP_PORT"),
    ) else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_DELIVERY_SERVICES").is_none());
        return;
    };
    let root = fixture(
        Some(redis.parse().unwrap()),
        Some(&mysql),
        Some(smtp.parse().unwrap()),
    );
    let object_store = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", object_store.local_addr().unwrap());
    let available = Arc::new(AtomicBool::new(true));
    let downloads = Arc::new(AtomicUsize::new(0));
    let objects = axum::Router::new()
        .route(
            "/attachments/tenant/nested/object.bin",
            axum::routing::get(|| async {
                (
                    [(http::header::CONTENT_TYPE, "application/x-object-fixture")],
                    vec![0_u8, 91, 255],
                )
            }),
        )
        .route(
            "/reports/{name}",
            axum::routing::get(|| async { axum::response::Redirect::temporary("/download") }),
        )
        .route(
            "/download",
            axum::routing::get({
                let available = Arc::clone(&available);
                let downloads = Arc::clone(&downloads);
                move || {
                    let available = Arc::clone(&available);
                    let downloads = Arc::clone(&downloads);
                    async move {
                        if !available.load(Ordering::Relaxed) {
                            return Err(http::StatusCode::SERVICE_UNAVAILABLE);
                        }
                        downloads.fetch_add(1, Ordering::Relaxed);
                        Ok((
                            [(http::header::CONTENT_TYPE, "application/x-report-fixture")],
                            vec![0_u8, 92, 255],
                        ))
                    }
                }
            }),
        );
    let object_server = tokio::spawn(async move { axum::serve(object_store, objects).await });
    let path = root.0.join("source.txt");
    let marker = root.0.join("customization-enabled");
    std::fs::write(&marker, b"enabled").unwrap();
    let config_path = root.0.join("config.json");
    let mut config: Json = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["fixture"]["attachment_file"] = json!(path);
    config["fixture"]["mail_customizer_marker"] = json!(marker);
    config["fixture"]["attachment_url"] = json!(format!(
        "{endpoint}/reports/invoice%2042.bin?signature=fixture"
    ));
    config["filesystems"] = json!({
        "default":"local",
        "cloud":"objects",
        "disks": {
            "local":{"driver":"local","root":"local","throw":false},
            "archive":{"driver":"local","root":"archive","throw":true},
            "objects":{"driver":"s3","bucket":"attachments","region":"us-east-1",
                "key":"fixture-key","secret":"fixture-secret","endpoint":endpoint,
                "use_path_style_endpoint":true,"root":"tenant","throw":true}
        }
    });
    std::fs::write(config_path, config.to_string()).unwrap();
    let disk_path = root.0.join("local/exports/default.csv");
    let archive_path = root.0.join("archive/data.bin");
    for file in [&disk_path, &archive_path] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    }
    std::fs::write(&path, [0, 15, 255]).unwrap();
    std::fs::write(&disk_path, [0, 16, 255]).unwrap();
    std::fs::write(&archive_path, [0, 17, 255]).unwrap();

    let mut application = app(&root.0);
    application.boot().await.unwrap();
    let db = setup(&application).await;
    let mut kernel = Kernel::new(app(&root.0));
    kernel.bootstrap().await.unwrap();
    let (url, server) = serve_fixture_http(kernel, "attachment-mail").await;
    let upload_url = url.replace("/attachment-mail", "/attachment-upload");
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    let binary = [0, 31, 255];
    let client = reqwest::Client::new();
    for (name, mime, bytes, url) in [
        (
            "folder/upload.png",
            "text/plain",
            png.as_slice(),
            upload_url.clone(),
        ),
        (
            "unknown.bin",
            "application/x-client-fixture",
            binary.as_slice(),
            format!("{upload_url}?queued=1"),
        ),
    ] {
        let mut body = format!(
            "--fixture\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n"
        ).into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n--fixture--\r\n");
        client
            .post(url)
            .header(
                http::header::CONTENT_TYPE,
                "multipart/form-data; boundary=fixture",
            )
            .body(body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    reqwest::get(&url)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(downloads.load(Ordering::Relaxed), 1);
    std::fs::remove_file(&marker).unwrap();
    assert_eq!(
        reqwest::get(&url).await.unwrap().status(),
        http::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(downloads.load(Ordering::Relaxed), 1);
    std::fs::write(&marker, b"enabled").unwrap();
    available.store(false, Ordering::Relaxed);
    assert_eq!(
        reqwest::get(&url).await.unwrap().status(),
        http::StatusCode::INTERNAL_SERVER_ERROR
    );
    std::fs::remove_file(&disk_path).unwrap();
    assert_eq!(
        reqwest::get(&url).await.unwrap().status(),
        http::StatusCode::INTERNAL_SERVER_ERROR
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&marker).unwrap();
    application
        .call(
            Callback::new(&[], |_| {
                let path = path.clone();
                async move {
                    let receipt = AttachmentReceipt {
                        path: path.clone(),
                        subject: "Source attachments worker",
                    };
                    receipt
                        .assertHasAttachment(&Attachment::fromPath(&path))
                        .assertHasAttachment(
                            &RemoteReport(Config::string("fixture.attachment_url").unwrap())
                                .toMailAttachment()
                                .unwrap(),
                        )
                        .assertHasAttachedData(b"file receipt", "receipt.txt", Some("text/plain"))
                        .assertHasAttachedData(&[0, 255, 1], "generated.bin", None)
                        .assertHasAttachmentFromStorage(
                            "exports/default.csv",
                            Some("default.csv"),
                            None,
                        )
                        .assertHasAttachmentFromStorageDisk(
                            "objects",
                            "nested/object.bin",
                            None,
                            None,
                        )
                        .assertHasAttachmentFromStorageDisk(
                            "archive",
                            "data.bin",
                            Some("archive-record.bin"),
                            Some("application/x-openrust"),
                        );
                    assert!(
                        !receipt
                            .hasAttachment(&Attachment::fromStorage("nested/object.bin"))
                            .unwrap()
                    );
                    assert!(
                        !receipt
                            .hasAttachment(&Attachment::fromStorageDisk("archive", "data.bin"))
                            .unwrap()
                    );
                    AddressedReceipt.assertHasNoAttachments();
                    for connection in ["database", "redis"] {
                        Mail::to("to@example.test")
                            .unwrap()
                            .queue(
                                &receipt,
                                DispatchOptions {
                                    connection: Some(connection.into()),
                                    queue: Some("attachment-sources".into()),
                                    ..Default::default()
                                },
                                None,
                            )
                            .await
                            .unwrap()
                            .expect("enqueued without reading files");
                    }
                    Ok(())
                }
            }),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(downloads.load(Ordering::Relaxed), 1);
    std::fs::write(&path, [0, 25, 255]).unwrap();
    std::fs::write(&disk_path, [0, 26, 255]).unwrap();
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=attachment-sources",
            "--once",
            "--tries=3",
        ],
    )
    .await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Unsigned(1)
    );
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    std::fs::write(&marker, b"enabled").unwrap();
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=attachment-sources",
            "--once",
            "--tries=3",
        ],
    )
    .await;
    assert_eq!(
        db.table("jobs").value("attempts").await.unwrap(),
        Value::Unsigned(2)
    );
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    available.store(true, Ordering::Relaxed);
    for connection in ["database", "redis"] {
        artisan(
            &root.0,
            &[
                "queue:work",
                connection,
                "--queue=attachment-sources",
                "--stop-when-empty",
                "--sleep=0",
                "--tries=3",
            ],
        )
        .await;
    }
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);
    assert_eq!(downloads.load(Ordering::Relaxed), 3);
    let api = format!("http://127.0.0.1:{mailpit}/api/v1");
    let listing: Json = reqwest::get(format!("{api}/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut delivered = (0, 0);
    let mut uploaded = Vec::new();
    for row in listing["messages"].as_array().unwrap() {
        let subject = row["Subject"].as_str().unwrap();
        match subject {
            "Source attachments HTTP"
            | "Source attachments worker"
            | "Uploaded attachment HTTP" => {}
            _ => continue,
        }
        let id = row["ID"].as_str().unwrap();
        let raw = reqwest::get(format!("{api}/message/{id}/raw"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        if subject == "Uploaded attachment HTTP" {
            if raw.contains("upload.png") {
                assert!(
                    raw.contains("image/png") && raw.contains(&STANDARD.encode(png)),
                    "{raw}"
                );
                assert!(!raw.contains("folder/upload.png"));
                uploaded.push("png");
            } else {
                assert!(
                    raw.contains("unknown.bin")
                        && raw.contains("application/x-client-fixture")
                        && raw.contains(&STANDARD.encode(binary)),
                    "{raw}"
                );
                uploaded.push("fallback");
            }
            continue;
        }
        let (file, disk) = if subject == "Source attachments HTTP" {
            delivered.0 += 1;
            (15, 16)
        } else {
            delivered.1 += 1;
            (25, 26)
        };
        assert!(
            raw.contains("X-Tag: receipt\r\n")
                && raw.contains("X-Tag: shipment\r\n")
                && raw.contains("X-Metadata-order-id: R123\r\n"),
            "{raw}"
        );
        assert!(
            raw.contains("X-Custom-Marker: tracking-123")
                && raw.contains("customized body")
                && raw.contains("customized.txt"),
            "{raw}"
        );
        for marker in [
            "source.txt",
            "default.csv",
            "archive-record.bin",
            "object.bin",
            "generated.bin",
            "receipt.txt",
            "application/x-openrust",
            "application/x-object-fixture",
            "application/octet-stream",
            "invoice 42.bin",
            "application/x-report-fixture",
        ] {
            assert!(raw.contains(marker), "missing {marker}: {raw}");
        }
        for bytes in [
            [0, file, 255],
            [0, disk, 255],
            [0, 17, 255],
            [0, 91, 255],
            [0, 92, 255],
            [0, 255, 1],
        ] {
            assert!(raw.contains(&STANDARD.encode(bytes)), "{raw}");
        }
    }
    assert_eq!(delivered, (1, 2));
    uploaded.sort_unstable();
    assert_eq!(uploaded, ["fallback", "png"]);
    server.abort();
    object_server.abort();
    drop_delivery_tables(&db).await;
}

#[tokio::test]
async fn mail_customizers_are_application_local_and_preview_is_read_only() {
    let other = fixture(None, None, None);
    let mut first = app(&other.0);
    first.boot().await.unwrap();
    let manager = first
        .container()
        .make::<openrust::mail::MailManager>()
        .unwrap();
    manager
        .registerMessageCustomizer("fixture.marker", |_, _| Ok(()))
        .unwrap();
    assert!(
        manager
            .registerMessageCustomizer("fixture.marker", |_, _| Ok(()))
            .is_err()
    );
    let root = fixture(None, None, None);
    let mut application = app(&root.0);
    application.boot().await.unwrap();
    application
        .call(
            Callback::new(&[], |_| async {
                let receipt = AttachmentReceipt {
                    path: PathBuf::new(),
                    subject: "Custom mail",
                };
                assert!(receipt.render().await.unwrap().contains("Receipt for"));
                let result = Mail::to("to@example.test").unwrap().sendNow(&receipt).await;
                assert!(
                    result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("fixture.marker] is not registered")
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
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

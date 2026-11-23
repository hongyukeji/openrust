#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::contracts::events::{Dispatcher as _, Event as PayloadEvent, Listener};
use openrust::{
    auth::{
        AuthError, AuthManager, Credentials,
        events::{PasswordReset, Registered},
        notifications::{ResetPassword, VerifyEmail},
        passwords::PasswordResetStatus,
    },
    contracts::{
        auth::{Authenticatable, MustVerifyEmail},
        container::Container as _,
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{DatabaseManager, Value, eloquent::Model},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        Application, auth::EmailVerificationRequest, configuration::Routing, http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    mail::{
        MailResult, Mailable,
        mailables::{Content, Envelope},
    },
    notifications::messages::{MailDelivery, MailMessage},
    routing::{RouteParameter, Router, RoutingError, SignatureMode},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Auth, Event, Hash, Mail, Password, URL},
    },
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
};

#[path = "../app/models/user.rs"]
mod user;
use user::User;

#[derive(Clone, Copy)]
enum MailCustomization {
    Default,
    Urls,
    Messages,
}

struct Provider(MailCustomization);
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<AuthManager>()?
                .registerModel::<User>("fixture.user");
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move { configure_auth_mail(self.0) })
    }
}

struct VerificationMail {
    email: String,
    url: String,
}

impl Mailable for VerificationMail {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope {
            subject: "Custom verification mail".into(),
            to: vec![self.email.as_str().into()],
            ..Default::default()
        })
    }

    fn content(&self) -> MailResult<Content> {
        Ok(Content {
            text: Some(format!("Verify the account\n\n{}", self.url)),
            html: Some(format!("<p>Verify the account</p><p>{}</p>", self.url)),
            ..Default::default()
        })
    }
}

fn configure_auth_mail(customization: MailCustomization) -> ProviderResult {
    match customization {
        MailCustomization::Default => {}
        MailCustomization::Urls => {
            ResetPassword::createUrlUsing(|user, token| {
                assert_eq!(user.getEmailForPasswordReset(), "first@example.test");
                URL::route(
                    "password.reset",
                    &[
                        RouteParameter::named("token", token),
                        RouteParameter::named("email", user.getEmailForPasswordReset()),
                        RouteParameter::named("source", "custom"),
                    ],
                    true,
                )
                .map_err(backend)
            })?;
        }
        MailCustomization::Messages => {
            ResetPassword::createUrlUsing(|_, _| {
                Err(AuthError::Configuration("unexpected reset URL hook"))
            })?;
            ResetPassword::toMailUsing(|user, token| {
                assert_eq!(user.getEmailForPasswordReset(), "first@example.test");
                let url = ResetPassword::new(token).url(user.getEmailForPasswordReset())?;
                Ok(MailDelivery::Message(Box::new(
                    MailMessage::new()
                        .subject("Custom reset mail")
                        .action("Go to reset", url),
                )))
            })?;
            VerifyEmail::createUrlUsing(|user| {
                assert_eq!(user.getEmailForVerification(), "first@example.test");
                URL::temporarySignedRoute(
                    "verification.verify",
                    std::time::SystemTime::now() + Duration::from_secs(120),
                    &[
                        RouteParameter::named("id", "1"),
                        RouteParameter::named("hash", "cba463d2361edd11709c80a7e059e831da9efe8d"),
                        RouteParameter::named("source", "custom"),
                    ],
                    SignatureMode::Absolute,
                )
                .map_err(backend)
            })?;
            VerifyEmail::toMailUsing(|user, url| {
                assert_eq!(user.getEmailForVerification(), "first@example.test");
                assert!(url.contains("source=custom"));
                Ok(MailDelivery::Mailable(Box::new(VerificationMail {
                    email: user.getEmailForVerification().into(),
                    url: url.into(),
                })))
            })?;
        }
    }
    Ok(())
}

fn empty(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn credentials(email: &str) -> Credentials {
    Credentials::new([("email", Value::from(email))], "")
}

fn text_query(request: &Request, name: &str) -> Result<String, HttpError> {
    request
        .query(Some(name))?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(HttpError::InvalidRequest)
}

fn send_link(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let status =
            Password::sendResetLink(&credentials(&text_query(request, "email")?), None).await?;
        Response::json(StatusCode::OK, status.as_str())
    })
}

fn reset(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let token = request.route("token").ok_or(HttpError::InvalidRequest)?;
        let status = Password::reset(
            &credentials(&text_query(request, "email")?),
            token,
            b"new password",
            &|user, password| {
                Box::pin(async move {
                    let model = (user as &mut dyn std::any::Any)
                        .downcast_mut::<User>()
                        .ok_or(AuthError::InvalidIdentity)?;
                    let hash = Hash::make(password, &Default::default())
                        .await
                        .map_err(backend)?;
                    model.setAttribute("password", hash);
                    model.setRememberToken(Some("rotated".into()));
                    model.save().await.map_err(backend)?;
                    Event::dispatch(PasswordReset {
                        user: model.getAuthIdentifier(),
                    })
                    .await
                    .map_err(backend)?;
                    Ok(())
                })
            },
        )
        .await?;
        let code = if status == PasswordResetStatus::PasswordReset {
            StatusCode::OK
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        Response::json(code, status.as_str())
    })
}

fn send_verification(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let capability = user
            .as_email_verification()
            .ok_or(AuthError::InvalidIdentity)?;
        if !capability.hasVerifiedEmail() {
            if request.uri().path() == "/register-notification" {
                Event::dispatch(Registered { user })
                    .await
                    .map_err(backend)?;
            } else {
                capability.sendEmailVerificationNotification().await?;
            }
        }
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn verify(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let authenticated = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let mut user = User::find(Value::from(authenticated.getAuthIdentifier()))
            .await
            .map_err(backend)?
            .ok_or(HttpError::Unauthenticated)?;
        EmailVerificationRequest::fulfill(request, &mut user).await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn backend(error: impl std::error::Error + Send + Sync + 'static) -> AuthError {
    AuthError::Backend(Box::new(error))
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut routes = Router::new();
    routes.post("/forgot-password", send_link)?;
    routes
        .get("/reset-password/{token}", empty)?
        .name("password.reset");
    routes.post("/reset-password/{token}", reset)?;
    routes
        .get("/verify-email", empty)?
        .name("verification.notice");
    routes
        .get("/verify-email/{id}/{hash}", verify)?
        .name("verification.verify")
        .middleware_name("auth")
        .middleware_name("signed");
    routes
        .post("/register-notification", send_verification)?
        .middleware_name("auth");
    routes
        .post("/email/verification-notification", send_verification)?
        .name("verification.send")
        .middleware_name("auth")
        .middleware_name("throttle:6,1");
    routes
        .get("/protected", empty)?
        .middleware_name("auth")
        .middleware_name("verified");
    Ok(routes)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    json: bool,
) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let authorization = token.map_or_else(String::new, |token| format!("Authorization: Bearer {token}\r\n"));
        let accept = if json { "Accept: application/json\r\n" } else { "" };
        let message = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{authorization}{accept}Content-Length: 0\r\nConnection: close\r\n\r\n");
        stream.write_all(message.as_bytes()).await.unwrap();
        let mut result = String::new();
        stream.read_to_string(&mut result).await.unwrap();
        result
    }).await.unwrap()
}

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture_config(
    base: &Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, Json>, openrust::config::ConfigError> {
    Ok(serde_json::from_slice(&std::fs::read(base.join("fixture.json")).unwrap()).unwrap())
}

async fn start(
    mailpit: Option<(u16, u16)>,
    customization: MailCustomization,
) -> (Root, SocketAddr, Arc<Kernel>, tokio::task::JoinHandle<()>) {
    start_with_database(
        mailpit,
        customization,
        json!({"driver":"sqlite","database":"fixture.sqlite"}),
    )
    .await
}

async fn start_with_database(
    mailpit: Option<(u16, u16)>,
    customization: MailCustomization,
    database_config: Json,
) -> (Root, SocketAddr, Arc<Kernel>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let root = Root(std::env::temp_dir().join(format!(
        "openrust-auth-flow-{}-{}",
        std::process::id(),
        addr.port()
    )));
    std::fs::create_dir(&root.0).unwrap();
    std::fs::File::create(root.0.join("fixture.sqlite")).unwrap();
    let config = json!({
        "app":{"key":"base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","url":format!("http://{addr}")},
        "cache":{"default":"array","stores":{"array":{"driver":"array"}}},
        "hashing":{"driver":"bcrypt","bcrypt":{"rounds":4}},
        "database":{"default":"fixture","connections":{"fixture":database_config}},
        "auth":{"defaults":{"guard":"api","passwords":"users"},"guards":{"api":{"driver":"token","provider":"users"}},
                "providers":{"users":{"driver":"eloquent","model":"fixture.user"}},
                "passwords":{"users":{"provider":"users","table":"password_reset_tokens","expire":37,"throttle":60}}},
        "mail":{"default":if mailpit.is_some() {"smtp"} else {"array"}, "from":{"address":"auth@example.test"},
                "mailers":{"array":{"transport":"array"},"smtp":{"transport":"smtp","host":"127.0.0.1","port":mailpit.map_or(1, |(port, _)| port)}}}
    });
    std::fs::write(root.0.join("fixture.json"), config.to_string()).unwrap();
    let mut app = Application::configure(root.0.clone())
        .with_config(fixture_config)
        .with_providers(vec![Box::new(Provider(customization))])
        .with_routing(Routing::new(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let database = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    database
        .getSchemaBuilder()
        .create("users", |table| {
            table.id();
            table.string("email", 255).unique();
            table.string("password", 255);
            table.string("api_token", 255);
            table.string("remember_token", 100).nullable();
            table.timestamp("email_verified_at", 0).nullable();
            table.timestamps();
        })
        .await
        .unwrap();
    database
        .getSchemaBuilder()
        .create("password_reset_tokens", |table| {
            table.string("email", 255).primary();
            table.string("token", 255);
            table.timestamp("created_at", 0).nullable();
        })
        .await
        .unwrap();
    for (email, token) in [
        ("first@example.test", "first"),
        ("second@example.test", "second"),
    ] {
        database
            .table("users")
            .insert(&[[
                ("email".into(), email.into()),
                ("password".into(), "unused".into()),
                ("api_token".into(), token.into()),
            ]
            .into()])
            .await
            .unwrap();
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    let server = kernel.clone();
    let process = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(server).into_make_service())
            .await
            .unwrap();
    });
    (root, addr, kernel, process)
}

async fn mail_body(kernel: &Kernel, mailpit: Option<(u16, u16)>, subject: &str) -> String {
    if let Some((_, port)) = mailpit {
        let list = mailpit_json(port, "messages").await;
        let id = list["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["Subject"] == subject)
            .unwrap()["ID"]
            .as_str()
            .unwrap();
        let message = mailpit_json(port, &format!("message/{id}")).await;
        assert!(!message["HTML"].as_str().unwrap().is_empty());
        return message["Text"].as_str().unwrap().into();
    }
    let application = kernel.get_application();
    application
        .call(
            openrust::container::Callback::new(&[], |_| async {
                let messages = Mail::mailer(None).unwrap().sent().unwrap();
                let mime = messages
                    .iter()
                    .rev()
                    .map(|sent| String::from_utf8(sent.message.formatted()).unwrap())
                    .find(|mime| mime.contains(subject))
                    .unwrap();
                assert!(mime.contains("Content-Type: text/html"));
                Ok(mime.replace("=\r\n", "").replace("=3D", "="))
            }),
            Default::default(),
        )
        .await
        .unwrap()
}

fn extract_url(text: &str) -> String {
    let url = text
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '[' | ']'))
        .find(|item| item.starts_with("http://") && item.contains('?'))
        .unwrap();
    url.replace("&amp;", "&")
}

async fn mailpit_json(port: u16, path: &str) -> Json {
    let output = tokio::process::Command::new("curl")
        .args(["--fail", "--silent", "--max-time", "5"])
        .arg(format!("http://127.0.0.1:{port}/api/v1/{path}"))
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn target(addr: SocketAddr, url: &str) -> &str {
    url.strip_prefix(&format!("http://{addr}")).unwrap()
}

struct VerifyEvents(Arc<AtomicUsize>);

impl Listener for VerifyEvents {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            assert!(
                event
                    .downcast_ref::<openrust::auth::events::Verified>()
                    .is_some()
            );
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        })
    }
}

async fn exercise(mailpit: Option<(u16, u16)>) {
    let (_root, addr, kernel, server) = start(mailpit, MailCustomization::Default).await;
    let application = kernel.get_application();
    let events = Arc::new(AtomicUsize::new(0));
    application
        .container()
        .make::<Dispatcher>()
        .unwrap()
        .listen(
            "Verified",
            Arc::new(VerifyEvents(events.clone())),
            Delivery::Sync,
        )
        .unwrap();
    let send_path = "/forgot-password?email=first%40example.test";
    let absent = request(
        addr,
        "POST",
        "/forgot-password?email=unknown%40example.test",
        None,
        true,
    )
    .await;
    assert!(absent.contains("passwords.user"));
    assert!(
        request(addr, "POST", send_path, None, true)
            .await
            .contains("passwords.sent")
    );
    assert!(
        request(addr, "POST", send_path, None, true)
            .await
            .contains("passwords.throttled")
    );
    let reset_mail = mail_body(&kernel, mailpit, "Reset your password").await;
    assert!(reset_mail.contains("Reset Password"));
    assert!(reset_mail.contains("This password reset link will expire in 37 minutes."));
    let reset = extract_url(&reset_mail);
    let reset = target(addr, &reset);
    let (first, second) = tokio::join!(
        request(addr, "POST", reset, None, true),
        request(addr, "POST", reset, None, true)
    );
    assert_ne!(
        first.contains("passwords.reset"),
        second.contains("passwords.reset")
    );
    assert_ne!(
        first.contains("passwords.token"),
        second.contains("passwords.token")
    );

    let denied = request(addr, "GET", "/protected", Some("first"), true).await;
    assert!(denied.starts_with("HTTP/1.1 403 "), "{denied}");
    let redirect = request(addr, "GET", "/protected", Some("first"), false).await;
    assert!(
        redirect.starts_with("HTTP/1.1 302 ") && redirect.contains("/verify-email"),
        "{redirect}"
    );
    assert!(
        request(addr, "POST", "/register-notification", Some("first"), true)
            .await
            .starts_with("HTTP/1.1 204 ")
    );
    let verification_mail = mail_body(&kernel, mailpit, "Verify your email address").await;
    assert!(verification_mail.contains("Verify Email Address"));
    assert!(!verification_mail.contains("password reset link"));
    let signed = extract_url(&verification_mail);
    let path = target(addr, &signed);
    let hash = path
        .split('?')
        .next()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let expired = application
        .call(
            openrust::container::Callback::new(&[], |_| async move {
                Ok(URL::temporarySignedRoute(
                    "verification.verify",
                    std::time::SystemTime::now() - Duration::from_secs(120),
                    &[
                        RouteParameter::named("id", "1"),
                        RouteParameter::named("hash", hash),
                    ],
                    SignatureMode::Absolute,
                )
                .unwrap())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    assert!(
        request(addr, "GET", target(addr, &expired), Some("first"), true)
            .await
            .starts_with("HTTP/1.1 403 ")
    );

    assert!(
        request(addr, "GET", path, Some("second"), true)
            .await
            .starts_with("HTTP/1.1 403 ")
    );
    assert!(
        request(
            addr,
            "GET",
            &format!("{path}&tampered=1"),
            Some("first"),
            true
        )
        .await
        .starts_with("HTTP/1.1 403 ")
    );
    assert!(
        request(addr, "GET", path, Some("first"), true)
            .await
            .starts_with("HTTP/1.1 204 ")
    );
    assert!(
        request(addr, "GET", path, Some("first"), true)
            .await
            .starts_with("HTTP/1.1 204 ")
    );
    assert!(
        request(addr, "GET", "/protected", Some("first"), true)
            .await
            .starts_with("HTTP/1.1 204 ")
    );
    assert_eq!(events.load(Ordering::SeqCst), 1);
    for _ in 0..6 {
        assert!(
            request(
                addr,
                "POST",
                "/email/verification-notification",
                Some("first"),
                true
            )
            .await
            .starts_with("HTTP/1.1 204 ")
        );
    }
    assert!(
        request(
            addr,
            "POST",
            "/email/verification-notification",
            Some("first"),
            true
        )
        .await
        .starts_with("HTTP/1.1 429 ")
    );
    server.abort();
}

async fn exercise_custom_mail(mailpit: Option<(u16, u16)>) {
    let (_url_root, url_addr, url_kernel, url_server) =
        start(mailpit, MailCustomization::Urls).await;
    let (_mail_root, mail_addr, mail_kernel, mail_server) =
        start(mailpit, MailCustomization::Messages).await;
    let (_default_root, default_addr, default_kernel, default_server) =
        start(None, MailCustomization::Default).await;
    let forgot = "/forgot-password?email=first%40example.test";
    for addr in [url_addr, mail_addr, default_addr] {
        assert!(
            request(addr, "POST", forgot, None, true)
                .await
                .contains("passwords.sent")
        );
    }

    let reset = extract_url(&mail_body(&url_kernel, mailpit, "Reset your password").await);
    let reset = target(url_addr, &reset);
    assert!(reset.contains("source=custom"));
    assert!(
        request(url_addr, "POST", reset, None, true)
            .await
            .contains("passwords.reset")
    );
    let reset = extract_url(&mail_body(&mail_kernel, mailpit, "Custom reset mail").await);
    let reset = target(mail_addr, &reset);
    assert!(
        request(mail_addr, "POST", reset, None, true)
            .await
            .contains("passwords.reset")
    );
    let reset = extract_url(&mail_body(&default_kernel, None, "Reset your password").await);
    assert!(!target(default_addr, &reset).contains("source="));

    for addr in [mail_addr, default_addr] {
        assert!(
            request(addr, "POST", "/register-notification", Some("first"), true)
                .await
                .starts_with("HTTP/1.1 204 ")
        );
    }
    let verified = extract_url(&mail_body(&mail_kernel, mailpit, "Custom verification mail").await);
    let verified = target(mail_addr, &verified);
    assert!(verified.contains("source=custom"));
    assert!(
        request(mail_addr, "GET", verified, Some("first"), true)
            .await
            .starts_with("HTTP/1.1 204 ")
    );
    let default = extract_url(&mail_body(&default_kernel, None, "Verify your email address").await);
    assert!(!target(default_addr, &default).contains("source="));
    for server in [url_server, mail_server, default_server] {
        server.abort();
    }
}

async fn exercise_email_change(database: Json) {
    let (_root, addr, kernel, server) =
        start_with_database(None, MailCustomization::Default, database).await;
    let application = kernel.get_application();
    let events = Arc::new(AtomicUsize::new(0));
    application
        .container()
        .make::<Dispatcher>()
        .unwrap()
        .listen(
            "Verified",
            Arc::new(VerifyEvents(events.clone())),
            Delivery::Sync,
        )
        .unwrap();
    let send = "/email/verification-notification";
    let before = request(addr, "POST", send, Some("first"), true).await;
    assert!(before.starts_with("HTTP/1.1 204 "), "{before}");
    let old = extract_url(&mail_body(&kernel, None, "Verify your email address").await);

    // Keep the model a verification request could have loaded before the address was edited.
    application
        .call(
            openrust::container::Callback::new(&[], |_| async {
                use openrust::support::facades::DB;

                let mut stale = User::find(Value::Integer(1)).await.unwrap().unwrap();
                let table = || {
                    DB::connection(None)
                        .unwrap()
                        .table("users")
                        .r#where("id", "=", 1)
                };
                table()
                    .update(
                        [
                            ("email".into(), "FIRST@example.test".into()),
                            ("email_verified_at".into(), Value::Null),
                        ]
                        .into(),
                    )
                    .await
                    .unwrap();
                assert!(!stale.markEmailAsVerified().await.unwrap());
                assert_eq!(
                    table().value("email_verified_at").await.unwrap(),
                    Value::Null
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();

    let stale = request(addr, "GET", target(addr, &old), Some("first"), true).await;
    assert!(stale.starts_with("HTTP/1.1 403 "), "{stale}");
    assert_eq!(events.load(Ordering::SeqCst), 0);
    let sent = request(addr, "POST", send, Some("first"), true).await;
    assert!(sent.starts_with("HTTP/1.1 204 "), "{sent}");
    let current = extract_url(&mail_body(&kernel, None, "Verify your email address").await);
    assert_ne!(old, current);
    let path = target(addr, &current);
    let (first, second) = tokio::join!(
        request(addr, "GET", path, Some("first"), true),
        request(addr, "GET", path, Some("first"), true),
    );
    for response in [first, second] {
        assert!(response.starts_with("HTTP/1.1 204 "), "{response}");
    }
    assert_eq!(events.load(Ordering::SeqCst), 1);
    let verified = request(addr, "GET", "/protected", Some("first"), true).await;
    assert!(verified.starts_with("HTTP/1.1 204 "), "{verified}");
    server.abort();
}

#[tokio::test]
async fn sqlite_verification_requires_current_email_and_emits_once() {
    exercise_email_change(json!({"driver":"sqlite","database":"fixture.sqlite"})).await;
}

#[tokio::test]
async fn mysql_verification_requires_current_email_and_emits_once() {
    if let Ok(url) = std::env::var("OPENRUST_TEST_VERIFY_MYSQL_URL") {
        exercise_email_change(json!({"url":url})).await;
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_VERIFY_MYSQL").is_none());
    }
}

#[tokio::test]
async fn password_reset_and_email_verification_over_real_http() {
    exercise(None).await;
    exercise_custom_mail(None).await;
}

#[tokio::test]
async fn http_authentication_links_from_real_smtp() {
    let ports = std::env::var("OPENRUST_TEST_SMTP_PORT")
        .ok()
        .zip(std::env::var("OPENRUST_TEST_MAILPIT_HTTP_PORT").ok());
    if let Some((smtp, http)) = ports {
        let mailpit = Some((smtp.parse().unwrap(), http.parse().unwrap()));
        exercise(mailpit).await;
        exercise_custom_mail(mailpit).await;
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MAILPIT").is_none());
    }
}

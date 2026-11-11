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
        passwords::PasswordResetStatus,
    },
    contracts::{
        auth::Authenticatable, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    database::{DatabaseManager, Value, eloquent::Model},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        Application, auth::EmailVerificationRequest, configuration::Routing, http::Kernel,
    },
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
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

struct Provider;
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<AuthManager>()?
                .registerModel::<User>("fixture.user");
            Ok(())
        })
    }
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
                    model.set_remember_token(Some("rotated".into()));
                    model.save().await.map_err(backend)?;
                    Event::dispatch(PasswordReset {
                        user: model.get_auth_identifier(),
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
        let mut user = User::find(Value::from(authenticated.get_auth_identifier()))
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
    routes.r#match(&[Method::POST], "/forgot-password", send_link)?;
    routes
        .get("/reset-password/{token}", empty)?
        .name("password.reset");
    routes.r#match(&[Method::POST], "/reset-password/{token}", reset)?;
    routes
        .get("/verify-email", empty)?
        .name("verification.notice");
    routes
        .get("/verify-email/{id}/{hash}", verify)?
        .name("verification.verify")
        .middleware_name("auth")
        .middleware_name("signed");
    routes
        .r#match(&[Method::POST], "/register-notification", send_verification)?
        .middleware_name("auth");
    routes
        .r#match(
            &[Method::POST],
            "/email/verification-notification",
            send_verification,
        )?
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
        "database":{"default":"fixture","connections":{"fixture":{"driver":"sqlite","database":"fixture.sqlite"}}},
        "auth":{"defaults":{"guard":"api","passwords":"users"},"guards":{"api":{"driver":"token","provider":"users"}},
                "providers":{"users":{"driver":"eloquent","model":"fixture.user"}},
                "passwords":{"users":{"provider":"users","table":"password_reset_tokens","expire":60,"throttle":60}}},
        "mail":{"default":if mailpit.is_some() {"smtp"} else {"array"}, "from":{"address":"auth@example.test"},
                "mailers":{"array":{"transport":"array"},"smtp":{"transport":"smtp","host":"127.0.0.1","port":mailpit.map_or(1, |(port, _)| port)}}}
    });
    std::fs::write(root.0.join("fixture.json"), config.to_string()).unwrap();
    let mut app = Application::configure(root.0.clone())
        .withConfig(fixture_config)
        .withProviders(vec![Box::new(Provider)])
        .withRouting(Routing::new(routes))
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

async fn mail_url(kernel: &Kernel, mailpit: Option<(u16, u16)>, subject: &str) -> String {
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
        return extract_url(message["Text"].as_str().unwrap());
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
                // The text-only message can use quoted-printable transfer encoding.
                let mime = mime.replace("=\r\n", "").replace("=3D", "=");
                Ok(extract_url(&mime))
            }),
            Default::default(),
        )
        .await
        .unwrap()
}

fn extract_url(text: &str) -> String {
    text.lines()
        .find(|line| line.starts_with("http://"))
        .unwrap()
        .trim()
        .into()
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
    let (_root, addr, kernel, server) = start(mailpit).await;
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
    let reset = mail_url(&kernel, mailpit, "Reset Password Notification").await;
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
    let signed = mail_url(&kernel, mailpit, "Verify your email address").await;
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

#[tokio::test]
async fn password_reset_and_email_verification_over_real_http() {
    exercise(None).await;
}

#[tokio::test]
async fn http_authentication_links_from_real_smtp() {
    let ports = std::env::var("OPENRUST_TEST_SMTP_PORT")
        .ok()
        .zip(std::env::var("OPENRUST_TEST_MAILPIT_HTTP_PORT").ok());
    if let Some((smtp, http)) = ports {
        exercise(Some((smtp.parse().unwrap(), http.parse().unwrap()))).await;
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MAILPIT").is_none());
    }
}

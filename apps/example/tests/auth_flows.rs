//! 验证生成的认证路由端到端行为。

#[path = "../src/app.rs"]
mod app;
#[path = "../src/jobs/mod.rs"]
mod jobs;
#[path = "../src/routes.rs"]
mod routes;
#[path = "../src/seed.rs"]
mod seed;

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use figment::{
    Figment,
    providers::{Format, Toml},
};
use openrust::http::axum::http::StatusCode;
use openrust::mail::{MailDriver, Mailer};
use openrust::prelude::*;
use openrust::tokio::io::{AsyncReadExt, AsyncWriteExt};
use openrust::tokio::net::{TcpListener, TcpStream};
use openrust::tokio::sync::oneshot;
use testing::TestDatabase;

type TestResult = std::result::Result<(), Box<dyn StdError>>;

#[derive(Clone, Default)]
struct CaptureMailer {
    messages: Arc<Mutex<Vec<String>>>,
}

impl CaptureMailer {
    fn latest_code(&self) -> std::result::Result<String, Box<dyn StdError>> {
        let messages = self
            .messages
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let message = messages.last().ok_or("expected a generated email")?;
        let tail = message
            .split("Your OpenRust verification code is: ")
            .nth(1)
            .ok_or("generated email did not contain a verification code")?;
        let code = tail
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();
        if code.len() != 6 {
            return Err("generated email did not contain a six-digit code".into());
        }
        Ok(code)
    }
}

impl MailDriver for CaptureMailer {
    fn send(
        &self,
        message: openrust::mail::lettre::Message,
    ) -> BoxFuture<'_, openrust::Result<()>> {
        Box::pin(async move {
            let formatted = String::from_utf8_lossy(&message.formatted()).into_owned();
            self.messages
                .lock()
                .map_err(|error| {
                    openrust::Error::internal(std::io::Error::other(error.to_string()))
                })?
                .push(formatted);
            Ok(())
        })
    }
}

struct CaptureMailerPlugin(CaptureMailer);

impl Plugin for CaptureMailerPlugin {
    fn build(&self, app: &mut AppBuilder) -> openrust::Result<()> {
        app.insert(Mailer::new(self.0.clone()));
        Ok(())
    }

    fn finish<'a>(&'a self, _app: &'a App) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail() -> TestResult {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping generated auth HTTP test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<openrust_migration::Migrator>(&admin_url).await?;
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x42_u8; 32]);
    let link_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x24_u8; 32]);
    let config = format!(
        "[database]\nurl = {:?}\n\n[cache]\ndefault_store = \"default\"\n\n[cache.stores.default]\ndriver = \"memory\"\n\n[server]\ntrusted_proxy_ranges = [\"127.0.0.1/32\"]\n\n[auth.jwt]\nissuer = \"http://openrust.test\"\naudience = \"generated-auth-tests\"\nactive_key_id = \"test\"\n\n[auth.jwt.keys]\ntest = {:?}\n\n[security.signed_links]\nissuer = \"http://openrust.test\"\nactive_key_id = \"test-link\"\n\n[security.signed_links.keys]\ntest-link = {:?}\n\n\n[security.encryption]\nactive_key_id = \"test\"\n\n[security.encryption.keys]\ntest = \"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=\"\n\n[mail]\ndriver = \"log\"\n\n[sms]\ndriver = \"log\"\n",
        database.url(),
        key,
        link_key
    );
    let figment = Figment::new().merge(Toml::string(&config));
    let framework = figment
        .clone()
        .extract::<openrust::config::FrameworkConfig>()?;
    let mut builder = AppBuilder::new(framework, figment);
    app::Example::build(&mut builder);
    let captured_mail = CaptureMailer::default();
    builder.plugin(CaptureMailerPlugin(captured_mail.clone()));
    let app = openrust::lifecycle::finalize(builder).await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = openrust::http::build_router(&app)?;
    let (shutdown, shutdown_signal) = oneshot::channel();
    let server = openrust::tokio::spawn(async move {
        openrust::http::axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_signal.await;
        })
        .await
    });

    let result = run_auth_flow(address, &captured_mail, &app).await;
    let _ = shutdown.send(());
    server.await??;
    drop(app);
    database.close().await?;
    result
}

async fn run_auth_flow(
    address: SocketAddr,
    mail: &CaptureMailer,
    app: &openrust::App,
) -> TestResult {
    let root = request(address, "GET", "/", None, None, None).await?;
    assert_eq!(root.status, StatusCode::OK);
    assert_eq!(root.body["success"], true);
    assert_eq!(root.body["data"]["name"], "example");
    let queued = post(
        address,
        "/api/jobs/example-heartbeat",
        serde_json::json!({}),
    )
    .await?;
    assert_eq!(queued.body["success"], true);
    assert!(queued.body["data"]["job_id"].is_string());

    let email = format!("user-{}@example.test", std::process::id());
    let registered = post_with_forwarded_for(
        address,
        "/auth/register",
        serde_json::json!({
            "email": email,
            "password": "initial-password"
        }),
        "203.0.113.77",
    )
    .await?;
    assert_eq!(registered.status, StatusCode::OK);
    assert_eq!(registered.body["success"], true);
    assert_eq!(registered.body["data"]["status"], "accepted");

    let verification_code = mail.latest_code()?;
    let unverified_login = post(
        address,
        "/auth/login",
        serde_json::json!({
            "email": email,
            "password": "initial-password"
        }),
    )
    .await?;
    assert_eq!(unverified_login.body["success"], false);
    assert_eq!(unverified_login.body["code"], "unauthorized");

    let verified = post(
        address,
        "/auth/verify-email",
        serde_json::json!({
            "email": email,
            "code": verification_code
        }),
    )
    .await?;
    assert_eq!(verified.body["success"], true);
    assert_eq!(verified.body["data"]["status"], "verified");

    let login = post(
        address,
        "/auth/login",
        serde_json::json!({
            "email": email,
            "password": "initial-password"
        }),
    )
    .await?;
    assert_eq!(login.body["success"], true);
    let login_data = &login.body["data"];
    let user_id = login_data["user_id"]
        .as_i64()
        .ok_or("login omitted user ID")?;
    let access_token = login_data["access_token"]
        .as_str()
        .ok_or("login omitted access token")?;
    let refresh_token = login_data["refresh_token"]
        .as_str()
        .ok_or("login omitted refresh token")?;
    let current_user = get(address, "/api/me", access_token).await?;
    assert_eq!(current_user.body["data"]["user_id"], user_id);
    let personal_access_token = app
        .state()
        .try_get::<openrust::auth::PersonalAccessTokens>()?
        .issue(
            user_id,
            "generated-http-test",
            &["profile:read".to_owned()],
            None,
        )
        .await?;
    let pat_user = get(address, "/api/me/pat", personal_access_token.expose()).await?;
    assert_eq!(pat_user.body["success"], true);
    assert_eq!(pat_user.body["data"]["user_id"], user_id);

    let refreshed = post(
        address,
        "/auth/refresh",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(refreshed.body["success"], true);
    assert_ne!(refreshed.body["data"]["refresh_token"], refresh_token);
    let replay = post(
        address,
        "/auth/refresh",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(replay.body["success"], false);
    assert_eq!(replay.body["code"], "unauthorized");

    let reset_requested = post(
        address,
        "/auth/password-reset",
        serde_json::json!({
            "email": email
        }),
    )
    .await?;
    assert_eq!(reset_requested.body["success"], true);
    let reset_code = mail.latest_code()?;
    let reset = post(
        address,
        "/auth/password-reset/confirm",
        serde_json::json!({
            "email": email,
            "code": reset_code,
            "new_password": "replacement-password"
        }),
    )
    .await?;
    assert_eq!(reset.body["success"], true);
    assert_eq!(reset.body["data"]["status"], "password_reset");

    let login = post(
        address,
        "/auth/login",
        serde_json::json!({
            "email": email,
            "password": "replacement-password"
        }),
    )
    .await?;
    assert_eq!(login.body["success"], true);
    let refresh_token = login.body["data"]["refresh_token"]
        .as_str()
        .ok_or("login omitted refresh token")?;
    let logout = post(
        address,
        "/auth/logout",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(logout.body["success"], true);
    assert_eq!(logout.body["data"]["status"], "logged_out");
    let revoked = post(
        address,
        "/auth/refresh",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(revoked.body["success"], false);
    assert_eq!(revoked.body["code"], "unauthorized");

    for _ in 0..2 {
        let additional_login = post(
            address,
            "/auth/login",
            serde_json::json!({
                "email": email,
                "password": "replacement-password"
            }),
        )
        .await?;
        assert_eq!(additional_login.status, StatusCode::OK);
        assert_eq!(additional_login.body["success"], true);
    }
    let rate_limited = post(
        address,
        "/auth/login",
        serde_json::json!({
            "email": email,
            "password": "replacement-password"
        }),
    )
    .await?;
    // 生成的 API 项目默认使用 AlwaysOk envelope mode；错误会在 JSON 中保留稳定机器码，
    // 同时 HTTP 传输状态仍为 200。
    assert_eq!(rate_limited.status, StatusCode::OK);
    assert_eq!(rate_limited.body["success"], false);
    assert_eq!(rate_limited.body["code"], "rate_limited");
    Ok(())
}

struct HttpResponse {
    status: StatusCode,
    body: serde_json::Value,
}

async fn post(
    address: SocketAddr,
    path: &str,
    payload: serde_json::Value,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    request(address, "POST", path, Some(payload.to_string()), None, None).await
}

async fn post_with_forwarded_for(
    address: SocketAddr,
    path: &str,
    payload: serde_json::Value,
    forwarded_for: &str,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    request(
        address,
        "POST",
        path,
        Some(payload.to_string()),
        None,
        Some(forwarded_for),
    )
    .await
}

async fn get(
    address: SocketAddr,
    path: &str,
    bearer: &str,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    request(address, "GET", path, None, Some(bearer), None).await
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<String>,
    bearer: Option<&str>,
    forwarded_for: Option<&str>,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    let stream = TcpStream::connect(address).await?;
    let (mut reader, mut writer) = stream.into_split();
    let body = body.unwrap_or_default();
    let authorization = bearer.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let forwarded_for = forwarded_for.map_or_else(String::new, |address| {
        format!("X-Forwarded-For: {address}\r\n")
    });
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{authorization}{forwarded_for}\r\n{body}",
        body.len()
    );
    writer.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    reader.read_to_end(&mut response).await?;
    drop(writer);
    let response = String::from_utf8(response)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("malformed HTTP response for {method} {path}: {response:?}"))?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .ok_or("HTTP response omitted status")?
        .parse::<u16>()?;
    Ok(HttpResponse {
        status: StatusCode::from_u16(status)?,
        body: serde_json::from_str(body)?,
    })
}

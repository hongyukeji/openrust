//! 验证生成的认证路由端到端行为。

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
{{grpc_module}}#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_cookie.rs"]
mod request_cookie;
#[path = "support/request_input.rs"]
mod request_input;
#[path = "support/request_session.rs"]
mod request_session;
#[path = "../routes/web.rs"]
mod routes;
#[path = "../database/seeders.rs"]
mod seed;

use bootstrap::OpenRustGeneratedApplication as GeneratedApplication;

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::http::axum::http::StatusCode;
use openrust::mail::{MailDriver, Mailer};
use openrust::prelude::*;
use openrust::tokio::io::{AsyncReadExt, AsyncWriteExt};
use openrust::tokio::net::{TcpListener, TcpStream};
use openrust::tokio::sync::oneshot;
use testing::TestDatabase;

type TestResult<T = ()> = std::result::Result<T, Box<dyn StdError>>;

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

struct CaptureMailerProvider(CaptureMailer);

impl openrust::core::ServiceProvider for CaptureMailerProvider {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        app.instance(Arc::new(Mailer::new(self.0.clone())));
        Ok(())
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
        "[database]\nurl = {:?}\n\n[cache]\ndefault = \"default\"\n\n[cache.stores.default]\ndriver = \"memory\"\n\n[server]\ntrusted_proxy_ranges = [\"127.0.0.1/32\"]\n\n[auth.jwt]\nissuer = \"http://openrust.test\"\naudience = \"generated-auth-tests\"\nactive_key_id = \"test\"\n\n[auth.jwt.keys]\ntest = {:?}\n\n[security.signed_links]\nissuer = \"http://openrust.test\"\nactive_key_id = \"test-link\"\n\n[security.signed_links.keys]\ntest-link = {:?}\n\n{{encryption_config}}\n\n[view]\ndirectory = \"resources/views\"\n\n[mail]\ndriver = \"log\"\n\n[sms]\ndriver = \"log\"\n",
        database.url(),
        key,
        link_key
    );
    let figment = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(&config));
    let application_config = figment.extract::<bootstrap::Configuration>()?;
    let mut builder = openrust::core::ProviderBuilder::for_application::<GeneratedApplication>(
        &application_config,
    );
    let captured_mail = CaptureMailer::default();
    builder.provider(CaptureMailerProvider(captured_mail.clone()));
    let runtime = builder.start(figment).await?;
    let app = openrust::core::App::from_provider_runtime(runtime)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = openrust::http::build_router(&app)?;
    let (shutdown, shutdown_signal) = oneshot::channel();
    let mut server = openrust::tokio::spawn(async move {
        openrust::http::axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_signal.await;
        })
        .await
    });

    let result = bounded_auth_business(run_auth_flow(address, &captured_mail, &app)).await;
    let _ = shutdown.send(());
    let server_result: TestResult = match openrust::tokio::time::timeout(
        std::time::Duration::from_secs(5),
        &mut server,
    )
    .await
    {
        Ok(Ok(result)) => result.map_err(Into::into),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => {
            server.abort();
            let _ = openrust::tokio::time::timeout(std::time::Duration::from_secs(1), &mut server)
                .await;
            Err(error.into())
        }
    };
    let app_result = app
        .shutdown(openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    drop(app);
    let database_result =
        openrust::tokio::time::timeout(std::time::Duration::from_secs(10), database.close()).await;
    // 先完成所有回收；清理错误不能覆盖原始业务验收失败。
    result?;
    server_result?;
    app_result?;
    database_result??;
    Ok(())
}

async fn run_auth_flow(
    address: SocketAddr,
    mail: &CaptureMailer,
    app: &openrust::App,
) -> TestResult {
    let email = format!("user-{}@example.test", std::process::id());
    let registered = post_with_forwarded_for(
        address,
        "/api/auth/register",
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
    assert_eq!(registered.set_cookie_count, 1);
    // 不记录 Cookie 原文；保护模式须改变40字节会话ID的wire表示，往返另有正式helper覆盖。
{{session_protection_assertion}}

    let verification_code = mail.latest_code()?;
    let unverified_login = post(
        address,
        "/api/auth/login",
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
        "/api/auth/verify-email",
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
        "/api/auth/login",
        serde_json::json!({
            "email": email,
            "password": "initial-password"
        }),
    )
    .await?;
    assert_eq!(login.body["success"], true);
    let login_data = &login.body["data"]{{login_credentials_suffix}};
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
        .resolve::<openrust::auth::PersonalAccessTokens>()
        .await?
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
        "/api/auth/refresh",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(refreshed.body["success"], true);
    assert_ne!(refreshed.body["data"]["refresh_token"], refresh_token);
    let replay = post(
        address,
        "/api/auth/refresh",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(replay.body["success"], false);
    assert_eq!(replay.body["code"], "unauthorized");

    let reset_requested = post(
        address,
        "/api/auth/password-reset",
        serde_json::json!({
            "email": email
        }),
    )
    .await?;
    assert_eq!(reset_requested.body["success"], true);
    let reset_code = mail.latest_code()?;
    let reset = post(
        address,
        "/api/auth/password-reset/confirm",
        serde_json::json!({
            "email": email,
            "code": reset_code,
            "password": "  replacement-password  "
        }),
    )
    .await?;
    assert_eq!(reset.body["success"], true);
    assert_eq!(reset.body["data"]["status"], "password_reset");

    // 使用独立可信来源验证去空白后的密码不匹配，保留后续既有限流预算。
    let trimmed_login = post_with_forwarded_for(
        address,
        "/api/auth/login",
        serde_json::json!({
            "email": email,
            "password": "replacement-password"
        }),
        "203.0.113.79",
    )
    .await?;
    assert_eq!(trimmed_login.status, StatusCode::UNAUTHORIZED);
    assert_eq!(trimmed_login.body["success"], false);
    assert_eq!(trimmed_login.body["code"], "unauthorized");

    let login = post(
        address,
        "/api/auth/login",
        serde_json::json!({
            "email": email,
            "password": "  replacement-password  "
        }),
    )
    .await?;
    assert_eq!(login.body["success"], true);
    let refresh_token = login.body["data"]{{login_credentials_suffix}}["refresh_token"]
        .as_str()
        .ok_or("login omitted refresh token")?;
    let logout = post(
        address,
        "/api/auth/logout",
        serde_json::json!({
            "refresh_token": refresh_token
        }),
    )
    .await?;
    assert_eq!(logout.body["success"], true);
    assert_eq!(logout.body["data"]["status"], "logged_out");
    let revoked = post(
        address,
        "/api/auth/refresh",
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
            "/api/auth/login",
            serde_json::json!({
                "email": email,
                "password": "  replacement-password  "
            }),
        )
        .await?;
        assert_eq!(additional_login.status, StatusCode::OK);
        assert_eq!(additional_login.body["success"], true);
    }
    let rate_limited = post(
        address,
        "/api/auth/login",
        serde_json::json!({
            "email": email,
            "password": "  replacement-password  "
        }),
    )
    .await?;
    assert_eq!(rate_limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rate_limited.body["success"], false);
    assert_eq!(rate_limited.body["code"], "rate_limited");
{{mfa_flow}}    Ok(())
}

struct HttpResponse {
    status: StatusCode,
    body: serde_json::Value,
    set_cookie_count: usize,
    session_cookie_is_protected: bool,
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
    let set_cookies = headers
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("set-cookie")
                .then_some(value.trim())
        })
        .collect::<Vec<_>>();
    let session_cookie_is_protected = set_cookies
        .first()
        .and_then(|wire| {
            wire.split(';').next()?.split_once('=').map(|(_, value)| {
                value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        })
        .unwrap_or(false);
    Ok(HttpResponse {
        status: StatusCode::from_u16(status)?,
        body: serde_json::from_str(body)?,
        set_cookie_count: set_cookies.len(),
        session_cookie_is_protected,
    })
}

/// TestApp 使用相同 Provider 清单，覆盖 Mailer 后验证真实认证路由。
#[tokio::test]
async fn generated_test_app_overrides_mailer_for_registration() -> TestResult {
    use openrust::http::axum::{body::Body, http::Request};
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping generated TestApp auth: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<openrust_migration::Migrator>(&admin_url).await?;
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x42_u8; 32]);
    let link_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x24_u8; 32]);
    let config = format!(
        "[database]\nurl = {:?}\n[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[auth.jwt]\nissuer = 'http://openrust.test'\naudience = 'generated-auth-tests'\nactive_key_id = 'test'\n[auth.jwt.keys]\ntest = {:?}\n[security.signed_links]\nissuer = 'http://openrust.test'\nactive_key_id = 'test-link'\n[security.signed_links.keys]\ntest-link = {:?}\n{{encryption_config}}\n[view]\ndirectory = 'resources/views'\n",
        database.url(),
        key,
        link_key
    );
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Toml::string(&config));
    let mail = CaptureMailer::default();
    let captured = mail.clone();
    let input_source = source.clone();
    let mut builder = openrust::testing::TestApp::<GeneratedApplication>::builder(source);
    builder.configure_after_register(move |bindings| {
        bindings.instance(Arc::new(Mailer::new(captured.clone())));
        Ok(())
    });
    let mut app = builder.build().await?;
    let result = bounded_auth_business(async {
        request_input::assert_request_input::<GeneratedApplication>(input_source.clone()).await?;
        request_cookie::assert_request_cookie::<GeneratedApplication>(input_source.clone(), true)
            .await?;
        request_session::assert_request_session::<GeneratedApplication>(input_source, true).await?;
        let payload =
            serde_json::json!({"email": "testapp@example.test", "password": "initial-password"});
        let request = Request::builder()
            .method("POST")
            .uri("/api/auth/register")
            .header("content-type", "application/json")
            .extension(openrust::http::axum::extract::ConnectInfo(
                SocketAddr::from(([127, 0, 0, 1], 12345)),
            ))
            .body(Body::from(serde_json::to_vec(&payload)?))?;
        app.request(request).await?.assert_status(StatusCode::OK)?;
        assert_eq!(mail.latest_code()?.len(), 6);
        Ok(())
    })
    .await;
    let app_result = app
        .shutdown(openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    drop(app);
    let database_result =
        openrust::tokio::time::timeout(std::time::Duration::from_secs(10), database.close()).await;
    result?;
    app_result?;
    database_result??;
    Ok(())
}

/// 捕获每次 poll 中的业务 panic；取消和超时先释放借用，再由调用方统一回收资源。
async fn bounded_auth_business(
    future: impl std::future::Future<Output = TestResult>,
) -> TestResult {
    let mut future = std::pin::pin!(future);
    let caught = std::future::poll_fn(|context| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            future.as_mut().poll(context)
        })) {
            Ok(result) => result,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("业务验收 panic（非字符串 payload）");
                std::task::Poll::Ready(Err(std::io::Error::other(message.to_owned()).into()))
            }
        }
    });
    openrust::tokio::time::timeout(std::time::Duration::from_secs(120), caught).await?
}

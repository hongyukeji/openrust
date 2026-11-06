//! 显式JWT API的邮件、密码、受管PAT与刷新安全回归。
#![cfg(feature = "auth-fortify")]

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
{{grpc_module}}
#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_input.rs"]
mod request_input;

#[path = "../routes/mod.rs"]
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

fn test_configuration() -> openrust::config::FrameworkConfig {
    let mut config = openrust::config::FrameworkConfig::default();
    config.app.env = openrust::config::Environment::Testing;
    config
}

// 测试覆盖与真实启动共用代码配置；不能遗漏应用声明的具名账户 Provider。
fn application_test_source(overrides: Figment) -> TestResult<openrust::config::ConfigSources> {
    let entries = overrides.extract::<std::collections::BTreeMap<String, serde_json::Value>>()?;
    Ok(
        openrust::config::ConfigLoader::application_code_sources_with_overrides(
            ".",
            GeneratedApplication::config_namespace(),
            Some("testing"),
            &GeneratedApplication::config(),
            entries,
        )?,
    )
}

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

impl openrust::support::ServiceProvider for CaptureMailerProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(Mailer::new(self.0.clone()))).await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail() -> TestResult {
    {{auth_http_database_setup}}

    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x42_u8; 32]);
    let link_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x24_u8; 32]);
    let config = format!(
        "[session]\ndriver = 'database'\n[database]\nurl = {:?}\n\n[cache]\ndefault = \"default\"\n\n[cache.stores.default]\ndriver = \"memory\"\n\n[server]\ntrusted_proxy_ranges = [\"127.0.0.1/32\"]\n\n[auth.jwt]\nissuer = \"http://openrust.test\"\naudience = \"generated-auth-tests\"\nactive_key_id = \"test\"\n\n[auth.jwt.keys]\ntest = {:?}\n\n[security.signed_links]\nissuer = \"http://openrust.test\"\nactive_key_id = \"test-link\"\n\n[security.signed_links.keys]\ntest-link = {:?}\n\n{{encryption_config}}
\n\n[view]\ndirectory = \"resources/views\"\n\n[mail]\ndriver = \"log\"\n\n[sms]\ndriver = \"log\"\n",
        database.url(),
        key,
        link_key
    );
    let figment =
        Figment::from(Serialized::defaults(test_configuration())).merge(Toml::string(&config));
    let source = application_test_source(figment)?;
    let application_config = source.figment().extract::<bootstrap::Configuration>()?;
    let mut builder = openrust::foundation::ProviderBuilder::for_application::<GeneratedApplication>(
        &application_config,
    );
    let captured_mail = CaptureMailer::default();
    builder.provider(CaptureMailerProvider(captured_mail.clone()));
    let runtime = builder.start(source).await?;
    let app = openrust::foundation::Application::from_provider_runtime(runtime)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = openrust::foundation::http::build_router(&app)?;
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
    app: &openrust::Application,
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
    assert_eq!(registered.set_cookie_count, 0);
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
    assert_eq!(login.status, StatusCode::OK);
    let login_data = &login.body;
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
    let personal_access_tokens = app
        .state()
        .resolve::<openrust::auth::PersonalAccessTokens>()
        .await?;
    #[cfg(feature = "auth-fortify")]
    let personal_access_token = {
        // 生成项目沿真实登录JWT派生，不能用裸ID绕过已绑定的凭据授权。
        let rejected = personal_access_tokens
            .issue(user_id, "unverified", &[], None)
            .await
            .err()
            .ok_or("raw personal token issuance unexpectedly succeeded")?;
        assert_eq!(rejected.code(), "auth_credential_proof_required");
        let authority = app
            .state()
            .resolve::<openrust::auth::fortify::CredentialAuthority>()
            .await?;
        let jwt = app.state().resolve::<openrust::auth::JwtTokens>().await?;
        let parent = authority
            .authenticate_access_token(jwt.as_ref(), access_token)
            .await?;
        assert_eq!(parent.user_id(), user_id);
        let expires_at = parent.expires_at();
        personal_access_tokens
            .issue_from_verified(
                parent,
                "generated-http-test",
                &["profile:read".to_owned()],
                expires_at,
            )
            .await?
    };
    #[cfg(not(feature = "auth-fortify"))]
    let personal_access_token = personal_access_tokens
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
    assert_eq!(refreshed.status, StatusCode::OK);
    assert_ne!(refreshed.body["refresh_token"], refresh_token);
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
    assert_eq!(login.status, StatusCode::OK);
    let refresh_token = login.body["refresh_token"]
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
    assert_eq!(logout.status, StatusCode::OK);
    assert_eq!(logout.body["logged_out"], true);
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
        assert!(additional_login.body["access_token"].is_string());
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
    Ok(())
}

struct HttpResponse {
    status: StatusCode,
    body: serde_json::Value,
    set_cookie_count: usize,
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
    let body = if path == "/api/auth/login" {
        let mut value: serde_json::Value =
            serde_json::from_str(body.as_deref().ok_or("login body missing")?)?;
        let email = value
            .as_object_mut()
            .ok_or("login object missing")?
            .remove("email")
            .ok_or("login account missing")?;
        value["account"] = email;
        Some(value.to_string())
    } else {
        body
    };
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
        .ok_or("malformed generated HTTP response")?;
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
    assert!(
        set_cookies.is_empty(),
        "JWT API response must not set cookies: {method} {path}"
    );
    Ok(HttpResponse {
        status: StatusCode::from_u16(status)?,
        body: serde_json::from_str(body)?,
        set_cookie_count: set_cookies.len(),
    })
}

/// TestApp 使用相同 Provider 清单，覆盖 Mailer 后验证真实认证路由。
#[tokio::test]
async fn generated_test_app_overrides_mailer_for_registration() -> TestResult {
    use openrust::http::axum::{body::Body, http::Request};
    {{auth_testapp_database_setup}}

    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x42_u8; 32]);
    let link_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x24_u8; 32]);
    let config = format!(
        "[session]\ndriver = 'database'\n[database]\nurl = {:?}\n[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[auth.jwt]\nissuer = 'http://openrust.test'\naudience = 'generated-auth-tests'\nactive_key_id = 'test'\n[auth.jwt.keys]\ntest = {:?}\n[security.signed_links]\nissuer = 'http://openrust.test'\nactive_key_id = 'test-link'\n[security.signed_links.keys]\ntest-link = {:?}\n{{encryption_config}}
\n[view]\ndirectory = 'resources/views'\n",
        database.url(),
        key,
        link_key
    );
    let source =
        Figment::from(Serialized::defaults(test_configuration())).merge(Toml::string(&config));
    let source = application_test_source(source)?;
    let mail = CaptureMailer::default();
    let captured = mail.clone();
    let input_source = source.figment().clone();
    let mut builder = openrust::testing::TestApp::<GeneratedApplication>::builder(source);
    builder.configure_after_register(move |bindings| {
        let captured = captured.clone();
        Box::pin(async move {
            bindings.instance(Arc::new(Mailer::new(captured))).await?;
            Ok(())
        })
    });
    let mut app = builder.build().await?;
    let result = bounded_auth_business(async {
        request_input::assert_request_input::<GeneratedApplication>(input_source.clone()).await?;

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
        assert_generated_named_user_provider(
            app.app()?,
            "testapp@example.test",
            "initial-password",
        )
        .await?;
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

/// 解析生成应用实际注册的工厂，避免JWT成功掩盖默认账户Provider失联。
async fn assert_generated_named_user_provider(
    app: &openrust::Application,
    email: &str,
    plain: &str,
) -> TestResult {
    use openrust::auth::{AuthManager, Credentials};
    use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};

    let scope = app.state().container().scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    assert!(Arc::ptr_eq(
        &manager,
        &scope.resolve::<AuthManager>().await?
    ));
    assert_eq!(manager.default_driver()?, "api");
    // Session Guard需要HTTP请求槽；这里仅解析同scope生产注册的账户Provider。
    let provider = manager
        .create_user_provider(Some("users"))
        .await?
        .ok_or("生成应用未注册默认users Provider")?;
    // 配置故障沿生产工厂返回500；未知Provider名的None语义仍独立保留。
    let repository = app.config_repository();
    let providers = repository.section::<serde_json::Value>("auth.providers")?;
    repository.set(
        "auth.providers.users",
        serde_json::json!({"driver":"app-users"}),
    )?;
    let misconfigured = manager.create_user_provider(Some("users")).await;
    repository.set("auth.providers", providers)?;
    let Err(error) = misconfigured else {
        return Err("缺失table的应用Provider意外成功".into());
    };
    assert_eq!(error.code(), "app_user_table_required");
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let credentials = Credentials::new(serde_json::Map::from_iter([
        ("email".into(), serde_json::json!(email)),
        ("password".into(), serde_json::json!(plain)),
    ]));
    let user = provider
        .retrieve_by_credentials(&credentials)
        .await?
        .ok_or("生成应用Provider无法检索真实注册账户")?;
    let identifier = user.auth_identifier()?;
    let numeric_id = identifier
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or("真实数据库数字主键被账户Provider改变类型")?;
    assert_eq!(user.auth_identifier_name(), "id");
    assert_eq!(user.auth_password_name(), "password_hash");
    assert!(user.auth_password()?.is_some_and(|hash| !hash.is_empty()));
    assert!(
        provider
            .validate_credentials(user.as_ref(), &credentials)
            .await?
    );
    let wrong = Credentials::new(serde_json::Map::from_iter([
        ("email".into(), serde_json::json!(email)),
        (
            "password".into(),
            serde_json::json!("incorrect-generated-password"),
        ),
    ]));
    assert!(!provider.validate_credentials(user.as_ref(), &wrong).await?);
    let restored = provider
        .retrieve_by_id(&identifier)
        .await?
        .ok_or("生成应用Provider未保留数字账户标识")?;
    assert_eq!(restored.auth_identifier()?, identifier);
    assert_eq!(restored.auth_password_name(), "password_hash");

    let token = "generated-provider-remember-probe";
    assert_eq!(user.remember_token_name(), "remember_token");
    provider.update_remember_token(user.as_ref(), token).await?;
    let remembered = provider
        .retrieve_by_token(&identifier, token)
        .await?
        .ok_or("生成应用Provider未持久化记住我token")?;
    assert_eq!(remembered.auth_identifier()?, identifier);

    let db = scope.resolve::<openrust::database::Db>().await?;
    let table = db.quoted_table_name("users")?;
    let placeholder = match db.backend()? {
        DbBackend::Postgres => "$1",
        DbBackend::MySql | DbBackend::Sqlite => "?",
        _ => return Err("生成认证测试不支持当前数据库后端".into()),
    };
    // 只有表名进入SQL文本，并复用Db标识符校验；账户主键始终绑定。
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            db.backend()?,
            format!("UPDATE {table} SET disabled_at=CURRENT_TIMESTAMP WHERE id={placeholder}"),
            [numeric_id.into()],
        ))
        .await?;
    assert!(matches!(
        provider.retrieve_by_id(&identifier).await,
        Err(openrust::Error::Unauthorized)
    ));
    assert!(matches!(
        provider.retrieve_by_credentials(&credentials).await,
        Err(openrust::Error::Unauthorized)
    ));
    assert!(matches!(
        provider.retrieve_by_token(&identifier, token).await,
        Err(openrust::Error::Unauthorized)
    ));
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

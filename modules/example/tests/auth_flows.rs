//! 验证根应用挂载示例业务后的真实认证流程。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

#[path = "../../../bootstrap/app.rs"]
mod bootstrap;
#[path = "../../../modules/mod.rs"]
mod modules;
#[path = "../../../routes/web.rs"]
mod routes;

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::db::sea_orm::{ConnectionTrait, Statement};
use openrust::http::axum::http::StatusCode;
use openrust::mail::{MailDriver, Mailer};
use openrust::prelude::*;
use openrust::sms::{SmsDriver, SmsMessage, SmsReceipt, SmsSender};
use openrust::testing::TestDatabase;
use openrust::tokio::io::{AsyncReadExt, AsyncWriteExt};
use openrust::tokio::net::{TcpListener, TcpStream};
use openrust::tokio::sync::oneshot;

type TestResult = std::result::Result<(), Box<dyn StdError>>;

struct TestMigrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for TestMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(modules::example::database::migrations::Migrator::migrations());
        migrations
    }
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
            .take_while(char::is_ascii_digit)
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

#[derive(Clone, Default)]
struct CaptureSms {
    messages: Arc<Mutex<Vec<SmsMessage>>>,
}

impl SmsDriver for CaptureSms {
    fn send(&self, message: SmsMessage) -> BoxFuture<'_, openrust::Result<SmsReceipt>> {
        Box::pin(async move {
            self.messages
                .lock()
                .map_err(|error| {
                    openrust::Error::internal(std::io::Error::other(error.to_string()))
                })?
                .push(message);
            Ok(SmsReceipt {
                provider: "test-capture".to_owned(),
                message_id: None,
            })
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

struct CaptureSmsProvider(CaptureSms);
impl openrust::core::ServiceProvider for CaptureSmsProvider {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> openrust::Result<()> {
        app.instance(Arc::new(SmsSender::new(self.0.clone())));
        Ok(())
    }
}

#[tokio::test]
async fn generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail() -> TestResult {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping generated auth HTTP test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<TestMigrator>(&admin_url).await?;
    #[cfg(feature = "http-client")]
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await?;
    #[cfg(feature = "http-client")]
    let upstream_origin = format!("http://{}/", upstream_listener.local_addr()?);
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x42_u8; 32]);
    let link_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x24_u8; 32]);
    let config = format!(
        "[database]\nurl = {:?}\n\n[cache]\ndefault_store = \"default\"\n\n[cache.stores.default]\ndriver = \"memory\"\n\n[server]\ntrusted_proxy_ranges = [\"127.0.0.1/32\"]\n\n[auth.jwt]\nissuer = \"http://openrust.test\"\naudience = \"generated-auth-tests\"\nactive_key_id = \"test\"\n\n[auth.jwt.keys]\ntest = {:?}\n\n[security.signed_links]\nissuer = \"http://openrust.test\"\nactive_key_id = \"test-link\"\n\n[security.signed_links.keys]\ntest-link = {:?}\n\n\n[security.encryption]\nactive_key_id = \"test\"\n\n[security.encryption.keys]\ntest = \"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=\"\n\n[view]\ndirectory = \"modules/example/resources/views\"\n\n[mail]\ndriver = \"log\"\n\n[sms]\ndriver = \"log\"\n",
        database.url(),
        key,
        link_key
    );
    let mut framework = openrust::config::FrameworkConfig::default();
    framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let figment = Figment::from(Serialized::defaults(framework))
        .merge(Toml::string(&config))
        .merge(("app.name", "openrust"));
    #[cfg(feature = "metrics")]
    let figment = figment.merge(("server.metrics_endpoint_enabled", true));
    #[cfg(feature = "http-client")]
    let figment = figment.merge(("example.article_summary_origin", upstream_origin));
    let bootstrap_configuration = figment.extract::<bootstrap::Configuration>()?;
    let mut builder = openrust::core::ProviderBuilder::for_application::<bootstrap::Server>(
        &bootstrap_configuration,
    );
    let captured_mail = CaptureMailer::default();
    builder.provider(CaptureMailerProvider(captured_mail.clone()));
    let captured_sms = CaptureSms::default();
    builder.provider(CaptureSmsProvider(captured_sms.clone()));
    let app = openrust::core::App::from_provider_runtime(builder.start(figment).await?)?;
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

    #[cfg(feature = "http-client")]
    let (upstream_shutdown, upstream_shutdown_signal) = oneshot::channel::<()>();
    #[cfg(feature = "http-client")]
    let upstream_server = openrust::tokio::spawn(async move {
        let router = openrust::http::axum::Router::new().route(
            "/articles/{id}/summary",
            openrust::http::axum::routing::get(
                |openrust::http::axum::extract::Path(id): openrust::http::axum::extract::Path<
                    i64,
                >| async move {
                    match id.rem_euclid(3) {
                        1 => (
                            StatusCode::SERVICE_UNAVAILABLE,
                            openrust::http::axum::Json(serde_json::json!({"error":"unavailable"})),
                        ),
                        2 => (
                            StatusCode::OK,
                            openrust::http::axum::Json(
                                serde_json::json!({"summary":"x".repeat(20_000)}),
                            ),
                        ),
                        _ => (
                            StatusCode::OK,
                            openrust::http::axum::Json(
                                serde_json::json!({"summary":format!("Summary {id}")}),
                            ),
                        ),
                    }
                },
            ),
        );
        openrust::http::axum::serve(upstream_listener, router)
            .with_graceful_shutdown(async {
                let _ = upstream_shutdown_signal.await;
            })
            .await
    });

    let result = run_auth_flow(address, &captured_mail, &captured_sms, &app).await;
    let _ = shutdown.send(());
    server.await??;
    #[cfg(feature = "http-client")]
    {
        let _ = upstream_shutdown.send(());
        upstream_server.await??;
    }
    app.shutdown(openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    drop(app);
    database.close().await?;
    result
}

#[allow(clippy::too_many_lines)]
async fn run_auth_flow(
    address: SocketAddr,
    mail: &CaptureMailer,
    sms: &CaptureSms,
    app: &openrust::App,
) -> TestResult {
    let root = request(address, "GET", "/api", None, None, None).await?;
    assert_eq!(root.status, StatusCode::OK);
    assert_eq!(root.body["success"], true);
    assert_eq!(root.body["data"]["name"], "openrust");
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
    run_article_flow(address, mail, access_token, user_id, &email, app).await?;
    #[cfg(feature = "metrics")]
    assert_article_metrics_after_real_http_request(address).await?;

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
            "new_password": "replacement-password"
        }),
    )
    .await?;
    assert_eq!(reset.body["success"], true);
    assert_eq!(reset.body["data"]["status"], "password_reset");

    let login = post(
        address,
        "/api/auth/login",
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
                "password": "replacement-password"
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
            "password": "replacement-password"
        }),
    )
    .await?;
    // 此隔离测试显式选择 Semantic 模式，保持框架生产默认值不变。
    assert_eq!(rate_limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rate_limited.body["success"], false);
    assert_eq!(rate_limited.body["code"], "rate_limited");
    run_welcome_flow(address, mail, sms, access_token, &email).await?;
    Ok(())
}

async fn run_welcome_flow(
    address: SocketAddr,
    mail: &CaptureMailer,
    sms: &CaptureSms,
    access_token: &str,
    verified_email: &str,
) -> TestResult {
    let path = "/api/example/welcome";
    let payload = serde_json::json!({});
    let anonymous = request(address, "POST", path, Some(payload.to_string()), None, None).await?;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    let third_party = request(
        address,
        "POST",
        path,
        Some(
            serde_json::json!({
                "email":"third-party@example.test",
                "phone":"+15555550123"
            })
            .to_string(),
        ),
        Some(access_token),
        None,
    )
    .await?;
    assert!(!third_party.status.is_success());
    let mail_before = mail
        .messages
        .lock()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .len();
    let welcomed = request(
        address,
        "POST",
        path,
        Some(payload.to_string()),
        Some(access_token),
        None,
    )
    .await?;
    assert_eq!(welcomed.status, StatusCode::OK);
    assert_eq!(welcomed.body["data"]["status"], "accepted");
    {
        let messages = mail
            .messages
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        assert_eq!(messages.len(), mail_before + 1);
        let last = messages.last().ok_or("missing account welcome mail")?;
        assert!(last.contains(verified_email));
        assert!(!last.contains("third-party@example.test"));
    }
    let localized = request_with_locale(
        address,
        "POST",
        path,
        Some(payload.to_string()),
        Some(access_token),
        "zh-CN",
    )
    .await?;
    assert_eq!(localized.status, StatusCode::OK);
    let messages = mail
        .messages
        .lock()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    assert_eq!(messages.len(), mail_before + 2);
    let chinese = messages.last().ok_or("missing localized welcome mail")?;
    assert!(chinese.contains("Content-Transfer-Encoding: base64"));
    let encoded = chinese
        .split_once("\r\n\r\n")
        .ok_or("localized mail omitted its MIME body")?
        .1
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)?;
    assert!(String::from_utf8(decoded)?.contains("欢迎加入 OpenRust 示例业务。"));
    drop(messages);
    let messages = sms
        .messages
        .lock()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    assert_eq!(messages.len(), 0);
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn run_article_flow(
    address: SocketAddr,
    mail: &CaptureMailer,
    owner_token: &str,
    owner_id: i64,
    owner_email: &str,
    app: &openrust::App,
) -> TestResult {
    let path = "/api/example/articles";
    let anonymous = request(address, "GET", path, None, None, None).await?;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.body["code"], "unauthorized");

    let invalid = request(
        address,
        "POST",
        path,
        Some(serde_json::json!({"title": "   ", "body": "valid"}).to_string()),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid.body["code"], "validation_failed");
    assert!(invalid.body["errors"]["title"].is_array());

    let mut ids = Vec::new();
    for index in 1..=3 {
        let created = request(
            address,
            "POST",
            path,
            Some(
                serde_json::json!({
                    "title": format!("Article {index}"),
                    "body": format!("Body {index}")
                })
                .to_string(),
            ),
            Some(owner_token),
            None,
        )
        .await?;
        assert_eq!(created.status, StatusCode::CREATED);
        assert_eq!(created.body["success"], true);
        ids.push(
            created.body["data"]["id"]
                .as_i64()
                .ok_or("article id missing")?,
        );
    }

    let first_page = get(address, &format!("{path}?page=1&per_page=2"), owner_token).await?;
    assert_eq!(first_page.status, StatusCode::OK);
    assert_eq!(first_page.body["meta"]["total"], 3);
    assert_eq!(first_page.body["meta"]["page"], 1);
    assert_eq!(first_page.body["meta"]["per_page"], 2);
    assert_eq!(first_page.body["data"][0]["id"], ids[0]);
    assert_eq!(first_page.body["data"][1]["id"], ids[1]);
    let second_page = get(address, &format!("{path}?page=2&per_page=2"), owner_token).await?;
    assert_eq!(second_page.body["data"][0]["id"], ids[2]);
    assert_eq!(second_page.body["data"].as_array().map(Vec::len), Some(1));
    for query in ["page=0", "per_page=0", "per_page=101"] {
        let response = get(address, &format!("{path}?{query}"), owner_token).await?;
        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.body["code"], "validation_failed");
    }

    let other_email = format!("article-other-{}@example.test", std::process::id());
    let registered = post(
        address,
        "/api/auth/register",
        serde_json::json!({"email": other_email, "password": "other-password"}),
    )
    .await?;
    assert_eq!(registered.body["success"], true);
    let code = mail.latest_code()?;
    let verified = post(
        address,
        "/api/auth/verify-email",
        serde_json::json!({"email": other_email, "code": code}),
    )
    .await?;
    assert_eq!(verified.body["success"], true);
    let login = post(
        address,
        "/api/auth/login",
        serde_json::json!({"email": other_email, "password": "other-password"}),
    )
    .await?;
    let other_token = login.body["data"]["access_token"]
        .as_str()
        .ok_or("other access token missing")?;
    let foreign_list = get(address, path, other_token).await?;
    assert_eq!(foreign_list.body["meta"]["total"], 0);
    let detail = format!("{path}/{}", ids[0]);
    #[cfg(feature = "http-client")]
    run_external_summary_flow(address, &ids, owner_token, other_token).await?;
    #[cfg(feature = "signing")]
    run_share_flow(address, &detail, owner_token, other_token).await?;
    #[cfg(feature = "encryption")]
    run_private_note_flow(address, app, &ids, owner_token, other_token).await?;
    for method in ["GET", "PUT", "DELETE"] {
        let payload = (method == "PUT").then(|| {
            serde_json::json!({"title": "Hijacked title", "body": "Hijacked body"}).to_string()
        });
        let foreign = request(address, method, &detail, payload, Some(other_token), None).await?;
        assert_eq!(foreign.status, StatusCode::NOT_FOUND);
        assert_eq!(foreign.body["code"], "not_found");
    }
    let unchanged = get(address, &detail, owner_token).await?;
    assert_eq!(unchanged.body["data"]["title"], "Article 1");
    let audit_db = app.state().resolve::<openrust::Db>().await?;
    let audit_table = audit_db.quoted_table_name("audit_records")?;
    let audit_row = audit_db
        .connection()?
        .query_one_raw(openrust::db::sea_orm::Statement::from_string(
            audit_db.connection()?.get_database_backend(),
            format!(
                "SELECT actor_id, object_id, outcome FROM {audit_table} WHERE action = 'example.article.view'"
            ),
        ))
        .await?
        .ok_or("article view audit record missing")?;
    assert_eq!(
        audit_row.try_get::<String>("", "actor_id")?,
        owner_id.to_string()
    );
    assert_eq!(
        audit_row.try_get::<String>("", "object_id")?,
        ids[0].to_string()
    );
    assert_eq!(audit_row.try_get::<String>("", "outcome")?, "succeeded");

    let updated = request(
        address,
        "PUT",
        &detail,
        Some(serde_json::json!({"title": "Updated title", "body": "Updated body"}).to_string()),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(updated.body["data"]["title"], "Updated title");
    let removed = request(address, "DELETE", &detail, None, Some(owner_token), None).await?;
    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(removed.body["data"]["deleted"], true);
    let missing = get(address, &detail, owner_token).await?;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.body["code"], "not_found");

    let db = app.state().resolve::<openrust::Db>().await?;
    let backend = db.connection()?.get_database_backend();
    let failed: openrust::Result<()> = db
        .transaction(move |transaction| {
            Box::pin(async move {
                let insert = Statement::from_string(
                    backend,
                    format!(
                        "INSERT INTO example_articles (owner_id, title, body) VALUES ({owner_id}, 'Rollback sentinel', 'Must not persist')"
                    ),
                );
                transaction.execute_raw(insert).await.map_err(openrust::Error::internal)?;
                Err(openrust::Error::BadRequest("rollback probe".into()))
            })
        })
        .await;
    assert!(failed.is_err());
    let after_rollback = get(address, path, owner_token).await?;
    assert_eq!(after_rollback.body["meta"]["total"], 2);
    assert_eq!(after_rollback.body["data"][0]["title"], "Article 2");
    #[cfg(feature = "queue")]
    drain_article_index_jobs(app, owner_id, &ids).await?;
    #[cfg(feature = "queue")]
    drain_article_notifications(app, mail, owner_email).await?;
    let events = db
        .connection()?
        .query_one_raw(Statement::from_string(
            backend,
            "SELECT COUNT(*) AS count FROM example_article_events".to_owned(),
        ))
        .await?
        .ok_or("article event count missing")?
        .try_get::<i64>("", "count")?;
    assert_eq!(events, 5);
    db.connection()?
        .execute_raw(Statement::from_string(
            backend,
            "DROP TABLE example_article_events".to_owned(),
        ))
        .await?;
    let failed_event = request(
        address,
        "POST",
        path,
        Some(
            serde_json::json!({"title":"Rollback event sentinel","body":"Must not persist"})
                .to_string(),
        ),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(failed_event.status, StatusCode::INTERNAL_SERVER_ERROR);
    let after_failed_event = get(address, path, owner_token).await?;
    assert_eq!(after_failed_event.body["meta"]["total"], 2);
    Ok(())
}

#[cfg(feature = "metrics")]
async fn assert_article_metrics_after_real_http_request(address: SocketAddr) -> TestResult {
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;
    let response = String::from_utf8(bytes)?;
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("example_article_operations_total"));
    assert!(response.contains("example_article_operation_duration_seconds"));
    for label in [
        "operation=\"create\"",
        "operation=\"update\"",
        "operation=\"delete\"",
    ] {
        assert!(response.contains(label));
    }
    for secret_label in ["owner_id=", "article_id=", "email=", "body=", "token="] {
        assert!(!response.contains(secret_label));
    }
    Ok(())
}

#[cfg(feature = "http-client")]
async fn run_external_summary_flow(
    address: SocketAddr,
    ids: &[i64],
    owner_token: &str,
    other_token: &str,
) -> TestResult {
    let find_id = |remainder| {
        ids.iter()
            .find(|id| id.rem_euclid(3) == remainder)
            .copied()
            .ok_or("article residue missing")
    };
    let success_id = find_id(0)?;
    let failed_id = find_id(1)?;
    let oversized_id = find_id(2)?;
    let success_path = format!("/api/example/articles/{success_id}/summary");
    let foreign = get(address, &success_path, other_token).await?;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND);
    let success = get(address, &success_path, owner_token).await?;
    assert_eq!(success.status, StatusCode::OK);
    assert_eq!(
        success.body["data"]["summary"],
        format!("Summary {success_id}")
    );
    let failed = get(
        address,
        &format!("/api/example/articles/{failed_id}/summary"),
        owner_token,
    )
    .await?;
    assert_eq!(failed.status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        failed.body["code"],
        "example_article_summary_upstream_failed"
    );
    assert!(!failed.body.to_string().contains("127.0.0.1"));
    let oversized = get(
        address,
        &format!("/api/example/articles/{oversized_id}/summary"),
        owner_token,
    )
    .await?;
    assert_eq!(oversized.status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        oversized.body["code"],
        "example_article_summary_upstream_failed"
    );
    Ok(())
}

#[cfg(feature = "queue")]
async fn drain_article_notifications(
    app: &openrust::App,
    mail: &CaptureMailer,
    owner_email: &str,
) -> TestResult {
    let before = mail
        .messages
        .lock()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .len();
    let queue = app.state().resolve::<openrust::queue::Queue>().await?;
    let mut processed = 0;
    while queue
        .run_next_with_state("notifications", app.state())
        .await?
    {
        processed += 1;
        assert!(processed <= 4, "unexpected number of queued notifications");
    }
    assert_eq!(processed, 3);
    let messages = mail
        .messages
        .lock()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    assert_eq!(messages.len(), before + 3);
    for message in &messages[before..] {
        assert!(message.contains(owner_email));
        assert!(message.contains("Article created"));
    }
    Ok(())
}

#[cfg(feature = "encryption")]
#[allow(clippy::too_many_lines)]
async fn run_private_note_flow(
    address: SocketAddr,
    app: &openrust::App,
    ids: &[i64],
    owner_token: &str,
    other_token: &str,
) -> TestResult {
    let path = format!("/api/example/articles/{}/private-note", ids[0]);
    let missing = get(address, &path, owner_token).await?;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let foreign = request(
        address,
        "PUT",
        &path,
        Some(serde_json::json!({"note":"foreign secret"}).to_string()),
        Some(other_token),
        None,
    )
    .await?;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND);
    let foreign_read = get(address, &path, other_token).await?;
    assert_eq!(foreign_read.status, StatusCode::NOT_FOUND);
    let invalid = request(
        address,
        "PUT",
        &path,
        Some(serde_json::json!({"note":"  "}).to_string()),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);

    let secret = "owner-only private note";
    let saved = request(
        address,
        "PUT",
        &path,
        Some(serde_json::json!({"note":secret}).to_string()),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(saved.status, StatusCode::OK);
    assert!(saved.headers.contains("cache-control: private, no-store"));
    let read = get(address, &path, owner_token).await?;
    assert_eq!(read.body["data"]["note"], secret);
    assert!(read.headers.contains("cache-control: private, no-store"));
    let article = get(
        address,
        &format!("/api/example/articles/{}", ids[0]),
        owner_token,
    )
    .await?;
    assert!(article.body["data"].get("note").is_none());
    assert!(
        article.body["data"]
            .get("private_note_ciphertext")
            .is_none()
    );

    let db = app.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    let backend = connection.get_database_backend();
    // 模块业务表由 SeaORM entity 固定命名；框架 table_prefix 只用于框架表。
    let table = match backend {
        openrust::db::DbBackend::Postgres => "\"example_articles\"",
        openrust::db::DbBackend::MySql => "`example_articles`",
        _ => return Err("unsupported article test database".into()),
    };
    let select = match backend {
        openrust::db::DbBackend::Postgres => {
            format!("SELECT private_note_ciphertext FROM {table} WHERE id = $1")
        }
        openrust::db::DbBackend::MySql => {
            format!("SELECT private_note_ciphertext FROM {table} WHERE id = ?")
        }
        _ => return Err("unsupported test backend".into()),
    };
    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            select,
            [ids[0].into()],
        ))
        .await?
        .ok_or("private note row missing")?;
    let ciphertext = row.try_get::<String>("", "private_note_ciphertext")?;
    assert!(!ciphertext.contains(secret));
    let update = match backend {
        openrust::db::DbBackend::Postgres => {
            format!("UPDATE {table} SET private_note_ciphertext = $1 WHERE id = $2")
        }
        openrust::db::DbBackend::MySql => {
            format!("UPDATE {table} SET private_note_ciphertext = ? WHERE id = ?")
        }
        _ => return Err("unsupported test backend".into()),
    };
    connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            update,
            [ciphertext.into(), ids[1].into()],
        ))
        .await?;
    let swapped = get(
        address,
        &format!("/api/example/articles/{}/private-note", ids[1]),
        owner_token,
    )
    .await?;
    assert_eq!(swapped.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(swapped.body["code"], "internal_error");
    assert!(!swapped.body.to_string().contains(secret));
    Ok(())
}

#[cfg(feature = "queue")]
async fn drain_article_index_jobs(app: &openrust::App, owner_id: i64, ids: &[i64]) -> TestResult {
    use modules::example::jobs::article_index::{ARTICLE_STORE, ArticleIndexEntry, cache_key};
    let queue = app.state().resolve::<openrust::queue::Queue>().await?;
    let mut processed = 0;
    while queue.run_next_with_state("example", app.state()).await? {
        processed += 1;
        assert!(processed <= 16, "unexpected number of queued example jobs");
    }
    assert_eq!(processed, 6);
    let cache = app.state().resolve::<openrust::cache::Cache>().await?;
    let store = cache.store(ARTICLE_STORE).await?;
    assert!(
        store
            .get::<ArticleIndexEntry>(&cache_key(owner_id, ids[0]))
            .await?
            .is_none()
    );
    for (id, title) in [(ids[1], "Article 2"), (ids[2], "Article 3")] {
        assert_eq!(
            store
                .get::<ArticleIndexEntry>(&cache_key(owner_id, id))
                .await?
                .ok_or("article index entry missing")?
                .title,
            title
        );
    }
    Ok(())
}

#[cfg(feature = "signing")]
async fn run_share_flow(
    address: SocketAddr,
    detail: &str,
    owner_token: &str,
    other_token: &str,
) -> TestResult {
    let share_path = format!("{detail}/share");
    let foreign_issue = request(
        address,
        "POST",
        &share_path,
        Some("{}".to_owned()),
        Some(other_token),
        None,
    )
    .await?;
    assert_eq!(foreign_issue.status, StatusCode::NOT_FOUND);
    let issued = request(
        address,
        "POST",
        &share_path,
        Some("{}".to_owned()),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(issued.status, StatusCode::OK);
    let token = issued.body["data"]["token"]
        .as_str()
        .ok_or("signed article token missing")?;
    let public_path = format!("/api/example/shared-articles/{token}");
    let shared = request(address, "GET", &public_path, None, None, None).await?;
    assert_eq!(shared.status, StatusCode::OK);
    assert_eq!(shared.body["data"]["title"], "Article 1");
    let foreign_revoke = request(
        address,
        "POST",
        &format!("{share_path}/revoke"),
        Some(serde_json::json!({"token":token}).to_string()),
        Some(other_token),
        None,
    )
    .await?;
    assert_eq!(foreign_revoke.status, StatusCode::UNAUTHORIZED);
    let revoked = request(
        address,
        "POST",
        &format!("{share_path}/revoke"),
        Some(serde_json::json!({"token":token}).to_string()),
        Some(owner_token),
        None,
    )
    .await?;
    assert_eq!(revoked.status, StatusCode::OK);
    let denied = request(address, "GET", &public_path, None, None, None).await?;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
    Ok(())
}

struct HttpResponse {
    status: StatusCode,
    body: serde_json::Value,
    #[cfg(feature = "encryption")]
    headers: String,
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
    request_with_options(address, method, path, body, bearer, forwarded_for, None).await
}

async fn request_with_locale(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<String>,
    bearer: Option<&str>,
    locale: &str,
) -> std::result::Result<HttpResponse, Box<dyn StdError>> {
    request_with_options(address, method, path, body, bearer, None, Some(locale)).await
}

async fn request_with_options(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<String>,
    bearer: Option<&str>,
    forwarded_for: Option<&str>,
    locale: Option<&str>,
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
    let locale = locale.map_or_else(String::new, |locale| {
        format!("Accept-Language: {locale}\r\n")
    });
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{authorization}{forwarded_for}{locale}\r\n{body}",
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
        #[cfg(feature = "encryption")]
        headers: headers.to_ascii_lowercase(),
    })
}

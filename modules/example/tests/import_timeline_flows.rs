//! 文章导入时间线的真实数据库与认证 HTTP 回归。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::{Figment, providers::Serialized};
use openrust::Db;
use openrust::database::sea_orm::{ConnectionTrait, Statement};
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use std::time::Duration;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const TIMELINE: &str = "/api/example/article-imports/timeline";

struct TimelineMigrator;
#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for TimelineMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    _directory: tempfile::TempDir,
    token: String,
    other: String,
}
impl Fixture {
    async fn create() -> std::result::Result<Option<Self>, Box<dyn std::error::Error>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("跳过导入时间线数据库测试：未配置 OPENRUST_TEST_DATABASE_URL");
            return Ok(None);
        };
        let database = TestDatabase::create::<TimelineMigrator>(&admin).await?;
        let directory = tempfile::tempdir()?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source = Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"}, "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"timeline-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":directory.path()}}}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let table = db.quoted_table_name("users")?;
        // 使用真实账户和既有密码登录辅助方法，复用应用当前的认证 Provider。
        for id in [1_i64, 2] {
            let sql = format!(
                "INSERT INTO {table} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'timeline-{id}@example.test','not-used',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"
            );
            connection
                .execute_raw(Statement::from_string(
                    connection.get_database_backend(),
                    sql,
                ))
                .await?;
        }
        Ok(Some(Self {
            token: super::selected_test_auth::seeded_password_access(app.app()?, 1).await?,
            other: super::selected_test_auth::seeded_password_access(app.app()?, 2).await?,
            app,
            database,
            _directory: directory,
        }))
    }
    async fn close(mut self) -> TestResult {
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.database.close().await?;
        Ok(())
    }
}

fn request(
    token: Option<&str>,
) -> std::result::Result<Request<Body>, openrust::http::axum::http::Error> {
    request_uri(token, TIMELINE)
}

fn request_uri(
    token: Option<&str>,
    uri: &str,
) -> std::result::Result<Request<Body>, openrust::http::axum::http::Error> {
    let mut request = Request::get(uri).header("host", "localhost");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request.body(Body::empty())
}

#[tokio::test]
async fn timeline_counts_terminal_boundaries_and_isolates_accounts_over_http() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let observed = chrono::Utc::now().timestamp_millis();
    let future = observed + 86_400_000;
    // 正常下界、确定性上界、逆序、未来、NULL、未终结与另一账户。
    for (index, owner, state, created, terminal) in [
        (0, 1, "completed", 100, Some(100)),
        (1, 1, "failed", 100, Some(observed)),
        (2, 1, "failed", 200, Some(199)),
        (3, 1, "completed", 100, Some(future)),
        (4, 1, "completed", 100, None),
        (5, 1, "open", 100, Some(100)),
        (6, 2, "completed", 100, Some(100)),
        (7, 2, "failed", 200, Some(199)),
        (8, 2, "pending", 100, Some(future)),
    ] {
        let terminal: Option<i64> = terminal;
        db.table("example_article_imports")?
            .insert(vec![
                ("import_id", format!("timeline-{index}").into()),
                ("owner_id", i64::from(owner).into()),
                ("source_key", "timeline-fixture".into()),
                ("format", "csv".into()),
                ("state", state.into()),
                ("created_at_ms", i64::from(created).into()),
                ("terminal_at_ms", terminal.into()),
                ("source_removed", false.into()),
            ])
            .await?;
    }
    let exact = super::services::import_timeline::report(&db, 1, observed).await?;
    assert_eq!((exact.valid_count, exact.invalid_count), (2, 2));
    assert!(exact.has_unfinished_imports);
    assert!(matches!(
        super::services::import_timeline::report(&db, 1, -1).await,
        Err(error) if error.code() == "example_import_clock_invalid"
    ));
    let before = chrono::Utc::now().timestamp_millis();
    let response = fixture.app.request(request(Some(&fixture.token))?).await?;
    response.assert_status(StatusCode::OK)?;
    let report: serde_json::Value = response.json()?;
    assert_eq!(report["data"]["valid_count"], 2);
    assert_eq!(report["data"]["invalid_count"], 2);
    assert_eq!(report["data"]["has_unfinished_imports"], true);
    let sampled = report["data"]["observed_at_ms"]
        .as_i64()
        .ok_or("missing observation time")?;
    assert!(sampled >= before && sampled <= chrono::Utc::now().timestamp_millis());
    let response = fixture.app.request(request(Some(&fixture.other))?).await?;
    response.assert_status(StatusCode::OK)?;
    let report: serde_json::Value = response.json()?;
    assert_eq!(report["data"]["valid_count"], 1);
    assert_eq!(report["data"]["invalid_count"], 1);
    assert_eq!(report["data"]["has_unfinished_imports"], false);
    for (query, valid, invalid, unfinished) in [
        ("search=timeline-0&mode=any", 1, 0, false),
        ("search=timeline-0&mode=all", 0, 0, false),
        ("search=timeline-0&mode=none", 1, 2, true),
        ("search=timeline-&mode=all", 2, 2, true),
        ("search=missing&mode=none", 2, 2, true),
        ("search=timeline-6&mode=any", 0, 0, false),
        ("search=%27%20OR%201%3D1%20--&mode=any", 0, 0, false),
        ("search=&mode=none", 2, 2, true),
    ] {
        let uri = format!("{TIMELINE}?{query}");
        let response = fixture
            .app
            .request(request_uri(Some(&fixture.token), &uri)?)
            .await?;
        response.assert_status(StatusCode::OK)?;
        let report: serde_json::Value = response.json()?;
        assert_eq!(report["data"]["valid_count"], valid, "{query}");
        assert_eq!(report["data"]["invalid_count"], invalid, "{query}");
        assert_eq!(
            report["data"]["has_unfinished_imports"], unfinished,
            "{query}"
        );
    }
    let uri = format!("{TIMELINE}?search={}", "a".repeat(129));
    let response = fixture
        .app
        .request(request_uri(Some(&fixture.token), &uri)?)
        .await?;
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
    let body: serde_json::Value = response.json()?;
    assert!(body["errors"]["search"].is_array());
    fixture
        .app
        .request(request(None)?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    fixture.close().await
}

/// 显式 profile 使用真实应用配置求值，避免把字符串 profile 当对象覆写。
#[cfg(feature = "auth-fortify")]
fn jwt_profile_sources(
    root: &std::path::Path,
    url: &str,
) -> openrust::Result<openrust::config::ConfigSources> {
    use openrust::config::ConfigLoader;
    use serde_json::json;
    super::write_discovery_fixture(root)?;
    for directory in [
        "config",
        "modules/example/resources/views",
        "modules/example/lang",
        "resources/views",
        "bootstrap/cache",
    ] {
        std::fs::create_dir_all(root.join(directory)).map_err(openrust::Error::internal)?;
    }
    std::fs::write(
        root.join("config/cache.toml"),
        "default='redis'\n[stores.redis]\ndriver='memory'\nnamespace='timeline-jwt-api'\n",
    )
    .map_err(openrust::Error::internal)?;
    let source = ConfigLoader::application_code_sources_with_overrides(
        root,
        "TIMELINE_JWT_API_TEST_",
        Some("local"),
        &crate::config::boot().with_toml(),
        [("auth-profile", json!("jwt-api"))],
    )?;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    source.merge_ordered_entries([
        ("application_root", json!(root)),
        ("modules", json!({"activator":"registered","activators":{"file":{"statuses-file":root.join("module-statuses.json")}}})),
        ("app", json!({"env":"local","url":"http://localhost"})),
        ("database", json!({"url":url,"max_connections":4,"min_connections":0})),
        ("session", json!({"driver":"database","secure":false,"lottery":[0,100]})),
        ("security", json!({"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"active_key_id":"test","keys":{"test":key}}})),
        ("auth", json!({"jwt":{"issuer":"http://timeline.example.test","active_key_id":"test","keys":{"test":key}}})),
        #[cfg(feature = "realtime")]
        ("realtime", json!({"allowed_origins":["http://localhost"]})),
    ]).map_err(openrust::Error::from)
}

#[cfg(feature = "auth-fortify")]
async fn assert_no_cookie_capability(app: &TestApp<crate::bootstrap::Server>) -> TestResult {
    #[cfg(feature = "view")]
    assert!(
        !app.app()?
            .state()
            .container()
            .contains::<openrust::view::Views>()
    );
    assert!(
        !app.app()?
            .state()
            .container()
            .contains::<openrust::http::CookieFactory>()
    );
    let cookie_demo = app
        .request(
            Request::get("/api/example/cookie")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    cookie_demo.assert_status(StatusCode::NOT_FOUND)?;
    assert!(cookie_demo.headers().get("set-cookie").is_none());
    Ok(())
}

#[cfg(feature = "auth-fortify")]
#[tokio::test]
async fn jwt_profile_timeline_requires_current_scoped_family_and_sets_no_cookie() -> TestResult {
    use openrust::auth::JwtTokens;
    use openrust::foundation::ProviderId;
    use openrust::http::axum::extract::ConnectInfo;
    use std::net::SocketAddr;
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过显式 JWT 时间线数据库测试：未配置 OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = TestDatabase::create::<TimelineMigrator>(&admin).await?;
    let root = tempfile::tempdir()?;
    let mut builder = TestApp::<crate::bootstrap::Server>::builder(jwt_profile_sources(
        root.path(),
        database.url(),
    )?);
    // 示例网页按 Views 绑定决定是否挂载；纯 API 宿主移除实际视图 Provider。
    #[cfg(feature = "view")]
    {
        builder.remove_provider(ProviderId::of::<openrust::view::OptionalViewServiceProvider>());
        builder.remove_provider(ProviderId::of::<openrust::view::ViewServiceProvider>());
    }
    builder.remove_provider(ProviderId::of::<openrust::http::CookieServiceProvider>());
    #[cfg(feature = "session")]
    builder.remove_provider(ProviderId::of::<openrust::session::SessionServiceProvider>());
    let mut app = builder.build().await?;
    // 共享辅助方法创建真实账户并返回 scope=None 的正式密码家族，用于拒绝回归。
    let unbound = super::selected_test_auth::seeded_password_access(app.app()?, 1).await?;
    assert_no_cookie_capability(&app).await?;
    let db = app.app()?.state().resolve::<Db>().await?;
    db.table("example_article_imports")?
        .insert(vec![
            ("import_id", "timeline-scoped".into()),
            ("owner_id", 1_i64.into()),
            ("source_key", "timeline-fixture".into()),
            ("format", "csv".into()),
            ("state", "completed".into()),
            ("created_at_ms", 100_i64.into()),
            ("terminal_at_ms", 100_i64.into()),
            ("source_removed", false.into()),
        ])
        .await?;
    let login = Request::post("/api/auth/login")
        .header("host", "localhost").header("accept", "application/json")
        .header("content-type", "application/json")
        .extension(ConnectInfo(SocketAddr::from(([127,0,0,1], 41000))))
        .body(Body::from(serde_json::json!({"account":"fixture-1@example.test","password":"selected-protocol-fixture-password"}).to_string()))?;
    let response = app.request(login).await?;
    response.assert_status(StatusCode::OK)?;
    assert!(response.headers().get("set-cookie").is_none());
    // Fortify API 的 Response 直接返回凭据 JSON，与普通业务响应的 data 封装分开。
    let credentials: serde_json::Value = response.json()?;
    let access = credentials["access_token"]
        .as_str()
        .ok_or("缺少 access_token")?;
    let response = app.request(request(Some(access))?).await?;
    response.assert_status(StatusCode::OK)?;
    assert!(response.headers().get("set-cookie").is_none());
    let report: serde_json::Value = response.json()?;
    assert_eq!(report["data"]["valid_count"], 1);
    assert_eq!(report["data"]["invalid_count"], 0);
    assert_eq!(report["data"]["has_unfinished_imports"], false);
    let bare = app
        .app()?
        .state()
        .resolve::<JwtTokens>()
        .await?
        .issue_access_token(1)?;
    for token in [&unbound, &bare] {
        let response = app.request(request(Some(token))?).await?;
        response.assert_status(StatusCode::UNAUTHORIZED)?;
        assert!(response.headers().get("set-cookie").is_none());
    }
    let response = app.request(request(None)?).await?;
    response.assert_status(StatusCode::UNAUTHORIZED)?;
    assert!(response.headers().get("set-cookie").is_none());
    #[cfg(feature = "session")]
    assert!(
        !app.app()?
            .state()
            .container()
            .contains::<openrust::session::SessionStoreHandle>()
    );
    assert_eq!(db.table("sessions")?.count().await?, 0);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}

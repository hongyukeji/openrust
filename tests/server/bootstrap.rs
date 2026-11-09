//! 默认宿主的路由、健康诊断和配置验收。

use super::*;
use figment::{Figment, providers::Serialized};
use openrust::ServiceExt;
use std::fmt::Write as _;

pub(crate) fn test_config_sources(figment: &Figment) -> Result<openrust::config::ConfigSources> {
    let overrides = figment
        .extract::<serde_json::Map<String, serde_json::Value>>()
        .map_err(openrust::config::ConfigError::from)?;
    Ok(
        openrust::config::ConfigLoader::application_code_sources_with_overrides(
            env!("CARGO_MANIFEST_DIR"),
            "APP_",
            None,
            &Server::config(),
            overrides,
        )?,
    )
}

#[tokio::test]
async fn default_provider_manifest_serves_application_route() -> Result<()> {
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
        "app": {"env": "test"},
        "database": {"url": null},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    })));
    let sources = super::test_config_sources(&source)?;
    let configuration = sources
        .figment()
        .extract::<Configuration>()
        .map_err(openrust::config::ConfigError::from)?;
    let builder = openrust::foundation::ProviderBuilder::for_application::<Server>(&configuration);
    let runtime = builder.start(sources).await?;
    let app = openrust::Application::from_provider_runtime(runtime)?;
    let dispatcher = app
        .state()
        .resolve::<openrust::events::Dispatcher>()
        .await?;
    let named = app
        .state()
        .resolve_key::<openrust::events::Dispatcher>(openrust::container::ServiceKey::named(
            "events",
        ))
        .await?;
    assert!(std::sync::Arc::ptr_eq(&dispatcher, &named));
    // 无driver/无URL仍注册通用迁移命令；具体执行必须通过未初始化Db明确失败。
    assert!(app.commands().any(|command| command.name() == "migrate"));
    let db: std::sync::Arc<openrust::prelude::Db> = app.state().resolve().await?;
    assert!(db.backend().is_err());
    let router = openrust::foundation::http::build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            openrust::http::axum::http::Request::builder()
                .uri("/api")
                .header("host", "localhost")
                .body(openrust::http::axum::body::Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(
        response.status(),
        openrust::http::axum::http::StatusCode::OK
    );
    let body = openrust::http::axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(openrust::Error::internal)?;
    assert_eq!(json, application_identity("OpenRust"));
    let response = router
        .oneshot(
            openrust::http::axum::http::Request::builder()
                .uri("/up")
                .header("host", "localhost")
                .header("accept", "application/json")
                .body(openrust::http::axum::body::Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(
        response.status(),
        openrust::http::axum::http::StatusCode::OK
    );
    let body = openrust::http::axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).map_err(openrust::Error::internal)?,
        serde_json::json!({"status": "up"})
    );
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

struct HealthFailureReporter(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl ServiceProvider for HealthFailureReporter {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use openrust::http::axum::{http::StatusCode, response::IntoResponse as _};
            let reported = std::sync::Arc::clone(&self.0);
            app.exceptions(|handler| {
                handler
                    .report::<std::io::Error, _>(move |_| {
                        reported.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        openrust::foundation::ReportDecision::Stop
                    })
                    .render::<std::io::Error, _>(|_, _| {
                        Some((StatusCode::INTERNAL_SERVER_ERROR, "debug-rendered").into_response())
                    });
            })?;
            app.routes(|routes| routes.get("/healthz", openrust::http::health::diagnose))?;
            Ok(())
        })
    }
}

fn health_request(path: &str, accept: &str) -> Result<openrust::http::axum::extract::Request> {
    use openrust::http::axum::{body::Body, http::Request};

    Request::get(path)
        .header("host", "localhost")
        .header("accept", accept)
        .body(Body::empty())
        .map_err(openrust::Error::internal)
}

#[tokio::test]
async fn application_health_reports_failure_and_delegates_debug_rendering() -> Result<()> {
    use openrust::events::EventsServiceProvider;
    use openrust::foundation::events::DiagnosingHealth;
    use openrust::http::axum::http::StatusCode;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    for debug in [false, true] {
        let report_count = Arc::new(AtomicUsize::new(0));
        let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
            .merge(Serialized::defaults(serde_json::json!({
                "app": {"env": "testing", "name": "<diagnostics>", "debug": debug},
                "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
                "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
            })));
        let mut builder =
            openrust::testing::TestApp::<Server>::builder(super::test_config_sources(&source)?);
        let reported = Arc::clone(&report_count);
        builder.configure_providers(move |providers| {
            let mut listeners = EventsServiceProvider::new();
            listeners.listen_named::<DiagnosingHealth, _>(
                r"Illuminate\Foundation\Events\DiagnosingHealth",
                |_, _| {
                    Box::pin(async {
                        Err(openrust::Error::internal(std::io::Error::other(
                            "private diagnostic detail",
                        )))
                    })
                },
            );
            providers.force_provider(listeners);
            providers.provider(HealthFailureReporter(Arc::clone(&reported)));
        });
        let mut app = builder.build().await?;
        assert_eq!(app.app()?.config().app.debug, debug);
        assert_eq!(app.app()?.config().app.name, "<diagnostics>");
        let path = if debug { "/healthz" } else { "/up" };
        let response = app
            .request(health_request(path, "application/json")?)
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{}",
            String::from_utf8_lossy(response.body())
        );
        assert_eq!(report_count.load(Ordering::Relaxed), 1);
        if debug {
            assert_eq!(response.body(), b"debug-rendered");
        } else {
            assert_eq!(
                response.json::<serde_json::Value>()?,
                serde_json::json!({"status": "down"})
            );
            assert_eq!(
                response
                    .headers()
                    .get("cache-control")
                    .and_then(|value| value.to_str().ok()),
                Some("no-cache, private"),
                "JSON 健康诊断响应"
            );
            let html = app.request(health_request("/up", "text/html")?).await?;
            assert_eq!(html.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let text = std::str::from_utf8(html.body()).map_err(openrust::Error::internal)?;
            assert!(
                text.contains("<title>&lt;diagnostics&gt;</title>"),
                "测试夹具的 HTML 诊断响应为 {text}"
            );
            assert!(text.contains("Application experiencing problems"));
            assert!(!text.contains("private diagnostic detail"));
            assert_eq!(
                html.headers()
                    .get("cache-control")
                    .and_then(|value| value.to_str().ok()),
                Some("no-cache, private"),
                "HTML 健康诊断响应"
            );
            assert_eq!(report_count.load(Ordering::Relaxed), 2);
        }
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
    }
    Ok(())
}

fn application_identity(name: &str) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "code": "ok",
        "message": "ok",
        "data": {"name": name, "version": env!("CARGO_PKG_VERSION")},
    })
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
#[ignore = "需要真实 PostgreSQL/MySQL 管理连接，创建并清理自有隔离库"]
async fn named_only_database_keeps_default_services_unselected() -> Result<()> {
    let admin = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(openrust::Error::internal)?;
    let database =
        openrust::testing::TestDatabase::create::<openrust::database::FrameworkMigrator>(&admin)
            .await?;
    // 将创建后的全部错误纳入统一清理；失败不能只依赖 Drop 的尽力清理。
    let outcome = verify_named_only_database_host(&database).await;
    let cleanup = database.close().await;
    outcome.and(cleanup)
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn verify_named_only_database_host(database: &openrust::testing::TestDatabase) -> Result<()> {
    use openrust::http::axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    fn check(condition: bool) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(openrust::Error::custom(
                "test_named_database_selection",
                "真实具名库没有遵守默认连接隔离",
            ))
        }
    }
    let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(serde_json::json!({
        "application_root": root.path(),
        "app": {"env": "testing"},
        "database": {"url": null, "instances": {"reporting": {"url": database.url()}}},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {"active_key_id": "test", "keys": {
            "test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
        }}}
    })));
    let mut app =
        openrust::testing::TestApp::<Server>::builder(super::test_config_sources(&source)?)
            .build()
            .await?;
    let outcome: Result<()> = async {
        let databases = app.app()?.state().resolve::<openrust::Databases>().await?;
        check(databases.names() == ["default", "reporting"])?;
        check(databases.default_database().backend().is_err())?;
        databases
            .get("reporting")?
            .connection()?
            .ping()
            .await
            .map_err(openrust::Error::internal)?;
        #[cfg(feature = "auth")]
        {
            let container = app.app()?.state().container();
            // Hashing 独立于数据库认证服务；无默认库时仍登记管理器及默认驱动契约。
            check(
                container.contains::<openrust::hashing::HashManager>()
                    && container.contains::<dyn openrust::hashing::Hasher>()
                    && !container.contains::<openrust::auth::RefreshTokens>()
                    && !container.contains::<openrust::auth::PersonalAccessTokens>()
                    && !container.contains::<openrust::auth::Policy>(),
            )?;
        }
        #[cfg(feature = "queue")]
        check(
            app.app()?
                .state()
                .container()
                .contains::<openrust::queue::Queue>(),
        )?;
        check(
            app.app()?
                .commands()
                .any(|command| command.name() == "migrate"),
        )?;
        for (uri, status) in [
            ("/api", StatusCode::OK),
            ("/api/example/articles", StatusCode::NOT_FOUND),
        ] {
            app.request(
                Request::get(uri)
                    .header("host", "localhost")
                    .body(Body::empty())
                    .map_err(openrust::Error::internal)?,
            )
            .await?
            .assert_status(status)?;
        }
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome.and(shutdown)
}

#[cfg(feature = "queue")]
#[test]
fn queue_redis_defaults_ignore_cache_store_overrides() -> Result<()> {
    let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
    std::fs::write(
        root.path().join(".env"),
        "REDIS_HOST=initial.example.test\n",
    )
    .map_err(openrust::Error::internal)?;
    let source = openrust::config::ConfigLoader::application_code_sources_with_overrides(
        root.path(),
        "QUEUE_DEPENDENCY_TEST_",
        Some("testing"),
        &crate::config::boot(),
        [(
            "cache",
            serde_json::json!({"stores":{"redis":{
                "driver":"redis","dsn":"redis://effective.example.test:6381/6"
            }}}),
        )],
    )?;
    let queue: openrust::config::QueueConfig = source
        .figment()
        .extract_inner("queue")
        .map_err(openrust::config::ConfigError::from)?;
    assert_eq!(queue.default, "database");
    assert_eq!(
        queue.connections["redis"]["redis_url"],
        "redis://initial.example.test:6379/0"
    );
    let cache: serde_json::Value = source
        .figment()
        .extract_inner("cache")
        .map_err(openrust::config::ConfigError::from)?;
    assert_eq!(
        cache["stores"]["redis"]["dsn"],
        "redis://effective.example.test:6381/6"
    );
    assert_eq!(queue.connections["database"]["driver"], "database");
    let explicit = openrust::config::ConfigLoader::application_code_sources_with_overrides(
        root.path(),
        "QUEUE_DEPENDENCY_TEST_",
        Some("testing"),
        &crate::config::boot(),
        [
            (
                "cache",
                serde_json::json!({"stores":{"redis":{"driver":"redis",
                "dsn":"redis://effective.example.test:6381/6"}}}),
            ),
            (
                "queue",
                serde_json::json!({"default":"redis","connections":{"redis":{
                    "redis_url":"redis://independent.example.test:6382/7"
                }}}),
            ),
        ],
    )?;
    let queue: openrust::config::QueueConfig = explicit
        .figment()
        .extract_inner("queue")
        .map_err(openrust::config::ConfigError::from)?;
    assert_eq!(queue.default, "redis");
    assert_eq!(
        queue.connections["redis"]["redis_url"],
        "redis://independent.example.test:6382/7"
    );
    Ok(())
}

fn registered_sources(
    root: &std::path::Path,
    prefix: &str,
) -> Result<openrust::config::ConfigSources> {
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
            "application_root": root,
            "modules": {"activator":"registered"},
            "api": {"prefix":prefix},
            "app":{"env":"test"},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })));
    super::test_config_sources(&source)
}

fn registered_root() -> Result<tempfile::TempDir> {
    tempfile::tempdir().map_err(openrust::Error::internal)
}

#[tokio::test]
async fn configured_api_prefix_changes_mount_and_preserves_api_errors() -> Result<()> {
    use openrust::http::axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    for (prefix, endpoint, missing) in [
        ("api", "/api", "/api/missing"),
        ("api/admin", "/api/admin", "/api/admin/missing"),
        ("/api/admin/", "/api/admin", "/api/admin/missing"),
        ("//api/admin//", "/api/admin", "/api/admin/missing"),
        ("", "/", "/missing"),
        (
            "tenant/{tenant}/api",
            "/tenant/acme/api",
            "/tenant/acme/api/missing",
        ),
    ] {
        let root = registered_root()?;
        let mut app =
            openrust::testing::TestApp::<Server>::builder(registered_sources(root.path(), prefix)?)
                .build()
                .await?;
        let outcome: Result<()> = async {
            app.request(
                Request::get(endpoint)
                    .header("host", "localhost")
                    .body(Body::empty())
                    .map_err(openrust::Error::internal)?,
            )
            .await?
            .assert_status(StatusCode::OK)?
            .assert_json(&application_identity("OpenRust"))?;
            {
                let path = missing;
                let response = app
                    .request(
                        Request::get(path)
                            .header("host", "localhost")
                            .body(Body::empty())
                            .map_err(openrust::Error::internal)?,
                    )
                    .await?;
                response.assert_status(StatusCode::NOT_FOUND)?;
                let json: serde_json::Value = response.json()?;
                assert_eq!(json["success"], false);
            }
            if !prefix.is_empty() {
                let response = app
                    .request(
                        Request::get(format!("{endpoint}2/missing"))
                            .header("host", "localhost")
                            .header("accept", "application/json")
                            .body(Body::empty())
                            .map_err(openrust::Error::internal)?,
                    )
                    .await?;
                assert!(
                    response
                        .json::<serde_json::Value>()?
                        .get("success")
                        .is_none()
                );
                if endpoint != "/api" {
                    app.request(
                        Request::get("/api")
                            .header("host", "localhost")
                            .body(Body::empty())
                            .map_err(openrust::Error::internal)?,
                    )
                    .await?
                    .assert_status(StatusCode::NOT_FOUND)?;
                }
            }
            Ok(())
        }
        .await;
        let cleanup = app
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await;
        outcome?;
        cleanup?;
    }
    Ok(())
}

#[tokio::test]
async fn invalid_prefix_is_rejected_during_real_bootstrap() -> Result<()> {
    let root = registered_root()?;
    let mut oversized = String::new();
    for n in 0..26 {
        write!(&mut oversized, "/{{p{n}}}").map_err(openrust::Error::internal)?;
    }
    for prefix in ["/bad?private-token", oversized.as_str()] {
        let result =
            openrust::testing::TestApp::<Server>::builder(registered_sources(root.path(), prefix)?)
                .build()
                .await;
        assert!(
            matches!(result, Err(error) if error.to_string().contains("api.prefix") && !error.to_string().contains("private-token"))
        );
    }
    Ok(())
}

#[test]
fn editable_hashing_defaults_and_dotenv_share_one_configuration() -> Result<()> {
    let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
    let manifest =
        openrust::config::ConfigManifest::new().section("hashing", crate::config::hashing::config);
    let load = || {
        openrust::config::ConfigLoader::application_code_sources_with_overrides(
            root.path(),
            "HASHING_DEFAULTS_TEST_",
            Some("testing"),
            &manifest,
            std::iter::empty::<(&str, serde_json::Value)>(),
        )
    };
    let defaults: openrust::config::HashingConfig = load()?
        .figment()
        .extract_inner("hashing")
        .map_err(openrust::config::ConfigError::from)?;
    assert_eq!(defaults.driver, "bcrypt");
    assert_eq!(defaults.bcrypt.rounds, 12);
    assert!(defaults.bcrypt.verify);
    assert_eq!(defaults.bcrypt.limit, None);
    assert_eq!(
        (
            defaults.argon.memory,
            defaults.argon.time,
            defaults.argon.threads
        ),
        (65_536, 4, 1)
    );
    std::fs::write(root.path().join(".env"),
        "HASH_DRIVER=argon2id\nBCRYPT_ROUNDS=5\nHASH_VERIFY=false\nBCRYPT_LIMIT=8\nARGON_MEMORY=2048\nARGON_TIME=2\nARGON_THREADS=2\n")
        .map_err(openrust::Error::internal)?;
    let configured: openrust::config::HashingConfig = load()?
        .figment()
        .extract_inner("hashing")
        .map_err(openrust::config::ConfigError::from)?;
    assert_eq!(configured.driver, "argon2id");
    assert_eq!(configured.bcrypt.rounds, 5);
    assert_eq!(configured.bcrypt.limit, Some(8));
    assert!(!configured.bcrypt.verify && !configured.argon.verify);
    assert_eq!(
        (
            configured.argon.memory,
            configured.argon.time,
            configured.argon.threads
        ),
        (2048, 2, 2)
    );
    std::fs::write(
        root.path().join(".env"),
        "BCRYPT_ROUNDS=secret-invalid-cost\n",
    )
    .map_err(openrust::Error::internal)?;
    let Err(error) = load() else {
        return Err(openrust::Error::internal(std::io::Error::other(
            "invalid hashing environment was accepted",
        )));
    };
    assert!(error.to_string().contains("求值失败"));
    assert!(!error.to_string().contains("secret-invalid-cost"));
    Ok(())
}

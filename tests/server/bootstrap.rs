//! 默认宿主的真实路由与模块运行资源根验收。

use super::*;
use figment::{Figment, providers::Serialized};
use openrust::ServiceExt;
use openrust_modules::ModuleTestAppBuilderExt as _;
use std::fmt::Write as _;

#[tokio::test]
async fn default_provider_manifest_serves_application_route() -> Result<()> {
    let builder =
        openrust::core::ProviderBuilder::for_application::<Server>(&Configuration::default());
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
        "app": {"env": "test"},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    })));
    let runtime = builder
        .start(crate::modules::example::test_sources(&(source))?)
        .await?;
    let app = openrust::App::from_provider_runtime(runtime)?;
    // 无driver/无URL仍注册通用迁移命令；具体执行必须通过未初始化Db明确失败。
    assert!(app.commands().any(|command| command.name() == "migrate"));
    let db: std::sync::Arc<openrust::prelude::Db> = app.state().resolve().await?;
    assert!(db.backend().is_err());
    let router = openrust::http::build_router(&app)?;
    let response = router
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
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn module_resources_follow_configured_runtime_root() -> Result<()> {
    use openrust::http::axum::{body::Body, http::Request};

    let deployed = tempfile::tempdir().map_err(openrust::Error::internal)?;
    crate::modules::example::write_discovery_fixture(deployed.path())?;
    let module = deployed.path().join("modules/example");
    std::fs::create_dir_all(module.join("resources/views")).map_err(openrust::Error::internal)?;
    std::fs::create_dir_all(deployed.path().join("resources/lang/vendor/example"))
        .map_err(openrust::Error::internal)?;
    std::fs::create_dir_all(deployed.path().join("state")).map_err(openrust::Error::internal)?;
    // 默认位置故意关闭；若启动忽略独立状态路径，下面真实模块路由断言会失败。
    std::fs::write(
        deployed.path().join("modules_statuses.json"),
        "{\"example\":false}\n",
    )
    .map_err(openrust::Error::internal)?;
    std::fs::write(
        deployed.path().join("state/modules.json"),
        "{\"example\":true}\n",
    )
    .map_err(openrust::Error::internal)?;
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
            "application_root": deployed.path(),
            "modules": {"activator": "file", "activators": {"file": {"statuses-file": "state/modules.json"}}},
            "app": {"env": "test"},
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })));
    let mut app = openrust::testing::TestApp::<Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    )
    .build()
    .await?;
    let response = app
        .request(
            Request::get("/api/example")
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await?;
    let json: serde_json::Value = response.json()?;
    assert_eq!(json["data"]["message"], "Hello from modules/example");
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await
}

// 完整 envelope 断言可发现控制器误返回裸 JSON、丢字段或硬编码应用名称。
fn application_identity(name: &str) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "code": "ok",
        "message": "ok",
        "data": {"name": name, "version": env!("CARGO_PKG_VERSION")},
    })
}

#[tokio::test]
async fn disabled_example_keeps_application_api_and_removes_module_api() -> Result<()> {
    use openrust::http::axum::{
        body::Body,
        http::{Request, StatusCode},
    };

    let deployed = tempfile::tempdir().map_err(openrust::Error::internal)?;
    crate::modules::example::write_discovery_fixture(deployed.path())?;
    std::fs::create_dir_all(deployed.path().join("modules/example"))
        .map_err(openrust::Error::internal)?;
    std::fs::write(
        deployed.path().join("modules_statuses.json"),
        "{\"example\":false}\n",
    )
    .map_err(openrust::Error::internal)?;
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(serde_json::json!({
        "application_root": deployed.path(),
        "app": {"name": "Host without example", "env": "test"},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {"active_key_id": "test", "keys": {
            "test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
        }}}
    })));
    let mut app = openrust::testing::TestApp::<Server>::builder(
        crate::modules::example::test_sources(&source)?,
    )
    .build()
    .await?;
    let outcome: Result<()> = async {
        let response = app
            .request(
                Request::get("/api")
                    .header("host", "localhost")
                    .body(Body::empty())
                    .map_err(openrust::Error::internal)?,
            )
            .await?;
        response
            .assert_status(StatusCode::OK)?
            .assert_json(&application_identity("Host without example"))?;
        // API 组保持无状态，不因根应用路由迁移自动启动 Web Session。
        assert!(!response.headers().contains_key("set-cookie"));
        app.request(
            Request::get("/api/example")
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
#[ignore = "需要真实 PostgreSQL/MySQL 管理连接，创建并清理自有隔离库"]
async fn named_only_database_keeps_default_services_unselected() -> Result<()> {
    let admin = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(openrust::Error::internal)?;
    let database =
        openrust::testing::TestDatabase::create::<openrust::db::FrameworkMigrator>(&admin).await?;
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
    crate::modules::example::write_discovery_fixture(root.path())?;
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
    let mut app = openrust::testing::TestApp::<Server>::builder(
        crate::modules::example::test_sources(&source)?,
    )
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
fn queue_defaults_inherit_effective_cache_configuration() -> Result<()> {
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
    crate::modules::example::test_sources(&source)
}

fn registered_root() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
    for directory in ["modules/example/resources/views", "modules/example/lang"] {
        std::fs::create_dir_all(root.path().join(directory)).map_err(openrust::Error::internal)?;
    }
    Ok(root)
}

#[tokio::test]
async fn registered_module_requires_no_json_and_initializes_after_business() -> Result<()> {
    use openrust::http::axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    let root = registered_root()?;
    let mut app =
        openrust::testing::TestApp::<Server>::builder(registered_sources(root.path(), "/api")?)
            .build()
            .await?;
    let outcome: Result<()> = async {
        let repository = app.app()?.state().resolve::<openrust_modules::ModuleRepository>().await?;
        assert!(repository.find_or_fail("example")?.is_enabled()?);
        assert!(repository.statuses_file_path().is_none());
        assert_eq!(repository.get_path()?, root.path().join("modules"));
        app.request(Request::get("/api/example/module-initialization").header("host", "localhost").body(Body::empty()).map_err(openrust::Error::internal)?)
            .await?.assert_status(StatusCode::OK)?.assert_json(&serde_json::json!({"success":true,"code":"ok","message":"ok","data":{"initialized":true,"business_registered":true}}))?;
        assert!(!root.path().join("modules_statuses.json").exists());
        assert!(!root.path().join("modules/example/module.json").exists());
        Ok(())
    }.await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn configured_api_prefix_changes_mount_and_preserves_module_errors() -> Result<()> {
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
            for path in [missing, "/api/example/missing"] {
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

#[tokio::test]
async fn removing_registered_module_keeps_host_without_initialization_routes() -> Result<()> {
    use openrust::http::axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    let root = registered_root()?;
    let mut builder =
        openrust::testing::TestApp::<Server>::builder(registered_sources(root.path(), "/v1")?);
    builder.remove_module("example");
    let mut app = builder.build().await?;
    let outcome: Result<()> = async {
        app.request(
            Request::get("/v1")
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(openrust::Error::internal)?,
        )
        .await?
        .assert_status(StatusCode::OK)?;
        for path in ["/api/example", "/api/example/module-initialization"] {
            app.request(
                Request::get(path)
                    .header("host", "localhost")
                    .body(Body::empty())
                    .map_err(openrust::Error::internal)?,
            )
            .await?
            .assert_status(StatusCode::NOT_FOUND)?;
        }
        let repository = app
            .app()?
            .state()
            .resolve::<openrust_modules::ModuleRepository>()
            .await?;
        assert!(!repository.find_or_fail("example")?.is_loaded());
        Ok(())
    }
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup
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

use std::{collections::BTreeMap, path::Path};

use openrust::{
    config::{Environment, Value},
    contracts::{foundation::Application as _, session::Session as _},
    foundation::bootstrap::LoadEnvironmentVariables,
    testing::TestApp,
};
use serde_json::json;

#[path = "support/config_app.rs"]
mod app;

#[path = "../config/mod.rs"]
mod config;

#[cfg(windows)]
#[test]
fn canonical_windows_storage_paths_accept_portable_directory_suffixes() {
    let app = TestApp::create(|builder| builder).unwrap();
    let path = app.app().storage_path("framework/views");
    std::fs::create_dir_all(&path).unwrap();
    let probe = path.join("probe.txt");
    std::fs::write(&probe, "canonical Windows path").unwrap();
    assert_eq!(
        std::fs::read_to_string(probe).unwrap(),
        "canonical Windows path"
    );
    assert_eq!(path, app.app().base_path().join("storage/framework/views"));
}

fn assert_paths(values: &BTreeMap<String, Value>, storage: &Path) {
    assert_eq!(
        values["logging"]["channels"]["single"]["path"],
        json!(storage.join("logs/app.log"))
    );
    assert_eq!(
        values["logging"]["channels"]["emergency"]["path"],
        json!(storage.join("logs/app.log"))
    );
    assert_eq!(
        values["session"]["files"],
        json!(storage.join("framework/sessions"))
    );
    assert_eq!(
        values["cache"]["stores"]["file"]["path"],
        json!(storage.join("framework/cache/data"))
    );
    assert_eq!(
        values["cache"]["stores"]["file"]["lock_path"],
        values["cache"]["stores"]["file"]["path"]
    );
    assert_eq!(
        values["view"]["compiled"],
        json!(storage.join("framework/views"))
    );
    assert_eq!(
        values["filesystems"]["disks"]["local"]["root"],
        json!(storage.join("app/private"))
    );
    assert_eq!(
        values["filesystems"]["disks"]["public"]["root"],
        json!(storage.join("app/public"))
    );
}

#[tokio::test]
async fn storage_configuration_tracks_its_own_application_without_a_global_facade() {
    let mut first = TestApp::create(|builder| builder.with_config(config::load)).unwrap();
    let mut second = TestApp::create(|builder| builder.with_config(config::load)).unwrap();
    let one = first.root().join("one");
    let two = second.root().join("two");
    first.app().use_storage_path(&one).unwrap();
    second.app().use_storage_path(&two).unwrap();
    tokio::try_join!(first.boot(), second.boot()).unwrap();
    let env_a = first.app().make::<Environment>().await.unwrap();
    let env_b = second.app().make::<Environment>().await.unwrap();
    assert_paths(&config::load(first.root(), &env_a).unwrap(), &one);
    assert_paths(&config::load(second.root(), &env_b).unwrap(), &two);
    let a_config = first.app().config().unwrap();
    assert_eq!(
        a_config.get("session.files"),
        Some(json!(one.join("framework/sessions")))
    );
    let late = first.root().join("late");
    first.app().use_storage_path(&late).unwrap();
    assert_paths(&config::load(first.root(), &env_a).unwrap(), &late);
    assert_eq!(
        a_config.get("session.files"),
        Some(json!(one.join("framework/sessions")))
    );
    let root = first.root().to_owned();
    drop(first);
    // A retained environment does not keep the application/path service alive.
    assert_eq!(env_a.storage_path(&root, "logs"), root.join("storage/logs"));
}

#[tokio::test]
async fn standalone_configuration_uses_its_snapshot_and_preserves_specific_overrides() {
    let app = TestApp::create(|builder| builder).unwrap();
    let custom = app.root().join("自定义");
    std::fs::write(
        app.root().join(".env"),
        format!("APP_STORAGE_PATH='{}'\n", custom.display()),
    )
    .unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(app.root(), None).unwrap();
    let values = config::load(app.root(), &environment).unwrap();
    assert_paths(&values, &custom);
    assert_eq!(
        values["filesystems"]["links"][app.root().join("public/storage").to_str().unwrap()],
        json!(custom.join("app/public"))
    );
    std::fs::write(
        app.root().join(".env"),
        format!(
            "APP_STORAGE_PATH='{}'\nFILESYSTEM_PUBLIC_ROOT=uploads\nVIEW_COMPILED_PATH=compiled\n",
            custom.display()
        ),
    )
    .unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(app.root(), None).unwrap();
    let values = config::load(app.root(), &environment).unwrap();
    assert_eq!(values["view"]["compiled"], "compiled");
    assert_eq!(
        values["filesystems"]["disks"]["public"]["root"],
        json!(app.root().join("uploads"))
    );
    assert_eq!(
        values["filesystems"]["links"][app.root().join("public/storage").to_str().unwrap()],
        json!(app.root().join("uploads"))
    );
}

#[tokio::test]
async fn provider_fallbacks_use_the_application_paths_and_keep_explicit_log_paths() {
    use openrust::log::{LogContext, LogManager};
    let mut app = TestApp::create(|builder| builder).unwrap();
    let storage = app.root().join("fallback-storage");
    app.app().use_storage_path(&storage).unwrap();
    std::fs::create_dir_all(app.root().join("resources/views")).unwrap();
    std::fs::write(
        app.root().join("resources/views/probe.blade.html"),
        "{{ $label }}",
    )
    .unwrap();
    app.boot().await.unwrap();
    let logger = app.app().make::<LogManager>().await.unwrap();
    logger
        .info("fallback storage log", &LogContext::new())
        .unwrap();
    logger
        .build(&json!({"driver":"single", "path":"explicit.log"}))
        .unwrap()
        .info("explicit base path", &LogContext::new())
        .unwrap();
    assert!(
        std::fs::read_to_string(storage.join("logs/app.log"))
            .unwrap()
            .contains("fallback storage log")
    );
    assert!(app.root().join("explicit.log").exists());
    let views = app.app().make::<openrust::view::Factory>().await.unwrap();
    assert_eq!(
        views
            .render(
                "probe",
                &openrust::view::ViewData::from_iter([("label".into(), "rendered".into())])
            )
            .unwrap(),
        "rendered"
    );
    assert!(
        std::fs::read_dir(storage.join("framework/views"))
            .unwrap()
            .next()
            .is_some()
    );
    // Missing configured links use the same application path service.
    #[cfg(unix)]
    {
        std::fs::create_dir_all(app.root().join("public")).unwrap();
        std::fs::create_dir_all(storage.join("app/public")).unwrap();
        app.artisan(["storage:link"]).await.assert_successful();
        assert_eq!(
            std::fs::read_link(app.root().join("public/storage")).unwrap(),
            storage.join("app/public")
        );
        app.artisan(["storage:unlink"]).await.assert_successful();
    }
}

fn session_fallback_routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/session-fallback", session_fallback_response)?;
    Ok(())
}

fn session_fallback_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        request.session_mut()?.put("probe", json!(true))?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, json!({"stored":true}))
    })
}

#[tokio::test]
async fn file_session_fallback_tracks_path_changes_between_requests() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok([
                    (
                        "app".into(),
                        json!({"env":"testing", "debug":true, "url":"http://localhost"}),
                    ),
                    ("session".into(), json!({"driver":"file"})),
                ]
                .into())
            })
            .with_middleware(|middleware| {
                middleware.group(
                    "web",
                    vec![
                        openrust::routing::MiddlewareEntry::Instance(std::sync::Arc::new(
                            openrust::cookie::middleware::EncryptCookies::new(std::sync::Arc::new(
                                openrust::encryption::Encrypter::new(
                                    [b'a'; 32],
                                    openrust::encryption::Cipher::Aes256Cbc,
                                )
                                .unwrap(),
                            )),
                        )),
                        openrust::routing::MiddlewareEntry::Instance(std::sync::Arc::new(
                            openrust::cookie::middleware::AddQueuedCookiesToResponse,
                        )),
                        openrust::routing::MiddlewareEntry::Instance(std::sync::Arc::new(
                            openrust::session::middleware::StartSession,
                        )),
                    ],
                );
            })
            .with_routing(
                openrust::foundation::configuration::Routing::default()
                    .web(session_fallback_routes),
            )
    })
    .unwrap();
    let first = app.root().join("session-one");
    let second = app.root().join("session-two");
    app.app().use_storage_path(&first).unwrap();
    let response = app.get("/session-fallback").await.unwrap();
    assert_eq!(
        response.status(),
        openrust::http::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    assert!(
        std::fs::read_dir(first.join("framework/sessions"))
            .unwrap()
            .next()
            .is_some()
    );
    app.app().use_storage_path(&second).unwrap();
    let response = app.get("/session-fallback").await.unwrap();
    assert_eq!(
        response.status(),
        openrust::http::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    assert!(
        std::fs::read_dir(second.join("framework/sessions"))
            .unwrap()
            .next()
            .is_some()
    );
    assert!(!app.root().join("storage/framework/sessions").exists());
}

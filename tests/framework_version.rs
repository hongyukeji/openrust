use openrust::{contracts::foundation::Application as _, foundation::Application};

#[allow(dead_code)]
#[path = "../config/app.rs"]
mod app_config;
#[allow(dead_code)]
#[path = "../config/env.rs"]
mod env;

#[tokio::test]
async fn application_package_version_and_env_override_do_not_change_framework_version() {
    for override_version in [None, Some("9.8.7-app")] {
        let root = std::env::temp_dir().join(format!(
            "framework-version-{}-{}",
            std::process::id(),
            openrust::support::Str::random(8)
        ));
        std::fs::create_dir_all(&root).unwrap();
        if let Some(version) = override_version {
            std::fs::write(root.join(".env"), format!("APP_VERSION={version}\n")).unwrap();
        }
        let mut application = Application::configure(root.clone())
            .with_config(|_, environment| {
                Ok(std::collections::BTreeMap::from([(
                    "app".into(),
                    app_config::load(environment)?.config,
                )]))
            })
            .create()
            .unwrap();
        application.boot().await.unwrap();
        assert_eq!(
            application.config().unwrap().get("app.version"),
            Some(override_version.unwrap_or(env!("CARGO_PKG_VERSION")).into())
        );
        assert_eq!(application.version(), Application::VERSION);
        drop(application);
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn version_response(
    _request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            serde_json::json!({
                "framework": openrust::support::facades::App::version().unwrap(),
                "application": openrust::support::facades::Config::get("app.version").unwrap(),
            }),
        )
    })
}

#[tokio::test]
async fn http_reports_independent_application_and_framework_versions_with_cached_configuration() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::Request,
        routing::Router,
    };
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let root = std::env::temp_dir().join(format!(
        "version-http-{}-{}",
        std::process::id(),
        openrust::support::Str::random(8)
    ));
    std::fs::create_dir_all(&root).unwrap();
    for version in [None, Some("7.6.5-application")] {
        std::fs::write(
            root.join(".env"),
            version.map_or_else(String::new, |value| format!("APP_VERSION={value}\n")),
        )
        .unwrap();
        for cached in [false, true] {
            let app = || {
                Application::configure(root.clone())
                    .with_config(|_, environment| {
                        Ok(std::collections::BTreeMap::from([(
                            "app".into(),
                            app_config::load(environment)?.config,
                        )]))
                    })
                    .with_routing(Routing::new(|_| {
                        let mut router = Router::new();
                        router.get("/version", version_response)?;
                        Ok(router)
                    }))
                    .create()
                    .unwrap()
            };
            if cached {
                let output = std::process::Command::new(env!("CARGO_BIN_EXE_main"))
                    .args(["artisan", "config:cache"])
                    .env("APP_BASE_PATH", &root)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{output:?}");
            }
            let mut kernel = Kernel::new(app());
            kernel.bootstrap().await.unwrap();
            assert_eq!(kernel.get_application().configuration_is_cached(), cached);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let handler = |State(kernel): State<Arc<Kernel>>, incoming: axum::extract::Request| async move {
                let (parts, body) = incoming.into_parts();
                let mut request = Request::from_parts(parts, body);
                let (parts, body) = kernel.handle(&mut request).await.into_parts();
                axum::response::Response::from_parts(parts, body)
            };
            let server = tokio::spawn(async move {
                axum::serve(
                    listener,
                    handler.with_state(Arc::new(kernel)).into_make_service(),
                )
                .await
                .unwrap();
            });
            let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
            socket
                .write_all(b"GET /version HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            let body: serde_json::Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["framework"], Application::VERSION);
            assert_eq!(
                body["application"],
                version.unwrap_or(env!("CARGO_PKG_VERSION"))
            );
            server.abort();
            let _ = server.await;
        }
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "config:clear"])
            .env("APP_BASE_PATH", &root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

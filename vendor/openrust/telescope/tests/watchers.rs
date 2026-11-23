use openrust::{
    contracts::events::Event,
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{DB, Event as Events, Route},
    testing::TestApp,
};
use openrust_telescope::{
    EntryType, Telescope, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
    watchers::{EventWatcher, QueryWatcher, Watcher},
};
use serde_json::json;

struct Probe(&'static str);
impl Event for Probe {
    fn name(&self) -> &str {
        self.0
    }
    fn properties(&self) -> Option<serde_json::Value> {
        Some(json!({"actual":true}))
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/watchers", handler)?;
    Route::get("/client-error", client_error_handler)?;
    for path in ["/json", "/plain", "/boundary", "/large", "/opaque"] {
        Route::get(path, response_handler)?;
    }
    Ok(())
}
fn handler(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        DB::connection(None)
            .unwrap()
            .select("SELECT 42 AS answer", &[])
            .await
            .unwrap();
        for name in [
            "Application\\Visible",
            "OpenRust\\Visible",
            "OpenRust\\Ignored",
        ] {
            Events::dispatch(Probe(name)).await.unwrap();
        }
        Ok(Response::from("observed"))
    })
}
fn application() -> TestApp {
    let app = TestApp::create(|builder| builder
        .with_config(|base, _| Ok(std::collections::BTreeMap::from([
            ("app".into(), json!({"env":"testing"})),
            ("database".into(), json!({"default":"test","connections":{"test":database_config(base)}})),
            ("cache".into(),json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache"),"lock_path":base.join("locks")}}})),
            ("telescope".into(),json!({"watchers":{"QueryWatcher":false,"EventWatcher":false,"RequestWatcher":{"size_limit":1},"ClientRequestWatcher":true}})),
        ])))
        .with_providers(vec![Box::new(TelescopeServiceProvider)])
        .with_routing(Routing::default().api(routes))
    ).unwrap();
    std::fs::write(app.root().join("watchers.sqlite"), []).unwrap();
    app
}
#[tokio::test]
async fn independent_watchers_use_own_options_and_framework_event_switch_is_application_local() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    app.get("/api/watchers").await.unwrap().assertOk();
    assert!(
        repository
            .get(Some(EntryType::Query), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    QueryWatcher::new(json!({"slow":0}).as_object().unwrap().clone())
        .register(app.app())
        .await
        .unwrap();
    EventWatcher::new(
        json!({"ignore":"OpenRust\\Ignored"})
            .as_object()
            .unwrap()
            .clone(),
    )
    .register(app.app())
    .await
    .unwrap();
    let disabled = QueryWatcher::new(json!({"enabled":false}).as_object().unwrap().clone());
    disabled.register(app.app()).await.unwrap();
    app.get("/api/watchers").await.unwrap().assertOk();
    let entries = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content["name"], "Application\\Visible");
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope
        .ignoreFrameworkEvents
        .store(false, std::sync::atomic::Ordering::Release);
    app.get("/api/watchers").await.unwrap().assertOk();
    let entries = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["name"] == "OpenRust\\Visible")
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.content["name"] != "OpenRust\\Ignored")
    );
    let queries = repository
        .get(Some(EntryType::Query), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(
        queries
            .iter()
            .filter(|entry| entry.content["sql"] == "SELECT 42 AS answer")
            .count(),
        2
    );
    assert!(queries.iter().all(|entry| entry.content["slow"] == true));
    let mut other = application();
    other.boot().await.unwrap();
    assert!(
        other
            .app()
            .make::<Telescope>()
            .await
            .unwrap()
            .ignoreFrameworkEvents
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

fn response_handler(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use http::{HeaderName, HeaderValue};
        use openrust::http::{Body, StatusCode};
        match request.path() {
            "api/json" => Response::json(
                StatusCode::OK,
                json!({"secret":"hidden","nested":{"password":"hidden"},"actual":true}),
            ),
            "api/plain" => Ok(Response::from("好".repeat(999)).header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )),
            "api/boundary" => Ok(Response::from("好".repeat(1000)).header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )),
            "api/large" => Ok(Response::from("好".repeat(1001)).header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )),
            _ => Ok(Response::new(StatusCode::OK, Body::from("opaque"))),
        }
    })
}
#[tokio::test]
async fn request_observes_buffered_content_redaction_character_limits_and_route_metadata() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.app()
        .make::<Telescope>()
        .await
        .unwrap()
        .hideResponseParameters(&["secret", "nested.password"]);
    for path in ["/json", "/plain", "/boundary", "/large", "/opaque"] {
        app.get(&format!("/api{path}")).await.unwrap().assertOk();
    }
    let entries = app
        .app()
        .make::<DatabaseEntriesRepository>()
        .await
        .unwrap()
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 5);
    for entry in entries {
        assert!(
            entry.content["controller_action"]
                .as_str()
                .unwrap()
                .ends_with("response_handler")
        );
        assert!(entry.content["middleware"].is_array());
        match entry.content["uri"].as_str().unwrap() {
            "/api/json" => assert_eq!(
                entry.content["response"],
                json!({"secret":"********","nested":{"password":"********"},"actual":true})
            ),
            "/api/plain" => assert_eq!(entry.content["response"], "好".repeat(999)),
            "/api/boundary" => assert_eq!(entry.content["response"], "好".repeat(1000)),
            "/api/large" => assert_eq!(entry.content["response"], "Purged By Telescope"),
            "/api/opaque" => assert_eq!(entry.content["response"], "Not Captured"),
            value => panic!("unexpected URI {value}"),
        }
    }
}

fn database_config(base: &std::path::Path) -> serde_json::Value {
    // 每个 TestApp 使用独立表前缀，避免共享验收库中的其他用例污染采集结果。
    let prefix = format!("w_{}_", &uuid::Uuid::now_v7().simple().to_string()[24..]);
    let mut connection = match std::env::var("TELESCOPE_TEST_DRIVER")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => json!({"driver":"sqlite","database":base.join("watchers.sqlite")}),
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()})
        }
        value => panic!("unsupported isolated driver {value}"),
    };
    connection["prefix"] = prefix.into();
    connection["prefix_indexes"] = true.into();
    connection
}

fn client_routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/client/{mode}", client_handler)?;
    Ok(())
}
fn client_handler(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let url = format!("http://example.test/{}", request.route("mode").unwrap());
        let response = openrust::support::facades::Http::get(&url)
            .await
            .map_err(|error| HttpError::Internal {
                source: Box::new(error),
            })?;
        Ok(Response::from(response.status().as_u16().to_string()))
    })
}
#[tokio::test]
async fn client_watcher_handles_closed_streams_and_uses_character_kilobytes() {
    use openrust::http::client::{Factory, Response as ClientResponse};
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|base, _| {
                Ok(std::collections::BTreeMap::from([
                    ("app".into(), json!({"env":"testing"})),
                    (
                        "database".into(),
                        json!({"default":"test","connections":{"test":database_config(base)}}),
                    ),
                    (
                        "telescope".into(),
                        json!({"watchers":{"ClientRequestWatcher":{"size_limit":1}}}),
                    ),
                ]))
            })
            .with_providers(vec![Box::new(TelescopeServiceProvider)])
            .with_routing(Routing::default().api(client_routes))
    })
    .unwrap();
    std::fs::write(app.root().join("watchers.sqlite"), []).unwrap();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hideResponseParameters(&["secret"]);
    let client = app.app().make::<Factory>().await.unwrap();
    client.fake(|request| {
        Some(Ok(match request.url().path() {
            "/small" => ClientResponse::from_json(
                http::StatusCode::OK,
                &json!({"secret":"value","actual":true}),
            )
            .unwrap(),
            "/large" => ClientResponse::new(
                http::StatusCode::OK,
                [(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("text/plain"),
                )]
                .into_iter()
                .collect(),
                "好".repeat(1001).into_bytes(),
            ),
            _ => ClientResponse::new(
                http::StatusCode::OK,
                [(
                    http::HeaderName::from_static("x-close"),
                    http::HeaderValue::from_static("yes"),
                )]
                .into_iter()
                .collect(),
                b"closed",
            ),
        }))
    });
    client.globalResponseMiddleware(|response| {
        if response.header("x-close").is_some() {
            response.close()?;
        }
        Ok(response)
    });
    for mode in ["small", "large", "closed"] {
        app.get(&format!("/api/client/{mode}"))
            .await
            .unwrap()
            .assertOk();
    }
    let entries = app
        .app()
        .make::<DatabaseEntriesRepository>()
        .await
        .unwrap()
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        match entry.content["uri"].as_str().unwrap() {
            "http://example.test/small" => assert_eq!(
                entry.content["response"],
                json!({"secret":"********","actual":true})
            ),
            "http://example.test/large" => {
                assert_eq!(entry.content["response"], "Purged By Telescope")
            }
            "http://example.test/closed" => {
                assert_eq!(entry.content["response"], "Stream Response")
            }
            uri => panic!("unexpected URI {uri}"),
        }
    }
}

fn client_error_handler(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let file = tempfile::NamedTempFile::new().unwrap();
        let output = std::fs::OpenOptions::new()
            .write(true)
            .open(file.path())
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        let result = openrust::support::facades::Http::sink(output)
            .unwrap()
            .get(&url)
            .await;
        server.await.unwrap();
        let error = result.unwrap_err();
        assert!(matches!(error, openrust::http::client::Error::Event(_)));
        let mut source: &(dyn std::error::Error + 'static) = &error;
        while let Some(inner) = source.source() {
            source = inner;
        }
        assert!(source.downcast_ref::<std::io::Error>().is_some());
        Ok(Response::from("read error propagated"))
    })
}
#[tokio::test]
async fn client_watcher_propagates_unreadable_seekable_file_errors() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/client-error")
        .await
        .unwrap()
        .assertSee("read error propagated", true);
}

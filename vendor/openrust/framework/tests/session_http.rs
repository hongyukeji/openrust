use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        session::Session,
    },
    cookie::middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    database::DatabaseManager,
    encryption::{Cipher, Encrypter},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RoutingError},
    session::middleware::StartSession,
    support::facades::Route,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let action = request.route("action").unwrap_or("read").to_owned();
        if action == "old" {
            return Response::json(
                StatusCode::OK,
                json!({
                    "name": request.old(Some("profile.name")),
                    "explicit_null": request.old(Some("profile.description")).is_some(),
                    "all": request.old(None),
                }),
            );
        }
        let session = request.session_mut()?;
        match action.as_str() {
            "seed" => {
                session.put("profile.name", json!("Ada"))?;
                session.flash("message", json!("Saved"))?;
            }
            "flash-input" => {
                session.flashInput(json!({"profile":{"name":"Ada","description":null}}))?
            }
            "keep" => session.keep(&["message"]),
            "rotate" => session.regenerate(true).await?,
            "invalidate" => session.invalidate().await?,
            _ => {}
        }
        Response::json(
            StatusCode::OK,
            json!({
                "id": session.getId(),
                "token": session.token(),
                "name": session.get("profile.name"),
                "message": session.get("message"),
            }),
        )
    })
}

fn register() -> Result<(), RoutingError> {
    Route::get("/session/{action}", route)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(
    driver: &'static str,
    protected: bool,
) -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Option<openrust::database::Connection>,
) {
    fn config(driver: &str) -> std::collections::BTreeMap<String, Value> {
        let mut config = [
            ("app".into(), json!({"key": "a".repeat(32)})),
            (
                "session".into(),
                json!({
                    "driver":driver, "cookie":"probe_session", "lottery":[0,100],
                    "encrypt":true, "expire_on_close":true,
                }),
            ),
        ]
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
        if driver == "database" {
            let url = std::env::var("OPENRUST_TEST_SESSION_MYSQL_URL").unwrap();
            config.insert("database".into(), json!({
                "default":"session_test","connections":{"session_test":{"url":url,"use_upsert_alias":true}}
            }));
        }
        config
    }
    let load = if driver == "database" {
        |_: &std::path::Path, _: &openrust::config::Environment| Ok(config("database"))
    } else {
        |_: &std::path::Path, _: &openrust::config::Environment| Ok(config("array"))
    };
    let app = Application::configure(std::env::temp_dir())
        .withConfig(load)
        .withMiddleware(move |middleware| {
            let mut entries = Vec::new();
            if protected {
                let encrypt = EncryptCookies::new(Arc::new(
                    Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap(),
                ));
                entries.push(MiddlewareEntry::Instance(Arc::new(encrypt)));
            }
            entries.push(MiddlewareEntry::Instance(Arc::new(
                AddQueuedCookiesToResponse,
            )));
            entries.push(MiddlewareEntry::Instance(Arc::new(StartSession)));
            middleware.group("web", entries);
        })
        .withRouting(Routing::default().web(register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let database = if driver == "database" {
        let db = kernel
            .get_application()
            .container()
            .get::<DatabaseManager>("db")
            .unwrap()
            .connection(None)
            .unwrap();
        db.statement("create table sessions (id varchar(255) primary key, user_id bigint null, ip_address varchar(45) null, user_agent text null, payload longtext not null, last_activity integer not null)", &[]).await.unwrap();
        Some(db)
    } else {
        None
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport
        .with_state(Arc::new(kernel))
        .into_make_service_with_connect_info::<SocketAddr>();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server, database)
}

async fn send(address: SocketAddr, action: &str, cookie: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("GET /session/{action} HTTP/1.1\r\nHost: {address}\r\nCookie: {cookie}\r\nUser-Agent: session-http-test\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).await.unwrap();
        wire
    }).await.unwrap()
}

fn read(response: &str) -> (&str, Value) {
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    let header = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: probe_session="))
        .unwrap();
    assert!(header.contains("; httponly"));
    assert!(!header.contains("Max-Age="));
    let first = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: "))
        .unwrap();
    (
        first.split(';').next().unwrap(),
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap(),
    )
}

async fn verify(driver: &'static str) {
    let (address, server, database) = start(driver, true).await;
    let initial_response = send(address, "seed", "").await;
    let (cookie, initial) = read(&initial_response);
    assert_eq!(initial["name"], "Ada");
    assert_eq!(initial["message"], "Saved");
    assert_eq!(initial["id"].as_str().unwrap().len(), 40);
    assert_eq!(initial["token"].as_str().unwrap().len(), 40);

    let keep_response = send(address, "keep", cookie).await;
    let (kept_cookie, kept) = read(&keep_response);
    assert_eq!(kept["message"], "Saved");
    let follow_response = send(address, "read", kept_cookie).await;
    let (follow_cookie, follow) = read(&follow_response);
    assert_eq!(follow["message"], "Saved");
    let aged_response = send(address, "read", follow_cookie).await;
    let (aged_cookie, aged) = read(&aged_response);
    assert!(aged["message"].is_null());
    assert_eq!(aged["name"], "Ada");
    assert_eq!(aged["id"], initial["id"]);
    assert_eq!(aged["token"], initial["token"]);

    if let Some(db) = &database {
        let row = db
            .table("sessions")
            .r#where("id", "=", aged["id"].as_str().unwrap())
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("user_agent"), Some(&"session-http-test".into()));
        assert_eq!(row.get("ip_address"), Some(&"127.0.0.1".into()));
        let Some(openrust::database::Value::Text(payload)) = row.get("payload") else {
            panic!("expected database payload");
        };
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        let payload = String::from_utf8(decoded).unwrap();
        assert!(Encrypter::appearsEncrypted(&payload));
        assert!(!payload.contains("Ada"));
    }

    let rotate_response = send(address, "rotate", aged_cookie).await;
    let (rotated_cookie, rotated) = read(&rotate_response);
    assert_ne!(rotated["id"], initial["id"]);
    assert_ne!(rotated["token"], initial["token"]);
    assert_eq!(rotated["name"], "Ada");
    let replay_response = send(address, "read", aged_cookie).await;
    let (_, replay) = read(&replay_response);
    assert!(replay["name"].is_null());

    let invalidated_response = send(address, "invalidate", rotated_cookie).await;
    let (invalidated_cookie, invalidated) = read(&invalidated_response);
    assert_ne!(invalidated["id"], rotated["id"]);
    assert!(invalidated["name"].is_null());
    assert!(invalidated["token"].is_null());
    let renewed_response = send(address, "read", invalidated_cookie).await;
    let (_, renewed) = read(&renewed_response);
    assert_eq!(renewed["id"], invalidated["id"]);
    assert_eq!(renewed["token"].as_str().unwrap().len(), 40);

    if let Some(db) = database {
        db.table("sessions")
            .r#where("id", "=", renewed["id"].as_str().unwrap())
            .update([("last_activity".into(), 1_i64.into())].into())
            .await
            .unwrap();
        let expired_response = send(address, "read", invalidated_cookie).await;
        let (_, expired) = read(&expired_response);
        assert_ne!(expired["token"], renewed["token"]);
        db.statement("drop table sessions", &[]).await.unwrap();
        let unavailable = send(address, "read", invalidated_cookie).await;
        assert!(unavailable.starts_with("HTTP/1.1 500 "));
        assert!(!unavailable.contains("set-cookie:"));
    }
    server.abort();
}

#[tokio::test]
async fn request_sessions_round_trip_through_array_storage() {
    verify("array").await;
    let (address, server, _) = start("array", false).await;
    let response = send(address, "seed", "").await;
    assert!(response.starts_with("HTTP/1.1 500 "));
    assert!(!response.contains("set-cookie:"));
    server.abort();
}

#[tokio::test]
async fn request_sessions_round_trip_through_mysql() {
    let configured = std::env::var_os("OPENRUST_TEST_SESSION_MYSQL_URL").is_some();
    assert!(configured || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL").is_none());
    if configured {
        verify("database").await;
    }
}

#[tokio::test]
async fn a_failed_save_preserves_flash_state_for_retry() {
    use openrust::{
        contracts::session::SessionHandler,
        session::{ArraySessionHandler, SessionError, SessionFuture, SessionResult, Store},
    };

    struct FailFirst {
        storage: ArraySessionHandler,
        fail: bool,
    }

    impl SessionHandler for FailFirst {
        fn read<'a>(
            &'a mut self,
            id: &'a str,
        ) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
            self.storage.read(id)
        }

        fn write<'a>(
            &'a mut self,
            id: &'a str,
            payload: &'a [u8],
        ) -> SessionFuture<'a, SessionResult<()>> {
            if std::mem::take(&mut self.fail) {
                return Box::pin(async { Err(SessionError::Storage) });
            }
            self.storage.write(id, payload)
        }

        fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
            self.storage.destroy(id)
        }

        fn gc(&mut self, lifetime: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
            self.storage.gc(lifetime)
        }
    }

    let storage = ArraySessionHandler::new(Duration::from_secs(60));
    let mut store = Store::new(
        "session",
        Box::new(FailFirst {
            storage: storage.clone(),
            fail: true,
        }),
    )
    .unwrap();
    store.start().await.unwrap();
    store.flash("message", json!("Saved")).unwrap();
    assert!(matches!(store.save().await, Err(SessionError::Storage)));
    assert!(store.isStarted());
    assert_eq!(store.get("message"), Some(&json!("Saved")));
    store.save().await.unwrap();
    assert!(!store.isStarted());

    let mut reader = Store::new("session", Box::new(storage)).unwrap();
    reader.setId(Some(store.getId())).unwrap();
    reader.start().await.unwrap();
    assert_eq!(reader.get("message"), Some(&json!("Saved")));
    reader.save().await.unwrap();
    reader.start().await.unwrap();
    assert!(reader.get("message").is_none());
}

#[tokio::test]
async fn old_input_can_be_read_once_through_the_http_request() {
    let (address, server, _) = start("array", true).await;
    let flashed = send(address, "flash-input", "").await;
    let (cookie, _) = read(&flashed);
    let next = send(address, "old", cookie).await;
    let (cookie, values) = read(&next);
    assert_eq!(
        values,
        json!({
            "name":"Ada","explicit_null":true,"all":{"profile":{"name":"Ada","description":null}}
        })
    );
    let expired = send(address, "old", cookie).await;
    let (_, values) = read(&expired);
    assert_eq!(
        values,
        json!({"name":null,"explicit_null":false,"all":null})
    );
    server.abort();
}

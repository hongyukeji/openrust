use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::{CacheError, CacheManager, SessionStore, Ttl, Value as CacheValue},
    contracts::{
        cache::{LockProvider, Repository as _, Store as _},
        container::Container as _,
        foundation::Application as _,
        http::Kernel as _,
        session::Session,
    },
    cookie::middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    database::DatabaseManager,
    encryption::{Cipher, Encrypter},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RoutingError},
    session::middleware::StartSession,
    support::facades::{Cache, Facade as _, Route},
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
        if let Some(action) = action.strip_prefix("cache-") {
            return session_cache_route(request, action).await;
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
            "oversized" => session.put("padding", json!("x".repeat(3000)))?,
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

async fn session_cache_route(request: &mut Request, action: &str) -> Result<Response, HttpError> {
    let cache = request.session()?.cache()?;
    let other = Cache::build(json!({"driver": "session", "key": "other.cache"}))?;
    let named = Cache::store(Some("session"))?;
    let owner = Cache::getFacadeRoot()
        .map_err(CacheError::Unavailable)?
        .getContainer()
        .ok_or(CacheError::SessionUnavailable)?;
    let underlying = SessionStore::new(Arc::downgrade(&owner), "widgets".into())?;
    assert_eq!(
        underlying.itemKey("dashboard.banner"),
        "widgets.dashboard.banner"
    );
    assert_eq!(underlying.getPrefix(), "");
    assert!(Arc::ptr_eq(&cache, &named));
    assert!(!cache.supportsTags() && !cache.supportsFlushingLocks());
    assert!(cache.get_store().lock_provider().is_none());

    match action {
        "seed" => {
            request.session_mut()?.put("marker", json!("retained"))?;
            assert!(cache.forever("dashboard.banner", &"ready".into()).await?);
            assert!(!named.add("dashboard.banner", &"wrong".into(), None).await?);
            assert!(cache.forever("binary", &vec![0, 255].into()).await?);
            assert!(
                cache
                    .put("expiring", &1.into(), Some(Ttl::Seconds(1)))
                    .await?
            );
            assert_eq!(cache.increment("expiring", 2).await?, 3);
            assert!(cache.put("stale", &1.into(), Some(Ttl::Seconds(1))).await?);
            assert!(
                cache
                    .forever("nullable", &CacheValue::Json(Value::Null))
                    .await?
            );
            assert!(underlying.all()?["dashboard"]["banner"].is_object());
            assert_eq!(cache.decrement("counter", 2).await?, -2);
            assert!(other.forever("dashboard.banner", &"other".into()).await?);
        }
        "touch" => {
            assert!(cache.touch("dashboard.banner", Ttl::Seconds(30)).await?);
            assert!(!cache.touch("absent", Ttl::Seconds(30)).await?);
            assert!(!cache.touch("nullable", Ttl::Seconds(30)).await?);
            assert_eq!(cache.pull("counter").await?, Some((-2).into()));
            assert!(!cache.forget("counter").await?);
        }
        "rotate" => {
            request.session_mut()?.regenerate(true).await?;
            assert!(underlying.forget("stale").await?);
        }
        "invalidate" => request.session_mut()?.invalidate().await?,
        "flush" => assert!(cache.flush().await?),
        _ => {}
    }
    let values = cache
        .many(&["dashboard.banner", "binary", "expiring", "counter"])
        .await?;
    let [banner, binary, expiring, counter] = values.as_slice() else {
        unreachable!("four requested keys");
    };
    let other = other.get("dashboard.banner").await?;
    Response::json(
        StatusCode::OK,
        json!({
            "id": request.session()?.getId(),
            "marker": request.session()?.get("marker"),
            "banner": banner.as_ref().and_then(CacheValue::as_str),
            "binary": binary.as_ref() == Some(&vec![0, 255].into()),
            "expiring": expiring.as_ref().and_then(CacheValue::as_i64),
            "counter": counter.as_ref().and_then(CacheValue::as_i64),
            "other": other.as_ref().and_then(CacheValue::as_str),
        }),
    )
}

fn register() -> Result<(), RoutingError> {
    Route::get("/session/{action}", route)
}

fn register_blocked() -> Result<(), RoutingError> {
    Route::getWith("/session/{action}", blocked_route, |route| {
        route.block(10, 2);
        Ok(())
    })?;
    Route::getWith("/session/immediate/{action}", blocked_route, |route| {
        route.block(10, 0);
        Ok(())
    })?;
    Route::getWith("/session/default/{action}", blocked_route, |route| {
        route.block(10, 2).withoutBlocking();
        Ok(())
    })
}

fn blocked_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let action = request.route("action").unwrap_or_default().to_owned();
        if action == "error" {
            return Err(HttpError::NotFound);
        }
        let session = request.session_mut()?;
        let mut count = session
            .get("count")
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if action == "increment" {
            tokio::time::sleep(Duration::from_millis(100)).await;
            count += 1;
            session.put("count", json!(count))?;
        }
        Response::json(StatusCode::OK, json!({"id":session.getId(), "count":count}))
    })
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
        config.insert(
            "cache".into(),
            json!({
                "stores": {"session": {"driver": "session", "key": "widgets"}}
            }),
        );
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
    let cache = kernel
        .get_application()
        .container()
        .get::<CacheManager>("cache")
        .unwrap()
        .store(Some("session"))
        .unwrap();
    assert!(matches!(
        cache.get("which").await,
        Err(CacheError::SessionUnavailable)
    ));
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
    let header = cookie_header(response, "probe_session");
    assert!(header.contains("; httponly"));
    assert!(!header.contains("Max-Age="));
    (
        header.split(';').next().unwrap(),
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap(),
    )
}

fn cookie_header<'a>(response: &'a str, name: &str) -> &'a str {
    response
        .lines()
        .filter_map(|line| line.strip_prefix("set-cookie: "))
        .find(|value| {
            value
                .split_once('=')
                .is_some_and(|(prefix, _)| prefix == name)
        })
        .unwrap_or_else(|| panic!("missing response cookie {name}"))
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

async fn start_local_driver(
    root: &std::path::Path,
    driver: &str,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    fn config(driver: &str) -> std::collections::BTreeMap<String, Value> {
        let cookie = driver.starts_with("cookie");
        [
            ("app".into(), json!({"key": "a".repeat(32)})),
            (
                "session".into(),
                json!({
                    "driver": if cookie {"cookie"} else {driver},
                    "files": "sessions", "cookie": "probe_session",
                    "lifetime": 1, "encrypt": driver != "cookie-clear", "expire_on_close": true,
                    "lottery": [1,1], "block": driver == "cookie-lock"
                }),
            ),
        ]
        .into()
    }
    let builder = Application::configure(root.to_path_buf());
    let app = match driver {
        "file" => builder.withConfig(|_, _| Ok(config("file"))),
        "native" => builder.withConfig(|_, _| Ok(config("native"))),
        "null" => builder.withConfig(|_, _| Ok(config("null"))),
        "cookie" => builder.withConfig(|_, _| Ok(config("cookie"))),
        "cookie-clear" => builder.withConfig(|_, _| Ok(config("cookie-clear"))),
        "cookie-lock" => builder.withConfig(|_, _| Ok(config("cookie-lock"))),
        _ => unreachable!(),
    }
    .withRouting(Routing::default().web(register))
    .create()
    .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}

#[tokio::test]
async fn cookie_sessions_round_trip_and_rotate_across_applications() {
    use openrust::cookie::CookieValuePrefix;

    let root = tempfile::tempdir().unwrap();
    let (first, first_server) = start_local_driver(root.path(), "cookie").await;
    let original = send(first, "seed", "").await;
    let (pointer, state) = read(&original);
    let id = state["id"].as_str().unwrap();
    let payload_header = cookie_header(&original, id);
    assert!(payload_header.len() <= 4096);
    assert!(payload_header.contains("; httponly"));
    assert!(!payload_header.contains("Max-Age="));
    let payload_cookie = payload_header.split(';').next().unwrap();
    assert!(!payload_header.contains("Ada"));
    let jar = format!("{pointer}; {payload_cookie}");
    first_server.abort();

    let (second, second_server) = start_local_driver(root.path(), "cookie").await;
    let continued = send(second, "read", &jar).await;
    let (_, continued_state) = read(&continued);
    assert_eq!(continued_state["id"], state["id"]);
    assert_eq!(continued_state["token"], state["token"]);
    assert_eq!(continued_state["name"], "Ada");
    assert_eq!(continued_state["message"], "Saved");

    let rotated = send(second, "rotate", &jar).await;
    let (_, rotated_state) = read(&rotated);
    assert_ne!(rotated_state["id"], state["id"]);
    assert_ne!(rotated_state["token"], state["token"]);
    assert_eq!(rotated_state["name"], "Ada");
    assert!(cookie_header(&rotated, id).contains("Max-Age=0"));
    assert!(!cookie_header(&rotated, rotated_state["id"].as_str().unwrap()).contains("Max-Age="));

    for cookies in [pointer.to_owned(), format!("{pointer}; {id}=tampered")] {
        let (_, fresh) = read(&send(second, "read", &cookies).await);
        assert!(fresh["name"].is_null());
        assert_ne!(fresh["token"], state["token"]);
    }

    let encrypter = Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap();
    let ciphertext =
        percent_encoding::percent_decode_str(payload_cookie.split_once('=').unwrap().1)
            .decode_utf8()
            .unwrap();
    let decoded = encrypter.decryptString(&ciphertext).unwrap();
    let mut envelope: Value = serde_json::from_slice(
        CookieValuePrefix::validate(id, &decoded, encrypter.getAllKeys()).unwrap(),
    )
    .unwrap();
    assert!(Encrypter::appearsEncrypted(
        envelope["data"].as_str().unwrap()
    ));
    envelope["expires"] = 0.into();
    let mut expired = CookieValuePrefix::create(id, encrypter.getKey()).to_vec();
    expired.extend(serde_json::to_vec(&envelope).unwrap());
    let expired = encrypter.encryptString(&expired).unwrap();
    let (_, fresh) = read(&send(second, "read", &format!("{pointer}; {id}={expired}")).await);
    assert!(fresh["name"].is_null());
    assert_ne!(fresh["token"], state["token"]);
    assert!(!root.path().join("sessions").exists());
    second_server.abort();
}

#[tokio::test]
async fn cookie_sessions_reject_over_budget_headers_and_session_blocking() {
    let root = tempfile::tempdir().unwrap();
    for (driver, action) in [("cookie-clear", "oversized"), ("cookie-lock", "seed")] {
        let (address, server) = start_local_driver(root.path(), driver).await;
        let response = send(address, action, "").await;
        assert!(response.starts_with("HTTP/1.1 500 "));
        assert!(!response.contains("set-cookie:"));
        server.abort();
    }
}

#[tokio::test]
async fn file_sessions_persist_across_applications_and_prune_expired_files() {
    use std::{fs, time::SystemTime};

    let root = tempfile::tempdir().unwrap();
    let (first, first_server) = start_local_driver(root.path(), "file").await;
    let seeded = send(first, "seed", "").await;
    let (cookie, original) = read(&seeded);
    let directory = root.path().join("sessions");
    let initial_file = directory.join(original["id"].as_str().unwrap());
    let stored = fs::read_to_string(&initial_file).unwrap();
    assert!(Encrypter::appearsEncrypted(&stored));
    assert!(!stored.contains("Ada"));
    first_server.abort();
    first_server.await.unwrap_err();

    let (second, second_server) = start_local_driver(root.path(), "native").await;
    let continued = send(second, "read", cookie).await;
    let (cookie, continued) = read(&continued);
    assert_eq!(continued["id"], original["id"]);
    assert_eq!(continued["token"], original["token"]);
    assert_eq!(continued["name"], "Ada");
    assert_eq!(continued["message"], "Saved");
    let rotated = send(second, "rotate", cookie).await;
    let (cookie, rotated) = read(&rotated);
    assert_ne!(rotated["id"], original["id"]);
    assert_eq!(rotated["name"], "Ada");
    assert!(!initial_file.exists());

    let expired = directory.join(rotated["id"].as_str().unwrap());
    let orphaned = directory.join("0".repeat(40));
    let other = directory.join("README");
    fs::write(&orphaned, b"expired").unwrap();
    fs::write(&other, b"keep").unwrap();
    for path in [&expired, &orphaned, &other] {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(120))
            .unwrap();
    }
    let (_, expired_response) = read(&send(second, "read", cookie).await);
    assert!(expired_response["name"].is_null());
    assert_ne!(expired_response["token"], rotated["token"]);
    assert!(
        expired.exists(),
        "the response should store a fresh session"
    );
    assert!(!orphaned.exists());
    assert!(other.exists());
    let invalidated = send(second, "invalidate", cookie).await;
    let (cookie, invalidated) = read(&invalidated);
    assert_ne!(invalidated["id"], rotated["id"]);
    assert!(!expired.exists());
    fs::File::create(directory.join(invalidated["id"].as_str().unwrap()))
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    let oversized = send(second, "read", cookie).await;
    assert!(oversized.starts_with("HTTP/1.1 500 "));
    assert!(!oversized.contains("set-cookie:"));
    second_server.abort();
}

#[tokio::test]
async fn null_sessions_discard_changes_and_rotate_tokens_between_requests() {
    let root = tempfile::tempdir().unwrap();
    let (address, server) = start_local_driver(root.path(), "null").await;
    let seeded = send(address, "seed", "").await;
    let (cookie, original) = read(&seeded);
    assert_eq!(original["name"], "Ada");
    let (_, next) = read(&send(address, "read", cookie).await);
    assert_eq!(next["id"], original["id"]);
    assert_ne!(next["token"], original["token"]);
    assert!(next["name"].is_null() && next["message"].is_null());
    assert!(!root.path().join("sessions").exists());
    server.abort();
}

#[tokio::test]
async fn redis_sessions_override_the_named_cache_connection_and_prefix() {
    use openrust::redis::{RedisManager, Value as RedisValue};

    let Ok(port) = std::env::var("OPENRUST_TEST_SESSION_REDIS_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SESSION_REDIS").is_none());
        return;
    };
    let port: u16 = port.parse().unwrap();
    let configuration = json!({
        "app": {"key": "a".repeat(32)},
        "session": {
            "driver": "redis", "store": "payloads", "connection": "sessions",
            "prefix": "session-only:", "lifetime": 2, "encrypt": true,
            "cookie": "probe_session", "expire_on_close": true
        },
        "cache": {
            "stores": {
                "payloads": {"driver": "redis", "connection": "cache", "prefix": "ordinary:"}
            }
        },
        "database": {"redis": {
            "options": {"prefix": "session-http:"},
            "default": {"port": port, "database": 0},
            "cache": {"port": port, "database": 1},
            "sessions": {"port": port, "database": 2}
        }}
    });
    let root = tempfile::tempdir().unwrap();
    tokio::fs::write(
        root.path().join("configuration.json"),
        serde_json::to_vec(&configuration).unwrap(),
    )
    .await
    .unwrap();
    let start = async || {
        let app =
            Application::configure(root.path().to_path_buf())
                .withConfig(|base, _| {
                    Ok(serde_json::from_slice(
                        &std::fs::read(base.join("configuration.json")).unwrap(),
                    )
                    .unwrap())
                })
                .withRouting(Routing::default().web(register))
                .create()
                .unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let kernel = Arc::new(kernel);
        let service = transport.with_state(kernel.clone()).into_make_service();
        let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        (address, server, kernel)
    };
    let (first, first_server, kernel) = start().await;
    let response = send(first, "seed", "").await;
    let (cookie, seeded) = read(&response);
    let id = seeded["id"].as_str().unwrap();
    let container = kernel.get_application().container();
    let redis = container.get::<RedisManager>("redis").unwrap();
    let sessions = redis.connection(Some("sessions")).unwrap();
    let ordinary = redis.connection(Some("cache")).unwrap();
    let session_key = sessions.key(&format!("session-only:{id}"));
    let ordinary_key = ordinary.key(&format!("ordinary:{id}"));
    let selected_cache = container
        .get::<CacheManager>("cache")
        .unwrap()
        .store(Some("payloads"))
        .unwrap();
    assert_eq!(selected_cache.get(id).await.unwrap(), None);
    assert!(selected_cache.forever(id, &"keep".into()).await.unwrap());
    assert_eq!(
        ordinary
            .command("EXISTS", &[ordinary_key.as_bytes()])
            .await
            .unwrap(),
        RedisValue::Int(1)
    );
    assert_eq!(
        ordinary
            .command("EXISTS", &[session_key.as_bytes()])
            .await
            .unwrap(),
        RedisValue::Int(0)
    );
    let ttl = sessions
        .command("TTL", &[session_key.as_bytes()])
        .await
        .unwrap();
    assert!(matches!(ttl, RedisValue::Int(115..=120)), "{ttl:?}");
    first_server.abort();

    let (second, second_server, _) = start().await;
    let followed = send(second, "read", cookie).await;
    let (cookie, followed) = read(&followed);
    assert_eq!(followed["token"], seeded["token"]);
    assert_eq!(followed["name"], "Ada");
    assert_eq!(followed["message"], "Saved");
    let rotated = send(second, "rotate", cookie).await;
    let (cookie, rotated) = read(&rotated);
    assert_ne!(rotated["id"], seeded["id"]);
    assert_eq!(
        sessions
            .command("EXISTS", &[session_key.as_bytes()])
            .await
            .unwrap(),
        RedisValue::Int(0)
    );
    assert_eq!(selected_cache.get(id).await.unwrap(), Some("keep".into()));
    let rotated_key = sessions.key(&format!("session-only:{}", rotated["id"].as_str().unwrap()));
    assert_eq!(
        sessions
            .command("PEXPIRE", &[rotated_key.as_bytes(), b"20"])
            .await
            .unwrap(),
        RedisValue::Int(1)
    );
    timeout(Duration::from_secs(2), async {
        while sessions
            .command("EXISTS", &[rotated_key.as_bytes()])
            .await
            .unwrap()
            == RedisValue::Int(1)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let (_, expired) = read(&send(second, "read", cookie).await);
    assert!(expired["name"].is_null());
    assert_ne!(expired["token"], rotated["token"]);
    second_server.abort();
}

#[tokio::test]
async fn request_sessions_round_trip_through_mysql() {
    let configured = std::env::var_os("OPENRUST_TEST_SESSION_MYSQL_URL").is_some();
    assert!(configured || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL").is_none());
    if configured {
        verify("database").await;
    }
}

async fn verify_session_cache(driver: &'static str) {
    let (address, server, database) = start(driver, true).await;
    let initial = send(address, "cache-seed", "").await;
    let (first, seeded) = read(&initial);
    assert_eq!(seeded["banner"], "ready");
    assert_eq!(seeded["expiring"], 3);
    assert_eq!(seeded["counter"], -2);
    assert_eq!(seeded["binary"], true);

    let independent = send(address, "cache-read", "").await;
    let (second, untouched) = read(&independent);
    assert_ne!(untouched["id"], seeded["id"]);
    assert!(untouched["banner"].is_null());
    assert!(untouched["other"].is_null());
    let second_seed = send(address, "cache-seed", second).await;
    let (second, _) = read(&second_seed);

    let touched = send(address, "cache-touch", first).await;
    let (_, touched) = read(&touched);
    assert!(touched["counter"].is_null());
    tokio::time::sleep(Duration::from_millis(1050)).await;

    let rotated = send(address, "cache-rotate", first).await;
    let (rotated_cookie, rotated) = read(&rotated);
    assert_ne!(rotated["id"], seeded["id"]);
    assert_eq!(rotated["banner"], "ready");
    assert_eq!(rotated["binary"], true);
    assert!(rotated["expiring"].is_null());

    let flushed = send(address, "cache-flush", rotated_cookie).await;
    let (flushed_cookie, flushed) = read(&flushed);
    assert!(flushed["banner"].is_null());
    assert_eq!(flushed["other"], "other");
    assert_eq!(flushed["marker"], "retained");
    assert_eq!(
        read(&send(address, "cache-read", second).await).1["banner"],
        "ready"
    );

    let invalidated = send(address, "cache-invalidate", flushed_cookie).await;
    let (_, invalidated) = read(&invalidated);
    assert_ne!(invalidated["id"], rotated["id"]);
    assert!(invalidated["marker"].is_null());
    assert!(invalidated["other"].is_null());

    if let Some(database) = database {
        database
            .statement("drop table sessions", &[])
            .await
            .unwrap();
    }
    server.abort();
}

#[tokio::test]
async fn session_cache_is_isolated_per_cookie() {
    verify_session_cache("array").await;
}

#[tokio::test]
async fn session_cache_persists_in_mysql() {
    let configured = std::env::var_os("OPENRUST_TEST_SESSION_MYSQL_URL").is_some();
    assert!(configured || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL").is_none());
    if configured {
        verify_session_cache("database").await;
    }
}

async fn verify_blocking(mysql_redis: bool, global: bool) {
    fn config(
        root: &std::path::Path,
        mysql_redis: bool,
        global: bool,
    ) -> std::collections::BTreeMap<String, Value> {
        let mut config = std::collections::BTreeMap::from([
            ("app".into(), json!({"key": "a".repeat(32), "debug": true})),
            (
                "session".into(),
                json!({
                    "driver": if mysql_redis {"database"} else {"array"},
                    "cookie":"probe_session", "lottery":[0,100], "table":"blocking_sessions",
                    "block_store": if mysql_redis {"redis"} else {"array"},
                    "block":global, "block_wait_seconds":0,
                    "expire_on_close": true,
                }),
            ),
            (
                "cache".into(),
                json!({
                    "default":"array",
                    "prefix":format!("openrust-session-block:{}:", root.file_name().unwrap().to_string_lossy()),
                    "stores":{"array":{"driver":"array"}, "redis":{"driver":"redis","connection":"default"}}
                }),
            ),
        ]);
        if mysql_redis {
            let url = std::env::var("OPENRUST_TEST_SESSION_MYSQL_URL").unwrap();
            let port = std::env::var("OPENRUST_TEST_REDIS_PORT").unwrap();
            config.insert("database".into(), json!({
                "default":"session_test","connections":{"session_test":{"url":url,"use_upsert_alias":true}},
                "redis":{"options":{"prefix":""},"default":{"url":format!("redis://127.0.0.1:{port}/")}}
            }));
        }
        config
    }

    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .withConfig(match (mysql_redis, global) {
            (true, _) => |root, _| Ok(config(root, true, false)),
            (false, true) => |root, _| Ok(config(root, false, true)),
            (false, false) => |root, _| Ok(config(root, false, false)),
        })
        .withRouting(Routing::default().web(register_blocked))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let container = kernel.get_application().container();
    let database = mysql_redis.then(|| {
        container
            .get::<DatabaseManager>("db")
            .unwrap()
            .connection(None)
            .unwrap()
    });
    if let Some(db) = &database {
        db.statement("create table blocking_sessions (id varchar(255) primary key, user_id bigint null, ip_address varchar(45) null, user_agent text null, payload longtext not null, last_activity integer not null)", &[]).await.unwrap();
    }
    let cache = container
        .get::<openrust::cache::CacheManager>("cache")
        .unwrap()
        .store(Some(if mysql_redis { "redis" } else { "array" }))
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport
        .with_state(Arc::new(kernel))
        .into_make_service_with_connect_info::<SocketAddr>();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

    let initial = send(address, "read", "").await;
    let (cookie, value) = read(&initial);
    let key = format!("session:{}", value["id"].as_str().unwrap());
    let mut held = cache
        .lock(&key, Some(Duration::from_secs(10)), None)
        .unwrap();
    assert!(held.get().await.unwrap());
    let blocked = send(address, "immediate/increment", cookie).await;
    assert!(blocked.starts_with("HTTP/1.1 500 "), "{blocked}");
    assert!(
        blocked.contains("Timed out acquiring the cache lock."),
        "{blocked}"
    );
    assert!(!blocked.contains("set-cookie:"));
    let default = send(address, "default/read", cookie).await;
    if global {
        assert!(default.starts_with("HTTP/1.1 500 "), "{default}");
    } else {
        assert_eq!(read(&default).1["count"], 0);
    }
    assert_eq!(read(&send(address, "increment", "").await).1["count"], 1);
    held.release().await.unwrap();

    let (first, second) = tokio::join!(
        send(address, "increment", cookie),
        send(address, "increment", cookie)
    );
    let mut counts = [
        read(&first).1["count"].as_i64().unwrap(),
        read(&second).1["count"].as_i64().unwrap(),
    ];
    counts.sort_unstable();
    assert_eq!(counts, [1, 2]);
    let failed = send(address, "error", cookie).await;
    assert!(failed.starts_with("HTTP/1.1 404 "), "{failed}");
    assert!(
        held.get().await.unwrap(),
        "downstream error must release the lock"
    );
    held.release().await.unwrap();
    let recovered = send(address, "immediate/increment", cookie).await;
    assert_eq!(read(&recovered).1["count"], 3);

    server.abort();
    if let Some(db) = database {
        db.statement("drop table blocking_sessions", &[])
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn blocked_requests_wait_per_session_and_release_the_lock_on_errors() {
    verify_blocking(false, false).await;
    verify_blocking(false, true).await;
}

#[tokio::test]
async fn mysql_sessions_use_redis_for_request_locks() {
    let configured = std::env::var_os("OPENRUST_TEST_SESSION_MYSQL_URL").is_some()
        && std::env::var_os("OPENRUST_TEST_REDIS_PORT").is_some();
    assert!(configured || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL_REDIS").is_none());
    if configured {
        verify_blocking(true, false).await;
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

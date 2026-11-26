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
        if let Some(action) = action.strip_prefix("request-") {
            return request_flash_route(request, action).await;
        }
        if action == "old" {
            return Response::json(
                StatusCode::OK,
                json!({
                    "name": request.old(Some("profile.name")),
                    "explicit_null": request.old(Some("profile.description")).is_some(),
                    "all": request.old(None),
                    "has_name": request.session()?.has_old_input(Some("profile.name")),
                    "has_null": request.session()?.has_old_input(Some("profile.description")),
                    "has_any": request.session()?.has_old_input(None),
                }),
            );
        }
        if let Some(action) = action.strip_prefix("cache-") {
            return session_cache_route(request, action).await;
        }
        if let Some(action) = action.strip_prefix("data-") {
            return session_data_route(request, action);
        }
        let session = request.session_mut()?;
        match action.as_str() {
            "seed" => {
                session.put("profile.name", json!("Ada"))?;
                session.flash("message", json!("Saved"))?;
            }
            "flash-input" => {
                session.flash_input(json!({"profile":{"name":"Ada","description":null}}))?
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
                "id": session.get_id(),
                "token": session.token(),
                "name": session.get("profile.name"),
                "message": session.get("message"),
            }),
        )
    })
}

async fn request_flash_route(request: &mut Request, action: &str) -> Result<Response, HttpError> {
    match action {
        "flash" => request.flash().await?,
        "only" => {
            request
                .flash_only(&["profile.description", "page", "missing"])
                .await?
        }
        "except" => {
            request
                .flash_except(&["profile.secret", "password"])
                .await?
        }
        "flush" => request.flush()?,
        _ => {}
    }
    let session = request.session()?;
    Response::json(
        StatusCode::OK,
        json!({
            "all": request.old(None),
            "literal": request.old(Some("profile.name")),
            "name": session.get("profile.name"),
            "token": session.token(),
            "has_any": session.has_old_input(None),
        }),
    )
}

fn session_data_route(request: &mut Request, action: &str) -> Result<Response, HttpError> {
    use openrust::session::SessionError;

    let session = request.session_mut()?;
    if action == "seed" {
        session.put_many(&[
            ("profile.name".into(), json!("before")),
            ("profile.note".into(), Value::Null),
        ])?;
        session.replace(&[("profile.name".into(), json!("Ada"))])?;
        assert!(session.exists("profile.note"));
        session.push("teams", json!("ops"))?;
        session.put("mixed", json!({"2": "existing", "label": true}))?;
        session.push("mixed", json!("new"))?;
        session.put("counter", json!(" 2.5 "))?;
        assert_eq!(session.increment("counter", 2)?, json!(4.5));
        assert_eq!(session.decrement("counter", 1)?, json!(3.5));
        session.put("overflow", json!(i64::MAX))?;
        assert!(session.increment("overflow", 1)?.as_f64().unwrap() >= 9_223_372_036_854_775_808.0);
        assert!(matches!(
            session.increment("teams", 1),
            Err(SessionError::InvalidValue)
        ));
        assert!(matches!(
            session.push("counter", json!(1)),
            Err(SessionError::InvalidValue)
        ));
        assert!(matches!(
            session.remember("unavailable", || Err(SessionError::Storage)),
            Err(SessionError::Storage)
        ));
        assert!(session.missing("unavailable"));
    } else if action == "continue" {
        session.push("teams", json!("dev"))?;
        assert_eq!(session.increment("counter", -1)?, json!(2.5));
    }
    let remembered = if action == "seed" {
        session.remember("profile.note", || Ok(json!("computed")))?
    } else {
        session.remember("profile.note", || panic!("callback must be lazy"))?
    };
    session.put("explicit_null", Value::Null)?;
    assert!(session.exists("explicit_null") && !session.has("explicit_null"));
    assert!(!session.missing("explicit_null") && session.missing("absent"));
    assert!(session.has_any(&["absent", "profile.name"]));
    let only = session.only(&["profile.name", "explicit_null"]);
    let except = session.except(&["profile.note", "mixed"]);
    assert!(!except.contains_key("mixed"));
    assert_eq!(session.get("profile.note"), Some(&remembered));
    Response::json(
        StatusCode::OK,
        json!({
            "only": only, "profile": except["profile"], "remembered": remembered,
            "teams": session.get("teams"), "mixed": session.get("mixed"),
            "counter": session.get("counter"),
        }),
    )
}

async fn session_cache_route(request: &mut Request, action: &str) -> Result<Response, HttpError> {
    let cache = request.session()?.cache()?;
    let other = Cache::build(json!({"driver": "session", "key": "other.cache"}))?;
    let named = Cache::store(Some("session"))?;
    let owner = Cache::get_facade_root()
        .map_err(CacheError::Unavailable)?
        .get_container()
        .ok_or(CacheError::SessionUnavailable)?;
    let underlying = SessionStore::new(Arc::downgrade(&owner), "widgets".into())?;
    assert_eq!(
        underlying.item_key("dashboard.banner"),
        "widgets.dashboard.banner"
    );
    assert_eq!(underlying.get_prefix(), "");
    assert!(Arc::ptr_eq(&cache, &named));
    assert!(!cache.supports_tags() && !cache.supports_flushing_locks());
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
            "id": request.session()?.get_id(),
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
    Route::r#match(
        &[openrust::http::Method::GET, openrust::http::Method::POST],
        "/session/{action}",
        route,
    )?;
    Ok(())
}

fn register_blocked() -> Result<(), RoutingError> {
    Route::get_with("/session/{action}", blocked_route, |route| {
        route.block(10, 2);
        Ok(())
    })?;
    Route::get_with("/session/immediate/{action}", blocked_route, |route| {
        route.block(10, 0);
        Ok(())
    })?;
    Route::get_with("/session/default/{action}", blocked_route, |route| {
        route.block(10, 2).without_blocking();
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
        Response::json(
            StatusCode::OK,
            json!({"id":session.get_id(), "count":count}),
        )
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
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
        .with_config(load)
        .with_middleware(move |middleware| {
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
        .with_routing(Routing::default().web(register))
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
        assert!(Encrypter::appears_encrypted(&payload));
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
        "file" => builder.with_config(|_, _| Ok(config("file"))),
        "native" => builder.with_config(|_, _| Ok(config("native"))),
        "null" => builder.with_config(|_, _| Ok(config("null"))),
        "cookie" => builder.with_config(|_, _| Ok(config("cookie"))),
        "cookie-clear" => builder.with_config(|_, _| Ok(config("cookie-clear"))),
        "cookie-lock" => builder.with_config(|_, _| Ok(config("cookie-lock"))),
        _ => unreachable!(),
    }
    .with_routing(Routing::default().web(register))
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
    let decoded = encrypter.decrypt_string(&ciphertext).unwrap();
    let mut envelope: Value = serde_json::from_slice(
        CookieValuePrefix::validate(id, &decoded, encrypter.get_all_keys()).unwrap(),
    )
    .unwrap();
    assert!(Encrypter::appears_encrypted(
        envelope["data"].as_str().unwrap()
    ));
    envelope["expires"] = 0.into();
    let mut expired = CookieValuePrefix::create(id, encrypter.get_key()).to_vec();
    expired.extend(serde_json::to_vec(&envelope).unwrap());
    let expired = encrypter.encrypt_string(&expired).unwrap();
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
    assert!(Encrypter::appears_encrypted(&stored));
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
                .with_config(|base, _| {
                    Ok(serde_json::from_slice(
                        &std::fs::read(base.join("configuration.json")).unwrap(),
                    )
                    .unwrap())
                })
                .with_routing(Routing::default().web(register))
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
async fn session_data_operations_persist_on_mysql() {
    let configured = std::env::var_os("OPENRUST_TEST_SESSION_MYSQL_URL").is_some();
    assert!(configured || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL").is_none());
    if !configured {
        return;
    }
    let (address, server, database) = start("database", true).await;
    let seeded = send(address, "data-seed", "").await;
    let (cookie, seeded) = read(&seeded);
    assert_eq!(seeded["only"], json!({"explicit_null": null}));
    assert_eq!(seeded["profile"], json!({"name": "Ada"}));
    assert_eq!(
        seeded["mixed"],
        json!({"2": "existing", "label": true, "3": "new"})
    );
    assert_eq!(seeded["remembered"], "computed");
    let updated = send(address, "data-continue", cookie).await;
    let (cookie, updated) = read(&updated);
    assert_eq!(updated["teams"], json!(["ops", "dev"]));
    assert_eq!(updated["counter"], json!(2.5));
    assert_eq!(read(&send(address, "data-read", cookie).await).1, updated);
    server.abort();
    database
        .unwrap()
        .statement("DROP TABLE sessions", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn request_sessions_round_trip_through_mysql() {
    let configured = std::env::var_os("OPENRUST_TEST_SESSION_MYSQL_URL").is_some();
    assert!(configured || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL").is_none());
    if configured {
        verify("database").await;
        verify_request_flashing("database").await;
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
        .with_config(match (mysql_redis, global) {
            (true, _) => |root, _| Ok(config(root, true, false)),
            (false, true) => |root, _| Ok(config(root, false, true)),
            (false, false) => |root, _| Ok(config(root, false, false)),
        })
        .with_routing(Routing::default().web(register_blocked))
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
    assert!(store.is_started());
    assert_eq!(store.get("message"), Some(&json!("Saved")));
    store.save().await.unwrap();
    assert!(!store.is_started());

    let mut reader = Store::new("session", Box::new(storage)).unwrap();
    reader.set_id(Some(store.get_id())).unwrap();
    reader.start().await.unwrap();
    assert_eq!(reader.get("message"), Some(&json!("Saved")));
    reader.save().await.unwrap();
    reader.start().await.unwrap();
    assert!(reader.get("message").is_none());
}

#[tokio::test]
async fn provider_registers_a_request_bound_session_driver_per_application() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use openrust::{
        contracts::session::SessionHandler as _,
        session::{ArraySessionHandler, SessionError},
        support::{ProviderResult, ServiceProvider, facades::Session as SessionFacade},
    };

    struct Custom {
        storage: ArraySessionHandler,
        resolutions: Arc<AtomicUsize>,
    }

    impl ServiceProvider for Custom {
        fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                let storage = self.storage.clone();
                let resolutions = self.resolutions.clone();
                SessionFacade::extend("redis", move |_, request| {
                    if !request.headers().contains_key("user-agent") {
                        return Err(SessionError::Configuration);
                    }
                    resolutions.fetch_add(1, Ordering::Relaxed);
                    Ok(storage.clone())
                })?;
                Ok(())
            })
        }
    }

    let start =
        async || {
            let root = tempfile::tempdir().unwrap();
            let storage = ArraySessionHandler::new(Duration::from_secs(60));
            let resolutions = Arc::new(AtomicUsize::new(0));
            let app =
                Application::configure(root.path().to_path_buf())
                    .with_config(|_, _| {
                        Ok([
                ("app".into(), json!({"key": "a".repeat(32)})),
                ("session".into(), json!({
                    "driver": "redis", "store": "unavailable",
                    "cookie": "probe_session", "encrypt": true, "expire_on_close": true,
                })),
            ].into())
                    })
                    .with_providers(vec![Box::new(Custom {
                        storage: storage.clone(),
                        resolutions: resolutions.clone(),
                    }) as Box<dyn ServiceProvider>])
                    .with_routing(Routing::default().web(register))
                    .create()
                    .unwrap();
            let mut kernel = Kernel::new(app);
            kernel.bootstrap().await.unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let service = transport.with_state(Arc::new(kernel)).into_make_service();
            let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
            (root, storage, resolutions, address, server)
        };
    let (_root, mut storage, calls, first, first_server) = start().await;
    let (_other, _, other_calls, second, second_server) = start().await;
    let seeded = send(first, "seed", "").await;
    let (cookie, state) = read(&seeded);
    let payload = storage
        .read(state["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(Encrypter::appears_encrypted(
        std::str::from_utf8(&payload).unwrap()
    ));
    assert_eq!(read(&send(first, "read", cookie).await).1["name"], "Ada");
    assert!(read(&send(second, "read", cookie).await).1["name"].is_null());
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(other_calls.load(Ordering::Relaxed), 1);
    first_server.abort();
    second_server.abort();
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
            "name":"Ada","explicit_null":true,"all":{"profile":{"name":"Ada","description":null}},
            "has_name":true,"has_null":false,"has_any":true,
        })
    );
    let expired = send(address, "old", cookie).await;
    let (_, values) = read(&expired);
    assert_eq!(
        values,
        json!({"name":null,"explicit_null":false,"all":null,"has_name":false,"has_null":false,"has_any":false})
    );
    server.abort();
}

#[tokio::test]
async fn request_flash_helpers_follow_the_session_lifecycle() {
    verify_request_flashing("array").await;
}

async fn verify_request_flashing(driver: &'static str) {
    let (address, server, database) = start(driver, true).await;
    let seeded = send(address, "seed", "").await;
    let (cookie, initial) = read(&seeded);
    let client = reqwest::Client::new();
    let flash = client
        .post(format!("http://{address}/session/request-flash?page=2"))
        .header("cookie", cookie)
        .json(&json!({
            "profile":{"name":"nested","description":null},
            "profile.name":"literal",
            "title":"  Trimmed  ",
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let flash: Value = flash.json().await.unwrap();
    let expected = json!({
        "profile":{"name":"nested","description":null},
        "profile.name":"literal",
        "title":"Trimmed",
        "page":"2",
    });
    assert_eq!(flash["all"], expected);
    let next = send(address, "request-old", cookie).await;
    let (_, next) = read(&next);
    assert_eq!(next["all"], expected);
    assert_eq!(next["literal"], "literal");
    assert!(read(&send(address, "request-old", "").await).1["all"].is_null());
    assert!(read(&send(address, "request-old", cookie).await).1["all"].is_null());

    let only = client
        .post(format!("http://{address}/session/request-only?page=3"))
        .header("cookie", cookie)
        .json(&json!({"profile":{"description":null,"secret":"excluded"},"extra":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let only: Value = only.json().await.unwrap();
    assert_eq!(
        only["all"],
        json!({"profile":{"description":null},"page":"3"})
    );

    let except = client
        .post(format!("http://{address}/session/request-except?page=4"))
        .header("cookie", cookie)
        .multipart(
            reqwest::multipart::Form::new()
                .text("profile[name]", "Ada")
                .text("profile[secret]", "excluded")
                .text("password", "excluded")
                .part(
                    "document",
                    reqwest::multipart::Part::text("file data").file_name("fixture.txt"),
                ),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let except: Value = except.json().await.unwrap();
    assert_eq!(except["all"], json!({"profile":{"name":"Ada"},"page":"4"}));
    let flushed = send(address, "request-flush", cookie).await;
    let (_, flushed) = read(&flushed);
    assert_eq!(flushed["all"], json!([]));
    assert_eq!(flushed["has_any"], false);
    assert_eq!(flushed["name"], "Ada");
    assert_eq!(flushed["token"], initial["token"]);
    let (_, continued) = read(&send(address, "request-old", cookie).await);
    assert_eq!(continued["all"], json!([]));
    let (_, expired) = read(&send(address, "request-old", cookie).await);
    assert!(expired["all"].is_null());
    assert_eq!(expired["token"], initial["token"]);
    server.abort();
    if let Some(database) = database {
        database
            .statement("DROP TABLE sessions", &[])
            .await
            .unwrap();
    }
}

#[test]
fn session_names_preserve_unicode_and_defer_cookie_validation() {
    use openrust::session::{ArraySessionHandler, Store};
    for name in ["初始化_会话", "", "name with spaces", "name/with/slash"] {
        let mut store = Store::new(
            name,
            Box::new(ArraySessionHandler::new(Duration::from_secs(60))),
        )
        .unwrap();
        assert_eq!(store.get_name(), name);
        store.set_name("另一个会话").unwrap();
        assert_eq!(store.get_name(), "另一个会话");
    }
}

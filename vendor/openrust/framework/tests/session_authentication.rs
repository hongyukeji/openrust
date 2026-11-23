#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthManager, AuthResult, Credentials},
    contracts::{
        auth::{Authenticatable, Guard as _, StatefulGuard as _},
        config::Repository,
        container::Container as _,
        foundation::Application as _,
        hashing::Hasher,
        http::Kernel as _,
        session::Session as _,
    },
    cookie::CookieValuePrefix,
    database::{
        DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    encryption::{Cipher, Encrypter},
    foundation::{Application, configuration::Routing, http::Kernel},
    hashing::HashOptions,
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::RoutingError,
    support::facades::{Auth, Route},
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct User(ModelState);

impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("auth_test_users")
            .primaryKey("identity")
            .incrementing(false)
            .timestamps(None, None);
        &META
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

impl Authenticatable for User {
    fn getAuthIdentifierName(&self) -> &str {
        "identity"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        self.getKey().unwrap().try_into().unwrap()
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        field(self, "password")
    }
    fn getRememberToken(&self) -> Option<&str> {
        field(self, "remember_token")
    }
    fn setRememberToken(&mut self, token: Option<String>) {
        self.setAttribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
    fn getAuthAttribute(&self, name: &str) -> AuthResult<Option<Value>> {
        self.getAttributeValue(name)
            .map_err(|source| AuthError::Backend(Box::new(source)))
    }
}

fn field<'a>(user: &'a User, name: &str) -> Option<&'a str> {
    match user.getAttribute(name) {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}

fn web() -> Result<(), RoutingError> {
    Route::get("/session", session)?;
    Route::get("/quiet", quiet_session)?;
    Route::get("/selected", selected_session)?;
    Route::post("/login", login)?;
    Route::post("/logout", logout)?;
    Route::post("/logout-current", logout_current)?;
    Route::group(openrust::routing::RouteGroup::new(), |router| {
        router.get("/private", session)?.middleware_name("auth");
        router
            .get("/quota", session)?
            .middleware_name("throttle:quota,1,web-quota");
        router
            .get("/private/{account}", session)?
            .model::<User>("account")?
            .middleware_name("auth");
        router.get("/guest", session)?.middleware_name("guest");
        Ok(())
    })
}

fn api() -> Result<(), RoutingError> {
    Route::get("/token", token)?;
    Route::get("/request", custom_request)?;
    Route::group(openrust::routing::RouteGroup::new(), |router| {
        router
            .get("/quota", token)?
            .middleware_name("auth:api")
            .middleware_name("throttle:quota,1,api-quota");
        Ok(())
    })?;
    Ok(())
}

fn token(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = Auth::guard(request, Some("api"))?.id().await?;
        Response::json(StatusCode::OK, json!({"user":id}))
    })
}

fn custom_request(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = Auth::guard(request, Some("custom"))?.id().await?;
        Response::json(StatusCode::OK, json!({"user":id}))
    })
}

fn session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut guard = Auth::sessionGuard(request, None)?;
        let id = guard.id().await?;
        let remember = guard.viaRemember();
        let session = request.session()?;
        Response::json(
            StatusCode::OK,
            json!({
                "user":id, "remember":remember, "id":session.getId(), "csrf":session.token()
            }),
        )
    })
}

fn quiet_session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(
        async move { Response::json(StatusCode::OK, json!({"id": request.session()?.getId()})) },
    )
}

fn selected_session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Auth::shouldUse(request, "custom")?;
        quiet_session(request).await
    })
}

fn login(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let email = request
            .input(Some("email"))
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(HttpError::InvalidRequest)?;
        let password = request
            .input(Some("password"))
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(HttpError::InvalidRequest)?;
        let remember = request
            .input(Some("remember"))
            .await?
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let key = email.to_ascii_lowercase();
        let credentials = Credentials::new([("email", Value::from(email))], password);
        let authenticated = Auth::sessionGuard(request, None)?
            .attemptThrottled(&credentials, remember, &key, 2, 60)
            .await?;
        if !authenticated {
            return Err(HttpError::Unauthenticated);
        }
        session(request).await
    })
}

fn logout(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Auth::sessionGuard(request, None)?
            .logoutAndInvalidate()
            .await?;
        session(request).await
    })
}

fn logout_current(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Auth::sessionGuard(request, None)?
            .logoutCurrentDevice()
            .await?;
        quiet_session(request).await
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

struct Wire {
    headers: String,
    body: Json,
}

impl Wire {
    fn cookie(&self, name: &str) -> String {
        let prefix = format!("set-cookie: {name}=");
        let line = self
            .headers
            .lines()
            .find(|line| line.starts_with(&prefix))
            .expect(name);
        line.trim_start_matches("set-cookie: ")
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }
}

async fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    body: &str,
    status: u16,
) -> Wire {
    timeout(Duration::from_secs(7), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let wire = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\nContent-Type: application/json\r\nSec-Fetch-Site: same-origin\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len());
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path}: {response}");
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        Wire { headers: headers.to_owned(), body: if body.is_empty() || (300..400).contains(&status) { Json::Null } else { serde_json::from_str(body).unwrap() } }
    }).await.unwrap()
}

async fn get(address: SocketAddr, path: &str, cookie: &str, status: u16) -> Wire {
    send(
        address,
        "GET",
        path,
        &format!("Cookie: {cookie}\r\n"),
        "",
        status,
    )
    .await
}

async fn assert_session_user(
    connection: &openrust::database::Connection,
    session: &Wire,
    user: Value,
) {
    let row = connection
        .table("auth_test_sessions")
        .r#where("id", "=", session.body["id"].as_str().unwrap())
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("user_id"), Some(&user));
}

async fn verify(database: Json, database_sessions: bool) {
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| Ok([
            ("app".into(), json!({"key":"a".repeat(32)})),
            ("session".into(), json!({"driver":"array","cookie":"auth_session","lottery":[0,100]})),
            ("cache".into(), json!({"default":"array","stores":{"array":{"driver":"array"}}})),
            ("hashing".into(), json!({"driver":"bcrypt","bcrypt":{"rounds":5}})),
            ("auth".into(), json!({
                "defaults":{"guard":"web"},
                "guards":{
                    "web":{"driver":"session","provider":"users"},
                    "api":{"driver":"token","provider":"database"},
                    "custom":{"driver":"custom-request"}
                },
                "providers":{
                    "users":{"driver":"eloquent","model":"TestUser"},
                    "database":{"driver":"database","table":"auth_test_users","identifier":"identity"}
                }
            })),
        ].into()))
        .withRouting(Routing::default().web(web).api(api)).create().unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config.set("database.default", json!("test")).unwrap();
    config.set("database.connections.test", database).unwrap();
    if database_sessions {
        config.set("session.driver", json!("database")).unwrap();
        config
            .set("session.table", json!("auth_test_sessions"))
            .unwrap();
        config.set("session.encrypt", json!(true)).unwrap();
    }
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    let manager = app.container().make::<AuthManager>().unwrap();
    manager.registerModel::<User>("TestUser");
    let provider = manager.createUserProvider("database").unwrap();
    manager.viaRequest("custom-request", move |request| {
        let provider = provider.clone();
        Box::pin(async move {
            if request
                .headers()
                .get("x-test-user")
                .is_some_and(|value| value == "001")
            {
                return provider
                    .retrieveById(&AuthIdentifier::String("001".into()))
                    .await;
            }
            Ok(None)
        })
    });
    let hash = hasher
        .make(
            b"good",
            &HashOptions::from_iter([("rounds".into(), json!(4))]),
        )
        .await
        .unwrap();
    for sql in [
        "DROP TABLE IF EXISTS auth_test_users",
        "CREATE TABLE auth_test_users (identity VARCHAR(20) PRIMARY KEY, email VARCHAR(50), password VARCHAR(100), remember_token VARCHAR(100), api_token VARCHAR(100), quota INTEGER)",
    ] {
        connection.statement(sql, &[]).await.unwrap();
    }
    connection.statement("INSERT INTO auth_test_users VALUES ('001', 'a@example.test', ?, NULL, 'CaseSensitive', 1)", &[hash.clone().into()]).await.unwrap();
    if database_sessions {
        for sql in [
            "DROP TABLE IF EXISTS auth_test_sessions",
            "CREATE TABLE auth_test_sessions (id VARCHAR(40) PRIMARY KEY, user_id VARCHAR(20), ip_address VARCHAR(45), user_agent TEXT, payload LONGTEXT NOT NULL, last_activity BIGINT NOT NULL)",
        ] {
            connection.statement(sql, &[]).await.unwrap();
        }
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });

    assert_eq!(
        get(address, "/private", "", 401).await.body["message"],
        "Unauthenticated."
    );
    get(address, "/private/missing", "", 401).await;
    let initial = get(address, "/session", "", 200).await;
    if database_sessions {
        assert_session_user(&connection, &initial, Value::Null).await;
    }
    let old_session = initial.cookie("auth_session");
    let guest_header = format!("Cookie: {old_session}\r\n");
    let rejected = r#"{"email":"other@example.test","password":"wrong"}"#;
    let (one, two) = tokio::join!(
        send(address, "POST", "/login", "", rejected, 401),
        send(address, "POST", "/login", "", rejected, 401)
    );
    assert_eq!(one.body, two.body);
    let limited = send(address, "POST", "/login", "", rejected, 429).await;
    assert!(limited.headers.contains("retry-after:"));
    let login = send(
        address,
        "POST",
        "/login",
        &guest_header,
        r#"{"email":"a@example.test","password":"good","remember":true}"#,
        200,
    )
    .await;
    assert_eq!(login.body["user"], "001");
    if database_sessions {
        assert_session_user(&connection, &login, "001".into()).await;
    }
    let stored = connection
        .table("auth_test_users")
        .first()
        .await
        .unwrap()
        .unwrap();
    let Some(Value::Text(updated)) = stored.get("password") else {
        panic!("missing password hash");
    };
    assert_ne!(updated, &hash);
    assert!(
        !hasher
            .needsRehash(updated, &HashOptions::default())
            .unwrap()
    );

    assert_ne!(login.body["id"], initial.body["id"]);
    assert_ne!(login.body["csrf"], initial.body["csrf"]);
    let logged_in = login.cookie("auth_session");
    let quota = get(address, "/quota", &logged_in, 200).await;
    assert!(quota.headers.contains("x-ratelimit-limit: 1"));
    get(address, "/quota", &logged_in, 429).await;
    let remember = login.cookie("remember_web");
    let encrypter = Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap();
    let encrypted = percent_encoding::percent_decode_str(remember.split_once('=').unwrap().1)
        .decode_utf8()
        .unwrap();
    let clear = encrypter.decryptString(&encrypted).unwrap();
    let clear =
        CookieValuePrefix::validate("remember_web", &clear, encrypter.getAllKeys()).unwrap();
    let recaller: Json = serde_json::from_slice(clear).unwrap();
    assert_eq!(recaller["id"], "001");
    assert!(
        login
            .headers
            .lines()
            .any(|line| line.starts_with("set-cookie: remember_web=")
                && line.to_ascii_lowercase().contains("httponly"))
    );
    get(address, "/private/missing", &logged_in, 404).await;
    let (member, guest) = tokio::join!(
        get(address, "/private", &logged_in, 200),
        get(address, "/session", "", 200)
    );
    assert_eq!(member.body["user"], "001");
    assert!(guest.body["user"].is_null());
    assert!(get(address, "/session", &old_session, 200).await.body["user"].is_null());
    assert!(
        get(address, "/guest", &logged_in, 302)
            .await
            .headers
            .contains("location: /")
    );
    if database_sessions {
        connection
            .statement(
                "UPDATE auth_test_sessions SET user_id = NULL WHERE id = ?",
                &[login.body["id"].as_str().unwrap().into()],
            )
            .await
            .unwrap();
        let quiet = get(address, "/quiet", &logged_in, 200).await;
        assert_eq!(quiet.body["id"], login.body["id"]);
        assert_session_user(&connection, &quiet, "001".into()).await;
        let selected = send(address, "GET", "/selected", "X-Test-User: 001\r\n", "", 200).await;
        assert_session_user(&connection, &selected, "001".into()).await;
        let default = get(address, "/quiet", &selected.cookie("auth_session"), 200).await;
        assert_eq!(default.body["id"], selected.body["id"]);
        assert_session_user(&connection, &default, Value::Null).await;

        let current = send(
            address,
            "POST",
            "/logout-current",
            &format!("Cookie: {logged_in}\r\n"),
            "",
            200,
        )
        .await;
        assert_eq!(current.body["id"], login.body["id"]);
        assert_session_user(&connection, &current, Value::Null).await;
    }

    let recalled = get(address, "/session", &remember, 200).await;
    assert_eq!(recalled.body["user"], "001");
    assert_eq!(recalled.body["remember"], true);
    if database_sessions {
        assert_session_user(&connection, &recalled, "001".into()).await;
    }
    let restored = recalled.cookie("auth_session");
    let logout = send(
        address,
        "POST",
        "/logout",
        &format!("Cookie: {restored}; {remember}\r\n"),
        "",
        200,
    )
    .await;
    assert!(logout.body["user"].is_null());
    if database_sessions {
        assert_session_user(&connection, &logout, Value::Null).await;
    }
    assert_ne!(logout.body["id"], recalled.body["id"]);
    assert_ne!(logout.body["csrf"], recalled.body["csrf"]);
    assert!(get(address, "/session", &remember, 200).await.body["user"].is_null());
    assert!(get(address, "/session", &restored, 200).await.body["user"].is_null());

    let bearer = send(
        address,
        "GET",
        "/api/token",
        "Authorization: Bearer CaseSensitive\r\n",
        "",
        200,
    )
    .await;
    assert_eq!(bearer.body["user"], "001");
    assert!(!bearer.headers.contains("set-cookie:"));
    let bearer_header = "Authorization: Bearer CaseSensitive\r\n";
    let quota = send(address, "GET", "/api/quota", bearer_header, "", 200).await;
    assert!(quota.headers.contains("x-ratelimit-limit: 1"));
    send(address, "GET", "/api/quota", bearer_header, "", 429).await;
    assert!(
        send(
            address,
            "GET",
            "/api/token",
            "Authorization: Bearer casesensitive\r\n",
            "",
            200
        )
        .await
        .body["user"]
            .is_null()
    );
    assert_eq!(
        send(
            address,
            "GET",
            "/api/request",
            "X-Test-User: 001\r\n",
            "",
            200
        )
        .await
        .body["user"],
        "001"
    );
    assert!(get(address, "/api/request", "", 200).await.body["user"].is_null());

    let second_login = send(
        address,
        "POST",
        "/login",
        "",
        r#"{"email":"a@example.test","password":"good","remember":true}"#,
        200,
    )
    .await;
    let old_remember = second_login.cookie("remember_web");
    let new_hash = hasher
        .make(b"changed", &HashOptions::default())
        .await
        .unwrap();
    connection
        .statement(
            "UPDATE auth_test_users SET password = ?",
            &[new_hash.into()],
        )
        .await
        .unwrap();
    assert!(get(address, "/session", &old_remember, 200).await.body["user"].is_null());

    server.abort();
    if database_sessions {
        connection
            .statement("DROP TABLE auth_test_sessions", &[])
            .await
            .unwrap();
    }
    connection
        .statement("DROP TABLE auth_test_users", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn session_authentication_on_sqlite() {
    verify(json!({"driver":"sqlite","database":":memory:"}), false).await;
}

#[tokio::test]
async fn session_authentication_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), true).await;
    }
}

#[path = "session_authentication/extensions.rs"]
mod extensions;

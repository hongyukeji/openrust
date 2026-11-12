use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{AuthIdentifier, AuthManager, Credentials},
    contracts::{
        auth::{Authenticatable, Guard as _, StatefulGuard as _},
        config::Repository,
        container::Container as _,
        foundation::Application as _,
        hashing::Hasher,
        http::Kernel as _,
        session::Session as _,
    },
    database::{
        DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
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
    fn get_auth_identifier_name(&self) -> &str {
        "identity"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.getKey().unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        field(self, "password")
    }
    fn get_remember_token(&self) -> Option<&str> {
        field(self, "remember_token")
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.setAttribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
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
    Route::r#match(&[Method::POST], "/login", login)?;
    Route::r#match(&[Method::POST], "/logout", logout)?;
    Route::group(openrust::routing::RouteGroup::new(), |router| {
        router.get("/private", session)?.middleware_name("auth");
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
        Wire { headers: headers.to_owned(), body: if body.is_empty() { Json::Null } else { serde_json::from_str(body).unwrap() } }
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

async fn verify(database: Json) {
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
                    .retrieve_by_id(&AuthIdentifier::String("001".into()))
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
        "CREATE TABLE auth_test_users (identity VARCHAR(20) PRIMARY KEY, email VARCHAR(50), password VARCHAR(100), remember_token VARCHAR(100), api_token VARCHAR(100))",
    ] {
        connection.statement(sql, &[]).await.unwrap();
    }
    connection.statement("INSERT INTO auth_test_users VALUES ('001', 'a@example.test', ?, NULL, 'CaseSensitive')", &[hash.clone().into()]).await.unwrap();
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
    let remember = login.cookie("remember_web");
    assert!(!remember.contains("001"));
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

    let recalled = get(address, "/session", &remember, 200).await;
    assert_eq!(recalled.body["user"], "001");
    assert_eq!(recalled.body["remember"], true);
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
    connection
        .statement("DROP TABLE auth_test_users", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn session_authentication_on_sqlite() {
    verify(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn session_authentication_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await;
    }
}

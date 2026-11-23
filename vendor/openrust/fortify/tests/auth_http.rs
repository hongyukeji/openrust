#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{Auth, AuthError, AuthFuture, AuthIdentifier, AuthManager, AuthResult, Credentials},
    contracts::{
        auth::{Authenticatable, UserProvider},
        container::Container as _,
        foundation::Application as _,
        hashing::Hasher,
        http::Kernel as _,
        session::Session as _,
    },
    database::Value,
    foundation::{Application, configuration::Routing, http::Kernel},
    hashing::HashOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Redirect, Route},
    },
};
use openrust_fortify::{Fortify, FortifyServiceProvider};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};
#[derive(Clone)]
struct Account {
    hash: String,
    token: Option<String>,
}

impl Authenticatable for Account {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        "001".into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        Some(&self.hash)
    }
    fn get_remember_token(&self) -> Option<&str> {
        self.token.as_deref()
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.token = token;
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        Ok(match name {
            "email" => Some("a@example.test".into()),
            "username" => Some("tester".into()),
            _ => None,
        })
    }
}

struct Accounts {
    user: Mutex<Account>,
    hasher: Arc<dyn Hasher>,
}

impl Accounts {
    fn snapshot(&self) -> Box<dyn Authenticatable> {
        Box::new(self.user.lock().unwrap().clone())
    }
}

impl UserProvider for Accounts {
    fn retrieve_by_id<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok((*id == AuthIdentifier::from("001")).then(|| self.snapshot())) })
    }

    fn retrieve_by_token<'a>(
        &'a self,
        id: &'a AuthIdentifier,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async {
            Ok(self
                .retrieve_by_id(id)
                .await?
                .filter(|user| user.get_remember_token() == Some(token)))
        })
    }

    fn update_remember_token<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async {
            self.user.lock().unwrap().token = Some(token.to_owned());
            user.set_remember_token(Some(token.to_owned()));
            Ok(())
        })
    }

    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async {
            let matches = !credentials.fields().is_empty()
                && credentials
                    .fields()
                    .iter()
                    .all(|(name, value)| match name.as_str() {
                        "email" => *value == "a@example.test".into(),
                        "username" => *value == "tester".into(),
                        _ => false,
                    });
            Ok(matches.then(|| self.snapshot()))
        })
    }

    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async {
            self.hasher
                .check(
                    credentials.password().as_bytes(),
                    user.get_auth_password().unwrap(),
                    &HashOptions::new(),
                )
                .await
                .map_err(|error| AuthError::Backend(Box::new(error)))
        })
    }

    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a mut dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            if force {
                let hash = self
                    .hasher
                    .make(credentials.password().as_bytes(), &HashOptions::new())
                    .await
                    .map_err(|error| AuthError::Backend(Box::new(error)))?;
                self.user.lock().unwrap().hash = hash;
            }
            Ok(())
        })
    }
}

struct Settings(Json);
impl ServiceProvider for Settings {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app
                .container()
                .get::<dyn openrust::contracts::config::Repository>("config")?;
            config.set("fortify", self.0.clone())?;
            Ok(())
        })
    }
}
fn current(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::guard(request, None)?.id().await?;
        let session = request.session()?;
        Response::json(
            StatusCode::OK,
            json!({"user":user,"id":session.get_id(),"csrf":session.token(),"confirmed":session.get("auth.password_confirmed_at"),"errors":session.get("errors")}),
        )
    })
}
fn intended(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Redirect::set_intended_url(request, "/settings?tab=security")?;
        current(request).await
    })
}
fn age(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let now = openrust::support::now(None)
            .map_err(|error| HttpError::Internal {
                source: Box::new(error),
            })?
            .as_date_time()
            .timestamp()
            .as_nanosecond()
            .div_euclid(1_000_000_000) as i64;
        request
            .session_mut()?
            .put("auth.password_confirmed_at", json!(now - 950))?;
        current(request).await
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/state", current)?;
    Route::get("/intended", intended)?;
    Route::get("/age", age)?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
struct Server {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    database: Option<openrust::database::Connection>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort()
    }
}
async fn start(settings: Json, configure: impl FnOnce(&Application)) -> Server {
    start_with_store(settings, None, configure).await
}
async fn start_with_store(
    settings: Json,
    mysql: Option<&str>,
    configure: impl FnOnce(&Application),
) -> Server {
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([
  ("app".into(),json!({"key":"a".repeat(32),"timezone":"UTC","url":"http://localhost"})),
  ("session".into(),json!({"driver":"array","cookie":"fortify_session","lottery":[0,100]})),
  ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
  ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
  ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"test-user"}},"password_timeout":900})),
 ].into())).with_providers(vec![Box::new(Settings(settings)),Box::new(FortifyServiceProvider)]).with_routing(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    let hash = hasher
        .make(b"good:secret", &HashOptions::new())
        .await
        .unwrap();
    let provider = Arc::new(Accounts {
        user: Mutex::new(Account { hash, token: None }),
        hasher,
    });
    app.container()
        .make::<AuthManager>()
        .unwrap()
        .provider("test-user", move |_, _| Ok(provider.clone()));
    let mut database = None;
    if let Some(driver) = mysql {
        let config = app
            .container()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap();
        let url = std::env::var("OPENRUST_TEST_MYSQL_URL").expect("MySQL acceptance URL");
        config
            .set("database.default", json!("fortify_mysql"))
            .unwrap();
        config
            .set("database.connections.fortify_mysql", json!({"url":url}))
            .unwrap();
        config.set("auth.providers.users",if driver=="eloquent" {json!({"driver":"eloquent","model":"FortifyUser"})}else{json!({"driver":"database","table":"fortify_auth_mysql_users","identifier":"identity"})}).unwrap();
        config.set("session.driver", json!("database")).unwrap();
        config
            .set("session.table", json!("fortify_auth_mysql_sessions"))
            .unwrap();
        let connection = app
            .container()
            .make::<openrust::database::DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap();
        let version = connection
            .select_one("SELECT VERSION() AS version", &[])
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(version.get("version"),Some(Value::Text(version)) if version.starts_with("8.4."))
        );
        for sql in [
            "DROP TABLE IF EXISTS fortify_auth_mysql_sessions",
            "DROP TABLE IF EXISTS fortify_auth_mysql_users",
            "CREATE TABLE fortify_auth_mysql_users (identity VARCHAR(20) PRIMARY KEY,email VARCHAR(100),password VARCHAR(255),remember_token VARCHAR(100))",
            "CREATE TABLE fortify_auth_mysql_sessions (id VARCHAR(255) PRIMARY KEY,user_id VARCHAR(255) NULL,ip_address VARCHAR(45) NULL,user_agent TEXT NULL,payload LONGTEXT NOT NULL,last_activity BIGINT NOT NULL)",
        ] {
            connection.statement(sql, &[]).await.unwrap();
        }
        let hasher = app.container().make::<dyn Hasher>().unwrap();
        let hash = hasher
            .make(b"good:secret", &HashOptions::new())
            .await
            .unwrap();
        connection.statement("INSERT INTO fortify_auth_mysql_users (identity,email,password) VALUES ('001','a@example.test',?)",&[hash.into()]).await.unwrap();
        app.container()
            .make::<AuthManager>()
            .unwrap()
            .register_model::<SqlUser>("FortifyUser");
        database = Some(connection);
    }
    configure(&app);
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    Server {
        address,
        task: tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
        database,
    }
}
struct Wire {
    headers: String,
    body: Json,
}
struct Client {
    address: SocketAddr,
    cookies: BTreeMap<String, String>,
    html: bool,
    fetch: bool,
}
impl Client {
    fn new(server: &Server) -> Self {
        Self {
            address: server.address,
            cookies: BTreeMap::new(),
            html: false,
            fetch: true,
        }
    }
    async fn send(&mut self, method: &str, path: &str, body: &str, status: u16) -> Wire {
        timeout(Duration::from_secs(7),async {
   let mut socket=TcpStream::connect(self.address).await.unwrap();
   let cookies=self.cookies.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join("; ");
   let accept=if self.html {"text/html"} else {"application/json"};let fetch=if self.fetch {"Sec-Fetch-Site: same-origin\r\n"}else{""};
   let wire=format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: {accept}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCookie: {cookies}\r\n{fetch}\r\n{body}",self.address,body.len());
   socket.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();assert!(response.starts_with(&format!("HTTP/1.1 {status} ")),"{method} {path}: {response}");
   let (headers,body)=response.split_once("\r\n\r\n").unwrap();
   for line in headers.lines() {if let Some(cookie)=line.strip_prefix("set-cookie: ") {let (name,value)=cookie.split(';').next().unwrap().split_once('=').unwrap();self.cookies.insert(name.into(),value.into());}}
   Wire{headers:headers.into(),body:serde_json::from_str(body).unwrap_or(Json::String(body.into()))}
  }).await.unwrap()
    }
}
const LOGIN: &str = r#"{"email":"a@example.test","password":"good:secret"}"#;
#[tokio::test]
async fn login_validation_session_rotation_confirmation_and_logout_over_http() {
    let server = start(json!({"features":[],"views":false}), |_| {}).await;
    let mut client = Client::new(&server);
    let before = client.send("GET", "/state", "", 200).await.body;
    assert!(before["user"].is_null());
    client.send("GET", "/login", "", 405).await;
    let invalid = client.send("POST", "/login", "{}", 422).await;
    assert!(invalid.body["errors"]["email"].is_array());
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"wrong"}"#,
            422,
        )
        .await;
    let login = client.send("POST", "/login", LOGIN, 200).await;
    assert_eq!(login.body, json!({"two_factor":false}));
    let after = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(after["user"], "001");
    assert_ne!(after["id"], before["id"]);
    assert_ne!(after["csrf"], before["csrf"]);
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"wrong"}"#,
            422,
        )
        .await;
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":null}"#,
            422,
        )
        .await;
    let confirm = client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"good:secret"}"#,
            201,
        )
        .await;
    assert_eq!(confirm.body, "");
    assert_eq!(
        client
            .send("GET", "/user/confirmed-password-status", "", 200)
            .await
            .body,
        json!({"confirmed":true})
    );
    client.send("GET", "/age", "", 200).await;
    assert_eq!(
        client
            .send("GET", "/user/confirmed-password-status", "", 200)
            .await
            .body,
        json!({"confirmed":false})
    );
    client.send("POST", "/logout", "{}", 204).await;
    let out = client.send("GET", "/state", "", 200).await.body;
    assert!(out["user"].is_null());
    assert_ne!(out["id"], after["id"]);
    assert_ne!(out["csrf"], after["csrf"]);
}
#[tokio::test]
async fn five_login_failures_lock_out_then_other_application_remains_independent() {
    let one = start(json!({"features":[],"views":false}), |_| {}).await;
    let two = start(json!({"features":[],"views":false}), |_| {}).await;
    let mut client = Client::new(&one);
    client.send("GET", "/state", "", 200).await;
    for _ in 0..5 {
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"wrong"}"#,
                422,
            )
            .await;
    }
    let lock = client.send("POST", "/login", LOGIN, 429).await;
    assert!(lock.body["errors"]["email"].is_array());
    let mut other = Client::new(&two);
    other.send("POST", "/login", LOGIN, 200).await;
}
#[tokio::test]
async fn web_intended_redirect_and_csrf_rejection_use_native_session_stack() {
    let server = start(
        json!({"features":[],"views":false,"redirects":{"logout":"/bye"}}),
        |_| {},
    )
    .await;
    let mut client = Client::new(&server);
    client.send("GET", "/intended", "", 200).await;
    client.fetch = false;
    client.send("POST", "/login", LOGIN, 419).await;
    client.fetch = true;
    client.html = true;
    let login = client.send("POST", "/login", LOGIN, 302).await;
    assert!(login.headers.contains("/settings?tab=security"));
    let out = client.send("POST", "/logout", "{}", 302).await;
    assert!(out.headers.contains("/bye"));
}

#[tokio::test]
async fn configured_username_lowercasing_prefix_and_path_are_used_by_login() {
    let server=start(json!({"features":[],"views":false,"prefix":"auth","username":"username","lowercase_usernames":true,"paths":{"login":"/sign-in"}}),|_|{}).await;
    let mut client = Client::new(&server);
    let invalid = client
        .send(
            "POST",
            "/auth/sign-in",
            r#"{"email":"a@example.test","password":"good:secret"}"#,
            422,
        )
        .await;
    assert!(invalid.body["errors"]["username"].is_array());
    assert!(invalid.body["errors"]["email"].is_null());
    client
        .send(
            "POST",
            "/auth/sign-in",
            r#"{"username":"TeStEr","password":"good:secret"}"#,
            200,
        )
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["user"],
        "001"
    );
    client.send("POST", "/login", LOGIN, 404).await;
    client.send("POST", "/auth/login", LOGIN, 404).await;
}
#[tokio::test]
async fn successful_login_clears_failed_attempts_and_web_lockout_flashes_errors() {
    let server = start(json!({"features":[],"views":false}), |_| {}).await;
    let mut client = Client::new(&server);
    for _ in 0..4 {
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"wrong"}"#,
                422,
            )
            .await;
    }
    client.send("POST", "/login", LOGIN, 200).await;
    client.send("POST", "/logout", "{}", 204).await;
    for _ in 0..5 {
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"wrong"}"#,
                422,
            )
            .await;
    }
    client.html = true;
    client.send("POST", "/login", LOGIN, 302).await;
    client.html = false;
    assert!(client.send("GET", "/state", "", 200).await.body["errors"].is_object());
}
#[tokio::test]
async fn authentication_and_password_confirmation_callbacks_are_application_scoped() {
    let server = start(json!({"features":[],"views":false}), |app| {
        let fortify = app.container().make::<Fortify>().unwrap();
        fortify.authenticateUsing(Arc::new(|request| {
            Box::pin(async move {
                let accepted = request
                    .input(Some("password"))
                    .await
                    .map_err(|error| AuthError::Http(Box::new(error)))?
                    == Some(json!("override"));
                Ok(accepted.then(|| {
                    Box::new(Account {
                        hash: "callback-user".into(),
                        token: None,
                    }) as Box<dyn Authenticatable>
                }))
            })
        }));
        fortify.confirmPasswordsUsing(Arc::new(|user, password| {
            Box::pin(async move {
                Ok(user.get_auth_identifier() == AuthIdentifier::from("001")
                    && password == Some("custom-confirm"))
            })
        }));
    })
    .await;
    let mut client = Client::new(&server);
    client.send("POST", "/login", LOGIN, 422).await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"override"}"#,
            200,
        )
        .await;
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"good:secret"}"#,
            422,
        )
        .await;
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"custom-confirm"}"#,
            201,
        )
        .await;
    let other = start(json!({"features":[],"views":false}), |_| {}).await;
    let mut other = Client::new(&other);
    other
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"override"}"#,
            422,
        )
        .await;
}
fn custom_pipeline<'a>(
    _: (),
    _: openrust::pipeline::Next<'a, (), Response, HttpError, Request>,
) -> openrust::pipeline::PipelineFuture<'a, Response, HttpError> {
    Box::pin(async { Response::json(StatusCode::ACCEPTED, json!({"custom_pipeline":true})) })
}
#[tokio::test]
async fn callback_pipeline_precedes_configured_stages_and_can_short_circuit() {
    let server = start(
        json!({"features":[],"views":false,"pipelines":{"login":["UnknownStage"]}}),
        |app| {
            app.container()
                .make::<Fortify>()
                .unwrap()
                .authenticateThrough(Arc::new(|_| Ok(vec![Box::new(custom_pipeline)])));
        },
    )
    .await;
    let mut client = Client::new(&server);
    assert_eq!(
        client.send("POST", "/login", LOGIN, 202).await.body,
        json!({"custom_pipeline":true})
    );
    assert!(client.send("GET", "/state", "", 200).await.body["user"].is_null());
}

use openrust::database::eloquent::{Model, ModelMetadata, ModelState};
struct SqlUser(ModelState);

impl Model for SqlUser {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("fortify_auth_mysql_users")
            .primary_key("identity")
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

impl Authenticatable for SqlUser {
    fn get_auth_identifier_name(&self) -> &str {
        "identity"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.get_key().unwrap().try_into().unwrap()
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
        self.set_attribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        self.get_attribute_value(name)
            .map_err(|source| AuthError::Backend(Box::new(source)))
    }
}

fn field<'a>(user: &'a SqlUser, name: &str) -> Option<&'a str> {
    match user.get_attribute(name) {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}

#[tokio::test]
async fn mysql_session_and_database_and_eloquent_user_providers_use_fortify_http() {
    if std::env::var("OPENRUST_TEST_MYSQL_URL").is_err() {
        assert!(
            std::env::var("OPENRUST_REQUIRE_MYSQL").is_err(),
            "required MySQL URL missing"
        );
        return;
    }
    for driver in ["database", "eloquent"] {
        let server =
            start_with_store(json!({"features":[],"views":false}), Some(driver), |_| {}).await;
        let mut client = Client::new(&server);
        let initial = client.send("GET", "/state", "", 200).await.body;
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"good:secret","remember":true}"#,
                200,
            )
            .await;
        let authenticated = client.send("GET", "/state", "", 200).await.body;
        assert_eq!(authenticated["user"], "001");
        assert_ne!(initial["id"], authenticated["id"]);
        let remembered = client
            .cookies
            .iter()
            .find(|(name, _)| name.starts_with("remember_"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .expect("remember cookie issued");
        let mut recalled = Client::new(&server);
        recalled
            .cookies
            .insert(remembered.0.clone(), remembered.1.clone());
        assert_eq!(
            recalled.send("GET", "/state", "", 200).await.body["user"],
            "001"
        );
        let connection = server.database.as_ref().unwrap();
        let row = connection
            .table("fortify_auth_mysql_sessions")
            .r#where("id", "=", authenticated["id"].as_str().unwrap())
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("user_id"), Some(&Value::Text("001".into())));
        let account = connection
            .table("fortify_auth_mysql_users")
            .first()
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(account.get("remember_token"),Some(Value::Text(token)) if !token.is_empty())
        );
        client
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"good:secret"}"#,
                201,
            )
            .await;
        assert_eq!(
            client
                .send("GET", "/user/confirmed-password-status", "", 200)
                .await
                .body,
            json!({"confirmed":true})
        );
        client.send("POST", "/logout", "{}", 204).await;
        let logged_out = client.send("GET", "/state", "", 200).await.body;
        assert!(logged_out["user"].is_null());
        let row = connection
            .table("fortify_auth_mysql_sessions")
            .r#where("id", "=", logged_out["id"].as_str().unwrap())
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("user_id"), Some(&Value::Null));
        let mut revoked = Client::new(&server);
        revoked.cookies.insert(remembered.0, remembered.1);
        assert!(revoked.send("GET", "/state", "", 200).await.body["user"].is_null());
        let old = connection
            .table("fortify_auth_mysql_sessions")
            .r#where("id", "=", authenticated["id"].as_str().unwrap())
            .first()
            .await
            .unwrap();
        assert!(old.is_none());
    }
}

#[tokio::test]
async fn named_limiter_replaces_builtin_login_attempt_limit() {
    let server = start(
        json!({"features":[],"views":false,"limiters":{"login":"fortify-login"}}),
        |app| {
            app.container()
                .make::<openrust::cache::RateLimiter>()
                .unwrap()
                .r#for("fortify-login", |_| {
                    openrust::cache::rate_limiting::Limit::per_minute(2)
                });
        },
    )
    .await;
    let mut client = Client::new(&server);
    for _ in 0..2 {
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"wrong"}"#,
                422,
            )
            .await;
    }
    let limited = client.send("POST", "/login", LOGIN, 429).await;
    assert!(limited.headers.contains("retry-after:"));
    assert!(limited.headers.contains("x-ratelimit-limit: 2"));
}
struct AcceptedLogin;
impl openrust_fortify::contracts::LoginResponse for AcceptedLogin {}
impl openrust_fortify::contracts::Response for AcceptedLogin {
    fn to_response<'a>(
        &'a self,
        _: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { Response::json(StatusCode::ACCEPTED, json!({"replaced":true})) })
    }
}
#[tokio::test]
async fn view_callbacks_and_container_response_replacements_are_application_owned() {
    use openrust_fortify::http::responses::SimpleViewResponse;
    let server = start(json!({"features":[],"views":true,"guard":null}), |app| {
        let fortify = app.container().make::<Fortify>().unwrap();
        fortify.loginView(SimpleViewResponse::callback(Arc::new(|request| {
            Box::pin(
                async move { Response::json(StatusCode::OK, json!({"login_view":request.path()})) },
            )
        })));
        fortify.confirmPasswordView(SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"confirm_view":true})) })
        })));
        app.container()
            .instance::<dyn openrust_fortify::contracts::LoginResponse>(Arc::new(AcceptedLogin))
            .unwrap();
    })
    .await;
    let mut client = Client::new(&server);
    assert_eq!(
        client.send("GET", "/login", "", 200).await.body,
        json!({"login_view":"login"})
    );
    assert_eq!(
        client.send("POST", "/login", LOGIN, 202).await.body,
        json!({"replaced":true})
    );
    assert_eq!(
        client
            .send("GET", "/user/confirm-password", "", 200)
            .await
            .body,
        json!({"confirm_view":true})
    );
    client.send("GET", "/login", "", 302).await;
    let other = start(json!({"features":[],"views":true}), |_| {}).await;
    Client::new(&other).send("GET", "/login", "", 500).await;
}
#[tokio::test]
async fn named_blade_view_uses_native_request_session_context() {
    let directory =
        std::env::temp_dir().join(format!("openrust-fortify-views-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("login.blade.html"),
        "<form>@csrf @guest guest @endguest</form>",
    )
    .unwrap();
    let server = start(json!({"features":[],"views":true}), |app| {
        app.container()
            .instance(Arc::new(openrust::view::Factory::new(
                openrust::view::FileViewFinder::new([directory.clone()]),
                directory.join("compiled"),
            )))
            .unwrap();
        app.container()
            .make::<Fortify>()
            .unwrap()
            .loginView("login");
    })
    .await;
    let mut client = Client::new(&server);
    let page = client.send("GET", "/login", "", 200).await.body;
    let html = page.as_str().unwrap();
    assert!(html.contains("name=\"_token\""));
    assert!(html.contains("guest"));
    let state = client.send("GET", "/state", "", 200).await.body;
    assert!(html.contains(state["csrf"].as_str().unwrap()));
    drop(server);
    std::fs::remove_dir_all(directory).unwrap();
}
#[tokio::test]
async fn password_status_preserves_php_mixed_seconds_comparison() {
    let server = start(json!({"features":[],"views":false}), |_| {}).await;
    let mut client = Client::new(&server);
    client.send("POST", "/login", LOGIN, 200).await;
    client.send("GET", "/age", "", 200).await;
    for (seconds, expected) in [
        (json!(null), false),
        (json!(true), false),
        (json!(false), false),
        (json!([]), true),
        (json!({}), true),
        (json!(1000.5), true),
        (json!("1e4"), true),
        (json!("z"), true),
        (json!("1000x"), false),
        (json!(" 10000 "), true),
        (json!(""), false),
    ] {
        let result = client
            .send(
                "GET",
                "/user/confirmed-password-status",
                &json!({"seconds":seconds}).to_string(),
                200,
            )
            .await;
        assert_eq!(
            result.body,
            json!({"confirmed":expected}),
            "seconds={seconds}"
        );
        assert_eq!(result.headers.contains("x-retry-after:"), expected);
    }
}

struct ObserveAuthentication(Arc<Mutex<Vec<&'static str>>>);
impl openrust::contracts::events::Listener for ObserveAuthentication {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        context: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            if let Some(failed) = event.downcast_ref::<openrust::auth::events::Failed>() {
                let credentials = failed
                    .credentials(context)
                    .expect("borrowed original credentials");
                assert_eq!(credentials.password(), "wrong");
                assert_eq!(failed.guard, "web");
                assert!(failed.user.is_none());
                let unrelated = openrust::auth::events::Failed {
                    guard: "web".into(),
                    user: None,
                };
                assert!(unrelated.credentials(context).is_none());
                self.0.lock().unwrap().push("failed");
            } else if let Some(lockout) = event.downcast_ref::<openrust::auth::events::Lockout>() {
                let request = lockout
                    .request(context)
                    .expect("borrowed original request metadata");
                assert_eq!(request.uri.path(), "/login");
                assert!(lockout.retry_after > 0);
                self.0.lock().unwrap().push("lockout");
            }
            Ok(None)
        })
    }
}
#[tokio::test]
async fn callback_failed_and_lockout_events_borrow_original_payloads() {
    use openrust::contracts::events::Dispatcher as _;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let server = start(json!({"features":[],"views":false,"guard":""}), |app| {
        app.container()
            .make::<Fortify>()
            .unwrap()
            .authenticateUsing(Arc::new(|_| Box::pin(async { Ok(None) })));
        let dispatcher = app
            .container()
            .make::<openrust::events::Dispatcher>()
            .unwrap();
        for name in ["Failed", "Lockout"] {
            dispatcher
                .listen(
                    name,
                    Arc::new(ObserveAuthentication(observed.clone())),
                    openrust::events::Delivery::Sync,
                )
                .unwrap();
        }
    })
    .await;
    let mut client = Client::new(&server);
    for _ in 0..5 {
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"wrong"}"#,
                422,
            )
            .await;
    }
    client.send("POST", "/login", LOGIN, 429).await;
    assert_eq!(
        *observed.lock().unwrap(),
        vec!["failed", "failed", "failed", "failed", "failed", "lockout"]
    );
}

#[tokio::test]
async fn configured_pipeline_preserves_order_and_rejects_unknown_class_before_authentication() {
    let server=start(json!({"features":[],"views":false,"pipelines":{"login":[null,false,"0",0,"OpenRust\\Fortify\\Actions\\CanonicalizeUsername","openrust_fortify::actions::AttemptToAuthenticate","PrepareAuthenticatedSession"]}}),|_|{}).await;
    let mut client = Client::new(&server);
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"A@EXAMPLE.TEST","password":"good:secret"}"#,
            200,
        )
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["user"],
        "001"
    );
    let invalid=start(json!({"features":[],"views":false,"pipelines":{"login":["AttemptToAuthenticate","Other\\AttemptToAuthenticate"]}}),|_|{}).await;
    let mut client = Client::new(&invalid);
    client.send("POST", "/login", LOGIN, 500).await;
    assert!(client.send("GET", "/state", "", 200).await.body["user"].is_null());
}

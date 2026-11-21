#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{Auth, AuthError, AuthIdentifier, AuthManager, AuthResult},
    contracts::{
        auth::Authenticatable,
        container::Container as _,
        events::{Dispatcher as _, Event, Listener},
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
    support::facades::Route,
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

use openrust::database::eloquent::{Model, ModelMetadata, ModelState};
use openrust_fortify::{FortifyResult, TwoFactorAuthenticatable, TwoFactorAuthenticationProvider};
#[derive(Clone)]
struct FactorUser(ModelState);

impl Model for FactorUser {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("fortify_challenge_users")
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

impl Authenticatable for FactorUser {
    fn getAuthIdentifierName(&self) -> &str {
        "account_id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        field(self, "account_id").unwrap().into()
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

fn field<'a>(user: &'a FactorUser, name: &str) -> Option<&'a str> {
    match user.getAttribute(name) {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}

impl TwoFactorAuthenticatable for FactorUser {}
const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
struct Engine {
    engine: TwoFactorAuthenticationProvider,
    generated: Mutex<Vec<usize>>,
}
impl openrust_fortify::contracts::TwoFactorAuthenticationProvider for Engine {
    fn generateSecretKey(&self, length: usize) -> FortifyResult<String> {
        self.generated.lock().unwrap().push(length);
        Ok(SECRET.into())
    }
    fn qrCodeUrl(&self, company: &str, account: &str, secret: &str) -> String {
        self.engine.qrCodeUrl(company, account, secret)
    }
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> HttpFuture<'a, FortifyResult<bool>> {
        self.engine.verify(secret, code)
    }
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn current(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::guard(request, None)?.id().await?;
        let session = request.session()?;
        Response::json(
            StatusCode::OK,
            json!({"user":user,"id":session.getId(),"csrf":session.token(),"challenge":session.get("login.id"),"remember":session.get("login.remember"),"status":session.get("status"),"errors":session.get("errors")}),
        )
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/state", current)?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}
struct Server {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    database: openrust::database::Connection,
    fortify: Arc<Fortify>,
    events: Arc<Mutex<Vec<String>>>,
    encrypter: Arc<dyn openrust::contracts::encryption::Encrypter>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort()
    }
}

struct Settings(Json, Json);
impl openrust::support::ServiceProvider for Settings {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            let config = app
                .container()
                .get::<dyn openrust::contracts::config::Repository>("config")?;
            let mut fortify = json!({"features":["two-factor-authentication"],"views":true});
            fortify
                .as_object_mut()
                .unwrap()
                .extend(self.1.as_object().unwrap().clone());
            config.set("fortify", fortify)?;
            config.set("fortify-options.two-factor-authentication", self.0.clone())?;
            Ok(())
        })
    }
}
struct Observe(Arc<Mutex<Vec<String>>>);
impl Listener for Observe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            use openrust_fortify::events::*;
            macro_rules! check {($($kind:ty),+)=>{$(if let Some(event)=event.downcast_ref::<$kind>() {let user=event.user::<FactorUser>(context).expect("borrowed concrete Eloquent account");assert_eq!(user.getKey(),Some(&Value::Text("001".into())));assert_eq!(user.getAuthIdentifier(),AuthIdentifier::from("login-001"));})+}}
            check!(
                TwoFactorAuthenticationChallenged,
                TwoFactorAuthenticationFailed,
                ValidTwoFactorAuthenticationCodeProvided,
                TwoFactorAuthenticationEnabled,
                TwoFactorAuthenticationConfirmed,
                TwoFactorAuthenticationDisabled,
                RecoveryCodesGenerated,
                RecoveryCodeReplaced
            );
            let name = event.name();
            if name.starts_with("OpenRust\\Fortify\\Events\\")
                || ["Attempting", "Login", "Failed", "Lockout"].contains(&name)
            {
                if let Some(failed) = event.downcast_ref::<openrust::auth::events::Failed>() {
                    assert!(failed.credentials(context).is_some());
                }
                self.0.lock().unwrap().push(name.into());
            }
            Ok(None)
        })
    }
}
async fn start(options: Json, settings: Json) -> Server {
    start_configured(options, settings, |_| {}).await
}
async fn start_configured(
    options: Json,
    settings: Json,
    configure: impl FnOnce(&Application),
) -> Server {
    let mut app = Application::configure(std::env::temp_dir()).withConfig(|_,_| {
        Ok([
            ("app".into(),json!({"key":"a".repeat(32),"timezone":"UTC","url":"http://localhost","name":"OpenRust Test"})),
            ("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}})),
            ("session".into(),json!({"driver":"database","table":"fortify_challenge_sessions","cookie":"fortify_session","lottery":[0,100]})),
            ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
            ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
            ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"FactorUser"}},"password_timeout":900})),
        ].into())
    }).withProviders(vec![Box::new(Settings(options,settings)),Box::new(FortifyServiceProvider)]).withRouting(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    let db = app
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let version = db
        .selectOne("SELECT VERSION() AS version", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(version.get("version"),Some(Value::Text(v)) if v.starts_with("8.4.")));
    for sql in [
        "DROP TABLE IF EXISTS fortify_challenge_users",
        "DROP TABLE IF EXISTS fortify_challenge_sessions",
        "CREATE TABLE fortify_challenge_users(identity VARCHAR(20) PRIMARY KEY,account_id VARCHAR(30) UNIQUE,email VARCHAR(100),password VARCHAR(255),remember_token VARCHAR(100),two_factor_secret TEXT NULL,two_factor_recovery_codes TEXT NULL,two_factor_confirmed_at DATETIME NULL)",
        "CREATE TABLE fortify_challenge_sessions(id VARCHAR(255) PRIMARY KEY,user_id VARCHAR(255) NULL,ip_address VARCHAR(45) NULL,user_agent TEXT NULL,payload LONGTEXT NOT NULL,last_activity BIGINT NOT NULL)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let hash = app
        .make::<dyn Hasher>()
        .await
        .unwrap()
        .make(b"good:secret", &HashOptions::new())
        .await
        .unwrap();
    db.statement("INSERT INTO fortify_challenge_users(identity,account_id,email,password) VALUES ('001','login-001','a@example.test',?)",&[hash.into()]).await.unwrap();
    app.make::<AuthManager>()
        .await
        .unwrap()
        .registerModel::<FactorUser>("FactorUser");
    let fortify = app.make::<Fortify>().await.unwrap();
    fortify.registerTwoFactorAuthenticatable::<FactorUser>();
    fortify.loginView("login");
    fortify.twoFactorChallengeView(
        openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"challenge_view":true})) })
        })),
    );
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    fortify.generateRecoveryCodesUsing(Some(Arc::new(move || {
        format!(
            "recovery-{}",
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    })));
    app.container()
        .instance::<dyn openrust_fortify::contracts::TwoFactorAuthenticationProvider>(Arc::new(
            Engine {
                engine: TwoFactorAuthenticationProvider::new(None)
                    .withClock(|| Ok(59))
                    .withWindow(0),
                generated: Mutex::new(Vec::new()),
            },
        ))
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    app.make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            "*",
            Arc::new(Observe(events.clone())),
            openrust::events::Delivery::Sync,
        )
        .unwrap();
    let encrypter = app
        .make::<dyn openrust::contracts::encryption::Encrypter>()
        .await
        .unwrap();
    configure(&app);
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    Server {
        address,
        task: tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
        database: db,
        fortify,
        events,
        encrypter,
    }
}
fn mysql() -> bool {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return false;
    }
    true
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
async fn official_two_factor_routes_manage_encrypted_account_and_complete_native_session_challenges()
 {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(json!({"confirm":true,"confirmPassword":true}), json!({})).await;
    let mut client = Client::new(&server);
    let before = client.send("GET", "/state", "", 200).await.body;
    client.send("GET", "/two-factor-challenge", "", 302).await;
    client
        .send("POST", "/two-factor-challenge", r#"{"code":"287082"}"#, 422)
        .await;
    assert_eq!(
        client.send("POST", "/login", LOGIN, 200).await.body,
        json!({"two_factor":false})
    );
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["user"],
        "login-001"
    );
    client
        .send("POST", "/user/two-factor-authentication", "{}", 423)
        .await;
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
            .send("GET", "/user/two-factor-qr-code", "", 200)
            .await
            .body,
        json!([])
    );
    client
        .send("GET", "/user/two-factor-secret-key", "", 404)
        .await;
    assert_eq!(
        client
            .send("GET", "/user/two-factor-recovery-codes", "", 200)
            .await
            .body,
        json!([])
    );
    assert_eq!(
        client
            .send("POST", "/user/two-factor-authentication", "{}", 200)
            .await
            .body,
        ""
    );
    assert_eq!(
        client
            .send("GET", "/user/two-factor-secret-key", "", 200)
            .await
            .body,
        json!({"secretKey":SECRET})
    );
    let qr = client
        .send("GET", "/user/two-factor-qr-code", "", 200)
        .await
        .body;
    assert!(qr["url"].as_str().unwrap().contains(SECRET));
    let svg = qr["svg"].as_str().unwrap();
    assert!(svg.contains("width=\"192\" height=\"192\""));
    assert!(!svg.contains(SECRET));
    if let Some(dir) = std::env::var_os("OPENRUST_TEST_ARTIFACT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("challenge.svg"), svg).unwrap();
        std::fs::write(dir.join("challenge-uri.txt"), qr["url"].as_str().unwrap()).unwrap();
    }
    let codes = client
        .send("GET", "/user/two-factor-recovery-codes", "", 200)
        .await
        .body;
    assert_eq!(codes.as_array().unwrap().len(), 8);
    client
        .send("POST", "/user/two-factor-authentication", "{}", 200)
        .await;
    assert_eq!(
        client
            .send("GET", "/user/two-factor-recovery-codes", "", 200)
            .await
            .body,
        codes
    );
    client
        .send(
            "POST",
            "/user/confirmed-two-factor-authentication",
            r#"{"code":"invalid"}"#,
            422,
        )
        .await;
    client.send("POST", "/logout", "{}", 204).await;
    // Unconfirmed account continues through the ordinary native password pipeline.
    assert_eq!(
        client.send("POST", "/login", LOGIN, 200).await.body,
        json!({"two_factor":false})
    );
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"good:secret"}"#,
            201,
        )
        .await;
    client
        .send(
            "POST",
            "/user/confirmed-two-factor-authentication",
            r#"{"code":"287082"}"#,
            200,
        )
        .await;
    client.send("POST", "/logout", "{}", 204).await;
    server.events.lock().unwrap().clear();
    assert_eq!(
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"a@example.test","password":"good:secret","remember":true}"#,
                200
            )
            .await
            .body,
        json!({"two_factor":true})
    );
    assert_eq!(
        *server.events.lock().unwrap(),
        vec!["OpenRust\\Fortify\\Events\\TwoFactorAuthenticationChallenged"]
    );
    let pending = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(pending["challenge"], "001");
    assert_eq!(pending["remember"], true);
    assert!(pending["user"].is_null());
    assert_eq!(
        client
            .send("GET", "/two-factor-challenge", "", 200)
            .await
            .body,
        json!({"challenge_view":true})
    );
    client
        .send("POST", "/two-factor-challenge", r#"{"code":123}"#, 422)
        .await;
    client
        .send("POST", "/two-factor-challenge", r#"{"code":"0"}"#, 422)
        .await;
    let invalid = client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"bad","code":"invalid"}"#,
            422,
        )
        .await
        .body;
    assert!(invalid["errors"]["recovery_code"].is_array());
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["challenge"],
        "001"
    );
    client
        .send("POST", "/two-factor-challenge", r#"{"code":"287082"}"#, 204)
        .await;
    let logged = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(logged["user"], "login-001");
    assert!(logged["challenge"].is_null());
    assert!(logged["remember"].is_null());
    assert_ne!(logged["id"], pending["id"]);
    assert_ne!(logged["csrf"], before["csrf"]);
    assert!(client.cookies.contains_key("remember_web"));
    // The remembered session restores through the same native Eloquent provider.
    let mut remembered = Client::new(&server);
    remembered.cookies.insert(
        "remember_web".into(),
        client.cookies["remember_web"].clone(),
    );
    assert_eq!(
        remembered.send("GET", "/state", "", 200).await.body["user"],
        "login-001"
    );
    client.send("POST", "/logout", "{}", 204).await;
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"recovery-0","code":"invalid"}"#,
            204,
        )
        .await;
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"good:secret"}"#,
            201,
        )
        .await;
    let replaced = client
        .send("GET", "/user/two-factor-recovery-codes", "", 200)
        .await
        .body;
    assert_eq!(replaced[0], "recovery-8");
    client.send("POST", "/logout", "{}", 204).await;
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"recovery-0"}"#,
            422,
        )
        .await;
    client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"recovery-1"}"#,
            204,
        )
        .await;
    client
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"good:secret"}"#,
            201,
        )
        .await;
    client
        .send("POST", "/user/two-factor-recovery-codes", "{}", 200)
        .await;
    let regenerated = client
        .send("GET", "/user/two-factor-recovery-codes", "", 200)
        .await
        .body;
    assert_ne!(regenerated, codes);
    client.html = true;
    let disabled = client
        .send("DELETE", "/user/two-factor-authentication", "{}", 302)
        .await;
    assert!(disabled.headers.contains("location:"));
    client.html = false;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["status"],
        "two-factor-authentication-disabled"
    );
    client
        .send("DELETE", "/user/two-factor-authentication", "{}", 200)
        .await;
    assert_eq!(
        client
            .send("GET", "/user/two-factor-qr-code", "", 200)
            .await
            .body,
        json!([])
    );
    client.send("POST", "/logout", "{}", 204).await;
    assert_eq!(
        client.send("POST", "/login", LOGIN, 200).await.body,
        json!({"two_factor":false})
    );
    server
        .database
        .statement("DROP TABLE fortify_challenge_sessions", &[])
        .await
        .unwrap();
    server
        .database
        .statement("DROP TABLE fortify_challenge_users", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn callback_pipeline_prefix_paths_and_unconfirmed_optional_factor_follow_upstream() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server=start(json!({"confirm":false,"confirmPassword":false}),json!({"prefix":"security","paths":{"two-factor":{"login":"/challenge","enable":"/enable"}},"home":"/account"})).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    server.fortify.authenticateUsing(Arc::new(move |request| {
        let calls = observed.clone();
        Box::pin(async move {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let password = request
                .input(Some("password"))
                .await
                .map_err(|e| AuthError::Http(Box::new(e)))?;
            if password != Some(json!("good:secret")) {
                return Ok(None);
            }
            Ok(FactorUser::find("001")
                .await
                .map_err(|e| AuthError::Backend(Box::new(e)))?
                .map(|user| Box::new(user) as Box<dyn Authenticatable>))
        })
    }));
    let mut client = Client::new(&server);
    client.send("POST", "/login", LOGIN, 404).await;
    client
        .send(
            "POST",
            "/security/login",
            r#"{"email":"a@example.test","password":"wrong"}"#,
            422,
        )
        .await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    client.send("POST", "/security/login", LOGIN, 200).await;
    // Fortify calls the callback in both stages when no challenge is needed.
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3);
    client.send("POST", "/security/enable", "{}", 200).await;
    client.send("POST", "/security/logout", "{}", 204).await;
    client.html = true;
    let redirect = client.send("POST", "/security/login", LOGIN, 302).await;
    assert!(redirect.headers.contains("/security/challenge"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 4);
    client.send("GET", "/security/challenge", "", 200).await;
    let failure = client
        .send(
            "POST",
            "/security/challenge",
            r#"{"recovery_code":"bad"}"#,
            302,
        )
        .await;
    assert!(failure.headers.contains("/security/challenge"));
    client.html = false;
    let state = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(state["challenge"], "001");
    assert!(state["errors"].is_object());
    client.html = true;
    let success = client
        .send("POST", "/security/challenge", r#"{"code":"287082"}"#, 302)
        .await;
    assert!(success.headers.contains("/account"));
    client.html = false;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["user"],
        "login-001"
    );
    client.send("POST", "/security/logout", "{}", 204).await;
    for _ in 0..5 {
        client
            .send(
                "POST",
                "/security/login",
                r#"{"email":"a@example.test","password":"wrong"}"#,
                422,
            )
            .await;
    }
    client.send("POST", "/security/login", LOGIN, 429).await;
    server
        .database
        .statement("DROP TABLE fortify_challenge_sessions", &[])
        .await
        .unwrap();
    server
        .database
        .statement("DROP TABLE fortify_challenge_users", &[])
        .await
        .unwrap();
}

struct CustomTwoFactorLogin;
impl openrust_fortify::contracts::TwoFactorLoginResponse for CustomTwoFactorLogin {}
impl openrust_fortify::contracts::Response for CustomTwoFactorLogin {
    fn toResponse<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { Response::json(StatusCode::ACCEPTED, json!({"custom_login":true})) })
    }
}
#[tokio::test]
async fn challenge_limiters_csrf_container_responses_and_provider_errors_use_native_middleware() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start_configured(
        json!({}),
        json!({"views":false,"limiters":{"two-factor":"challenge"}}),
        |app| {
            app.container()
                .make::<openrust::cache::RateLimiter>()
                .unwrap()
                .r#for("challenge", |_| {
                    openrust::cache::rate_limiting::Limit::perMinute(2)
                });
            app.container()
                .instance::<dyn openrust_fortify::contracts::TwoFactorLoginResponse>(Arc::new(
                    CustomTwoFactorLogin,
                ))
                .unwrap();
        },
    )
    .await;
    let mut client = Client::new(&server);
    client.send("POST", "/login", LOGIN, 200).await;
    client.fetch = false;
    client
        .send("POST", "/user/two-factor-authentication", "{}", 419)
        .await;
    client.fetch = true;
    client
        .send("POST", "/user/two-factor-authentication", "{}", 200)
        .await;
    client.send("GET", "/two-factor-challenge", "", 405).await;
    client.send("POST", "/logout", "{}", 204).await;
    let started = std::time::Instant::now();
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"wrong"}"#,
            422,
        )
        .await;
    assert!(started.elapsed() >= Duration::from_millis(190));
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send("POST", "/two-factor-challenge", r#"{"code":"bad"}"#, 422)
        .await;
    let success = client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"bad","code":"287082"}"#,
            202,
        )
        .await;
    assert_eq!(success.body, json!({"custom_login":true}));
    client.send("POST", "/logout", "{}", 204).await;
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"recovery-0"}"#,
            429,
        )
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["challenge"],
        "001"
    );
    assert!(
        !server
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.ends_with("RecoveryCodeReplaced"))
    );
    server
        .database
        .statement("DROP TABLE fortify_challenge_sessions", &[])
        .await
        .unwrap();
    server
        .database
        .statement("DROP TABLE fortify_challenge_users", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn broken_encryption_and_missing_accounts_preserve_upstream_challenge_consumption_order() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(json!({}), json!({})).await;
    let mut client = Client::new(&server);
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send("POST", "/user/two-factor-authentication", "{}", 200)
        .await;
    client.send("POST", "/logout", "{}", 204).await;
    server
        .database
        .statement(
            "UPDATE fortify_challenge_users SET two_factor_secret='broken'",
            &[],
        )
        .await
        .unwrap();
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send("POST", "/two-factor-challenge", r#"{"code":"287082"}"#, 500)
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["challenge"],
        "001"
    );
    let events = server.events.lock().unwrap().clone();
    assert!(
        !events
            .iter()
            .any(|v| v.ends_with("TwoFactorAuthenticationFailed"))
    );
    // Recovery code is consumed in the session before replacement saves or emits its event.
    server.database.unprepared("CREATE TRIGGER fortify_recovery_save_failure BEFORE UPDATE ON fortify_challenge_users FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='fixture rejects account save'").await.unwrap();
    client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"recovery-0"}"#,
            500,
        )
        .await;
    assert!(client.send("GET", "/state", "", 200).await.body["challenge"].is_null());
    assert!(
        !server
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.ends_with("RecoveryCodeReplaced"))
    );
    server
        .database
        .unprepared("DROP TRIGGER fortify_recovery_save_failure")
        .await
        .unwrap();
    client.send("POST", "/login", LOGIN, 200).await;
    server
        .fortify
        .generateRecoveryCodesUsing(Some(Arc::new(|| "replacement".into())));
    server
        .database
        .statement(
            "ALTER TABLE fortify_challenge_users DROP COLUMN two_factor_recovery_codes",
            &[],
        )
        .await
        .unwrap();
    client
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"recovery_code":"recovery-0"}"#,
            500,
        )
        .await;
    // Decryption/attribute failure happens before session consumption.
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["challenge"],
        "001"
    );
    server
        .database
        .statement("DELETE FROM fortify_challenge_users", &[])
        .await
        .unwrap();
    client.send("GET", "/two-factor-challenge", "", 302).await;
    client
        .send("POST", "/two-factor-challenge", r#"{"code":"287082"}"#, 422)
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["challenge"],
        "001"
    );
    server
        .database
        .statement("DROP TABLE fortify_challenge_sessions", &[])
        .await
        .unwrap();
    server
        .database
        .statement("DROP TABLE fortify_challenge_users", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn fixed_php_challenge_request_controller_cases_match_mysql_http_sessions_and_events() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(json!({}), json!({})).await;
    let row = server
        .database
        .selectOne("SELECT password FROM fortify_challenge_users", &[])
        .await
        .unwrap()
        .unwrap();
    let password = row.get("password").unwrap().clone();
    let secret = server.encrypter.encrypt(SECRET.as_bytes(), true).unwrap();
    let codes = server
        .encrypter
        .encrypt(br#"["recovery-0","0"]"#, true)
        .unwrap();
    let cases: Vec<Json> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/two-factor-challenge.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 20);
    for case in cases {
        server
            .database
            .statement("DELETE FROM fortify_challenge_users", &[])
            .await
            .unwrap();
        server.database.statement("INSERT INTO fortify_challenge_users (identity,account_id,email,password,two_factor_secret,two_factor_recovery_codes) VALUES ('001','login-001','a@example.test',?,?,?)",&[password.clone(),secret.clone().into(),codes.clone().into()]).await.unwrap();
        let mut client = Client::new(&server);
        assert_eq!(
            client
                .send(
                    "POST",
                    "/login",
                    r#"{"email":"a@example.test","password":"good:secret","remember":true}"#,
                    200
                )
                .await
                .body,
            json!({"two_factor":true})
        );
        if case["exists"] == false {
            server
                .database
                .statement("DELETE FROM fortify_challenge_users", &[])
                .await
                .unwrap();
        }
        server.events.lock().unwrap().clear();
        let response = client
            .send(
                "POST",
                "/two-factor-challenge",
                &case["input"].to_string(),
                u16::try_from(case["status"].as_u64().unwrap()).unwrap(),
            )
            .await;
        if let Some(field) = case["field"].as_str() {
            assert!(response.body["errors"][field].is_array(), "{case:?}");
        }
        let state = client.send("GET", "/state", "", 200).await.body;
        assert_eq!(state["challenge"], case["challenge"], "{case:?}");
        assert_eq!(state["remember"], case["remember"], "{case:?}");
        let events: Vec<_> = server
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.rsplit('\\').next().unwrap().to_owned())
            .collect();
        assert_eq!(json!(events), case["events"], "{case:?}");
    }
    server
        .database
        .statement("DROP TABLE fortify_challenge_sessions", &[])
        .await
        .unwrap();
    server
        .database
        .statement("DROP TABLE fortify_challenge_users", &[])
        .await
        .unwrap();
}

struct CustomChallengeStage;
impl openrust_fortify::contracts::RedirectsIfTwoFactorAuthenticatable for CustomChallengeStage {
    fn handle<'a>(
        &'a self,
        _: &'a mut Request,
    ) -> HttpFuture<'a, Result<Option<Response>, HttpError>> {
        Box::pin(async {
            Ok(Some(Response::json(
                StatusCode::ACCEPTED,
                json!({"custom_challenge":true}),
            )?))
        })
    }
}
#[tokio::test]
async fn challenge_stage_contract_can_replace_the_default_pipeline_through_the_container() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start_configured(json!({}), json!({"views":false}), |app| {
        app.container()
            .instance::<dyn openrust_fortify::contracts::RedirectsIfTwoFactorAuthenticatable>(
                Arc::new(CustomChallengeStage),
            )
            .unwrap();
    })
    .await;
    let mut client = Client::new(&server);
    assert_eq!(
        client.send("POST", "/login", LOGIN, 202).await.body,
        json!({"custom_challenge":true})
    );
    let state = client.send("GET", "/state", "", 200).await.body;
    assert!(state["user"].is_null());
    assert!(state["challenge"].is_null());
    assert!(server.events.lock().unwrap().is_empty());
    server
        .database
        .statement("DROP TABLE fortify_challenge_sessions", &[])
        .await
        .unwrap();
    server
        .database
        .statement("DROP TABLE fortify_challenge_users", &[])
        .await
        .unwrap();
}

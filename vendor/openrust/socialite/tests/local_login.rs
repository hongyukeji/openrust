#[path = "support/local_login.rs"]
mod recipe;
use axum::{
    Router,
    extract::State,
    handler::Handler as _,
    response::IntoResponse,
    routing::{get, post},
};
use openrust::{
    auth::{Auth, AuthError, AuthIdentifier, AuthManager, AuthResult},
    contracts::{
        auth::Authenticatable, container::Container as _, http::Kernel as _, session::Session as _,
    },
    database::{
        Connection, DatabaseManager, Value as DbValue,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
    support::facades::Route,
};
use openrust_socialite::{SocialiteManager, SocialiteServiceProvider, two::GithubProvider};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Clone)]
struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("users")
            .primaryKey("id")
            .incrementing(false)
            .timestamps(None, None);
        &META
    }
    fn from_state(s: ModelState) -> Self {
        Self(s)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
fn text<'a>(u: &'a User, key: &str) -> Option<&'a str> {
    match u.getAttribute(key) {
        Some(DbValue::Text(v)) => Some(v),
        _ => None,
    }
}
impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::try_from(self.getKey().unwrap()).unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        text(self, "password")
    }
    fn get_remember_token(&self) -> Option<&str> {
        text(self, "remember_token")
    }
    fn set_remember_token(&mut self, v: Option<String>) {
        self.setAttribute("remember_token", v.map_or(DbValue::Null, DbValue::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn getAuthAttribute(&self, key: &str) -> AuthResult<Option<DbValue>> {
        self.getAttributeValue(key)
            .map_err(|e| AuthError::Backend(Box::new(e)))
    }
}
fn internal(e: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(e),
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/login/github", login_github)?;
    Route::get("/login/work", login_work)?;
    Route::get("/callback/github", callback_github)?;
    Route::get("/callback/work", callback_work)?;
    Route::get("/who", who)?;
    Route::r#match(&[http::Method::POST], "/logout", logout)?;
    Ok(())
}
fn login_github(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    login(r, "github")
}
fn login_work(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    login(r, "work")
}
fn login<'a>(
    r: &'a mut Request,
    driver: &'static str,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        r.session_mut()?.put("socialite.provider", driver.into())?;
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>()
            .map_err(internal)?;
        Ok(manager
            .driver(driver, r)
            .map_err(internal)?
            .redirect()
            .await
            .map_err(internal)?
            .into())
    })
}
fn callback_github(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    callback(r, "github")
}
fn callback_work(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    callback(r, "work")
}
fn callback<'a>(
    r: &'a mut Request,
    driver: &'static str,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        if r.session_mut()?
            .pull("socialite.provider")
            .as_ref()
            .and_then(Value::as_str)
            != Some(driver)
        {
            return Response::json(
                StatusCode::BAD_REQUEST,
                json!({"message":"Invalid provider attempt"}),
            );
        }
        let manager = openrust::container::Container::getInstance()
            .make::<SocialiteManager>()
            .map_err(internal)?;
        let remote = match manager.driver(driver, r).map_err(internal)?.user().await {
            Ok(user) => user,
            Err(_) => {
                return Response::json(
                    StatusCode::BAD_REQUEST,
                    json!({"message":"Invalid provider response"}),
                );
            }
        };
        if !recipe::login_bound_user(r, driver, &remote).await? {
            return Response::json(
                StatusCode::FORBIDDEN,
                json!({"message":"No local account binding"}),
            );
        }
        Response::json(StatusCode::OK, json!({"authenticated":true}))
    })
}
fn who(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(r).await.map_err(internal)?;
        let session = r.session()?;
        Response::json(
            StatusCode::OK,
            json!({"user":user.as_ref().map(|u|u.get_auth_identifier().to_string()),"session":session.getId(),"csrf":session.token()}),
        )
    })
}
fn logout(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Auth::sessionGuard(r, Some("web"))
            .map_err(internal)?
            .logoutAndInvalidate()
            .await
            .map_err(internal)?;
        Ok(Response::noContent())
    })
}
#[derive(Default)]
struct Remote {
    calls: AtomicUsize,
}
async fn token(
    State(r): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    r.calls.fetch_add(1, Ordering::Relaxed);
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let fields = url::form_urlencoded::parse(&body)
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(fields["redirect_uri"], "https://rp.test/callback");
    axum::Json(json!({"access_token":fields["code"],"scope":"user:email"})).into_response()
}
async fn profile(
    State(r): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    r.calls.fetch_add(1, Ordering::Relaxed);
    let code = request.headers()["authorization"]
        .to_str()
        .unwrap()
        .strip_prefix("token ")
        .unwrap();
    if code == "revoked" {
        return (
            StatusCode::UNAUTHORIZED,
            "private upstream credential error",
        )
            .into_response();
    }
    let id = match code {
        "number" => json!(7),
        "boolean" => json!(true),
        "array" => json!([]),
        "blank" => json!(""),
        _ => json!(code),
    };
    axum::Json(json!({"id":id,"login":"alice","name":"Remote Alice","email":"shared@test","avatar_url":"https://image.test/alice"})).into_response()
}
async fn emails() -> axum::Json<Value> {
    axum::Json(json!([{"email":"shared@test","primary":true,"verified":true}]))
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Remote::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/token", post(token))
        .route("/user", get(profile))
        .route("/emails", get(emails))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, state, task)
}
struct App {
    base: String,
    database: Connection,
    task: tokio::task::JoinHandle<()>,
}
async fn app(remote: &str, url: &str, letter: &str) -> App {
    let settings:BTreeMap<String,Value>=[
        ("app".into(),json!({"key":letter.repeat(32),"env":"production","debug":false})),
        ("services".into(),json!({"github":{"client_id":"github-client","client_secret":"github-secret","redirect":"https://rp.test/callback"},"work":{"client_id":"work-client","client_secret":"work-secret","redirect":"https://rp.test/callback"}})),
        ("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":url}}})),
        ("session".into(),json!({"driver":"database","table":"sessions","cookie":"local_session","lottery":[0,100]})),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"SocialiteLocalUser"}}})),
    ].into();
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| Ok(Default::default()))
        .withProviders(vec![
            Box::new(Settings(settings)),
            Box::new(SocialiteServiceProvider),
        ])
        .withRouting(Routing::default().web(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.make::<AuthManager>()
        .await
        .unwrap()
        .registerModel::<User>("SocialiteLocalUser");
    let manager = app.make::<SocialiteManager>().await.unwrap();
    for provider in ["github", "work"] {
        manager.extend(
            provider,
            Arc::new(GithubProvider::withEndpoints(
                format!("{remote}/authorize"),
                format!("{remote}/token"),
                format!("{remote}/user"),
                format!("{remote}/emails"),
            )),
        );
    }
    let database = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let version = database
        .selectOne("SELECT VERSION() AS version", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(version.get("version"),Some(DbValue::Text(v)) if v.starts_with("8.4.")));
    for sql in [
        "CREATE TABLE users(id BIGINT UNSIGNED PRIMARY KEY,email VARCHAR(255) UNIQUE,password VARCHAR(255),remember_token VARCHAR(100) NULL)",
        "CREATE TABLE social_identities(provider VARBINARY(128) NOT NULL,provider_user_id VARBINARY(255) NOT NULL,user_id BIGINT UNSIGNED NOT NULL,UNIQUE(provider,provider_user_id),FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE)",
        "CREATE TABLE sessions(id VARCHAR(255) PRIMARY KEY,user_id VARCHAR(255) NULL,ip_address VARCHAR(45) NULL,user_agent TEXT NULL,payload LONGTEXT NOT NULL,last_activity BIGINT NOT NULL)",
        "INSERT INTO users(id,email,password) VALUES(101,'shared@test','unused'),(102,'other@test','unused')",
        "INSERT INTO social_identities(provider,provider_user_id,user_id) VALUES('github','007',101),('work','007',102),('github','ABC',101),('github','abc',102),('github','7',102)",
    ] {
        database.statement(sql, &[]).await.unwrap();
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        State(k): State<Arc<Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handleTransport(r).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    App {
        base,
        database,
        task,
    }
}
struct Client {
    http: Factory,
    base: String,
    cookie: Option<String>,
}
impl Client {
    fn new(app: &App) -> Self {
        Self {
            http: Factory::new().unwrap(),
            base: app.base.clone(),
            cookie: None,
        }
    }
    async fn get(&mut self, path: &str) -> openrust::http::client::Response {
        let mut r = self.http.pendingRequest().withoutRedirecting();
        if let Some(cookie) = &self.cookie {
            r = r.withHeaders(http::HeaderMap::from_iter([(
                http::header::COOKIE,
                http::HeaderValue::from_str(cookie).unwrap(),
            )]));
        }
        let response = r.get(&format!("{}{path}", self.base)).await.unwrap();
        self.cookies(&response);
        response
    }
    fn cookies(&mut self, r: &openrust::http::client::Response) {
        for header in r.headers().get_all("set-cookie") {
            let cookie = header.to_str().unwrap();
            if cookie.starts_with("local_session=") {
                self.cookie = Some(cookie.split(';').next().unwrap().into());
            }
        }
    }
    async fn who(&mut self) -> Value {
        let r = self.get("/who").await;
        assert_eq!(r.status(), StatusCode::OK);
        r.json().unwrap()
    }
    async fn begin(&mut self, provider: &str) -> String {
        let response = self.get(&format!("/login/{provider}")).await;
        assert_eq!(response.status(), StatusCode::FOUND);
        url::Url::parse(response.headers()["location"].to_str().unwrap())
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned()
    }
    async fn finish(
        &mut self,
        provider: &str,
        state: &str,
        code: &str,
    ) -> openrust::http::client::Response {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", state)
            .append_pair("code", code)
            .finish();
        self.get(&format!("/callback/{provider}?{query}")).await
    }
    async fn logout(&mut self, csrf: &str) -> openrust::http::client::Response {
        let response = self
            .http
            .pendingRequest()
            .withoutRedirecting()
            .withHeaders(http::HeaderMap::from_iter([
                (
                    http::header::COOKIE,
                    http::HeaderValue::from_str(self.cookie.as_deref().unwrap()).unwrap(),
                ),
                (
                    http::header::HeaderName::from_static("x-csrf-token"),
                    http::HeaderValue::from_str(csrf).unwrap(),
                ),
            ]))
            .asForm()
            .post(&format!("{}/logout", self.base), &json!({}))
            .await
            .unwrap();
        self.cookies(&response);
        response
    }
}
#[tokio::test]
async fn native_mysql_bound_identity_login_restores_rotates_isolates_and_rejects_unbound_or_failed_callbacks()
 {
    let Ok(mysql) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL required"
        );
        return;
    };
    let mut admin = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| Ok([("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}}))].into()))
        .create()
        .unwrap();
    admin.boot().await.unwrap();
    let connection = admin
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    for schema in ["socialite_app_a", "socialite_app_b"] {
        connection
            .statement(&format!("CREATE DATABASE {schema}"), &[])
            .await
            .unwrap();
    }
    let (remote, r, remote_task) = remote().await;
    let mut one_url = url::Url::parse(&mysql).unwrap();
    one_url.set_path("/socialite_app_a");
    let mut two_url = one_url.clone();
    two_url.set_path("/socialite_app_b");
    let one = app(&remote, one_url.as_str(), "a").await;
    let two = app(&remote, two_url.as_str(), "b").await;
    let mut alice = Client::new(&one);
    let before = alice.who().await;
    assert!(before["user"].is_null());
    let before_cookie = alice.cookie.clone();
    let state = alice.begin("github").await;
    assert_eq!(
        alice.finish("github", &state, "007").await.status(),
        StatusCode::OK
    );
    let after = alice.who().await;
    assert_eq!(after["user"], "101");
    assert_ne!(before["session"], after["session"]);
    assert_ne!(before["csrf"], after["csrf"]);
    let mut fixed = Client::new(&one);
    fixed.cookie = before_cookie;
    assert!(fixed.who().await["user"].is_null());
    let logged_in_cookie = alice.cookie.clone();
    let mut outsider = Client::new(&two);
    outsider.cookie = alice.cookie.clone();
    assert!(outsider.who().await["user"].is_null());
    assert_eq!(alice.logout("invalid-csrf").await.status().as_u16(), 419);
    assert_eq!(alice.who().await["user"], "101");
    assert_eq!(
        alice.logout(after["csrf"].as_str().unwrap()).await.status(),
        StatusCode::NO_CONTENT
    );
    let logged_out = alice.who().await;
    assert!(logged_out["user"].is_null());
    assert_ne!(after["session"], logged_out["session"]);
    assert_ne!(after["csrf"], logged_out["csrf"]);
    let mut replay = Client::new(&one);
    replay.cookie = logged_in_cookie;
    assert!(replay.who().await["user"].is_null());
    for (provider, code, id) in [
        ("work", "007", "102"),
        ("github", "ABC", "101"),
        ("github", "abc", "102"),
        ("github", "number", "102"),
    ] {
        let mut client = Client::new(&one);
        let state = client.begin(provider).await;
        assert_eq!(
            client.finish(provider, &state, code).await.status(),
            StatusCode::OK
        );
        assert_eq!(client.who().await["user"], id);
    }
    for code in ["unknown", "blank", "boolean", "array"] {
        let mut guest = Client::new(&one);
        let state = guest.begin("github").await;
        assert_eq!(
            guest.finish("github", &state, code).await.status(),
            StatusCode::FORBIDDEN
        );
        assert!(guest.who().await["user"].is_null());
    }
    let mut guest = Client::new(&one);
    let state = guest.begin("github").await;
    let before_calls = r.calls.load(Ordering::Relaxed);
    assert_eq!(
        guest.finish("work", &state, "007").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(r.calls.load(Ordering::Relaxed), before_calls);
    let state = guest.begin("github").await;
    assert_eq!(
        guest.finish("github", "wrong", "007").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(r.calls.load(Ordering::Relaxed), before_calls);
    assert_eq!(
        guest.finish("github", &state, "007").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert!(guest.who().await["user"].is_null());
    let state = guest.begin("github").await;
    assert_eq!(
        guest.finish("github", &state, "revoked").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert!(guest.who().await["user"].is_null());
    let mut left = Client::new(&one);
    let mut right = Client::new(&one);
    let ls = left.begin("github").await;
    let rs = right.begin("github").await;
    let (l, r) = tokio::join!(
        left.finish("github", &ls, "007"),
        right.finish("github", &rs, "007")
    );
    assert_eq!(l.status(), StatusCode::OK);
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(left.who().await["user"], "101");
    assert_eq!(right.who().await["user"], "101");
    assert!(one.database.statement("INSERT INTO social_identities(provider,provider_user_id,user_id) VALUES('github','007',102)",&[]).await.is_err());
    let rows = one
        .database
        .selectOne("SELECT COUNT(*) AS count FROM users", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        rows.get("count"),
        Some(DbValue::Integer(2) | DbValue::Unsigned(2))
    ));
    one.database
        .statement("DELETE FROM users WHERE id=102", &[])
        .await
        .unwrap();
    let mut deleted = Client::new(&one);
    let state = deleted.begin("work").await;
    assert_eq!(
        deleted.finish("work", &state, "007").await.status(),
        StatusCode::FORBIDDEN
    );
    one.database
        .statement("DROP TABLE social_identities", &[])
        .await
        .unwrap();
    let mut failed = Client::new(&one);
    let state = failed.begin("github").await;
    let response = failed.finish("github", &state, "007").await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!String::from_utf8_lossy(response.body()).contains("github-secret"));
    assert!(failed.who().await["user"].is_null());
    let mut unaffected = Client::new(&two);
    let state = unaffected.begin("github").await;
    assert_eq!(
        unaffected.finish("github", &state, "007").await.status(),
        StatusCode::OK
    );
    assert_eq!(unaffected.who().await["user"], "101");
    one.task.abort();
    two.task.abort();
    remote_task.abort();
}

struct Settings(BTreeMap<String, Value>);
impl openrust::support::ServiceProvider for Settings {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        use openrust::contracts::foundation::Application as _;
        Box::pin(async move {
            let config = app
                .container()
                .get::<dyn openrust::contracts::config::Repository>("config")?;
            for (key, value) in &self.0 {
                config.set(key, value.clone())?;
            }
            Ok(())
        })
    }
}

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
        validation::Validator as _,
    },
    database::Value,
    foundation::{Application, configuration::Routing, http::Kernel},
    hashing::HashOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::facades::{Route, Validator},
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

use openrust::auth::{AuthFuture, passwords::Password};
use openrust::contracts::auth::{CanResetPassword, MustVerifyEmail};
use openrust::database::eloquent::{Model, ModelMetadata, ModelState};
use openrust::http::RequestValue;
use openrust::validation::{Data, RuleSet, ValidationException};
use openrust_fortify::contracts;
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static NOTIFICATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[derive(Clone)]
struct AccountUser(ModelState);
impl Model for AccountUser {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("fortify_account_users")
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
fn field<'a>(user: &'a AccountUser, name: &str) -> Option<&'a str> {
    match user.getAttribute(name) {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}
impl Authenticatable for AccountUser {
    fn get_auth_identifier_name(&self) -> &str {
        "identity"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        field(self, "identity").unwrap().into()
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
    fn getAuthAttribute(&self, name: &str) -> AuthResult<Option<Value>> {
        self.getAttributeValue(name)
            .map_err(|e| AuthError::Backend(Box::new(e)))
    }
    fn as_password_reset(&self) -> Option<&dyn CanResetPassword> {
        Some(self)
    }
    fn as_password_reset_mut(&mut self) -> Option<&mut dyn CanResetPassword> {
        Some(self)
    }
    fn as_email_verification(&self) -> Option<&dyn MustVerifyEmail> {
        Some(self)
    }
}
impl CanResetPassword for AccountUser {
    fn getEmailForPasswordReset(&self) -> &str {
        field(self, "email").unwrap()
    }
    fn sendPasswordResetNotification<'a>(&'a self, _: &'a str) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
impl MustVerifyEmail for AccountUser {
    fn hasVerifiedEmail(&self) -> bool {
        self.getAttribute("email_verified_at")
            .is_some_and(|v| *v != Value::Null)
    }
    fn markEmailAsVerified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn markEmailAsUnverified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn sendEmailVerificationNotification(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(async {
            NOTIFICATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
    }
    fn getEmailForVerification(&self) -> &str {
        field(self, "email").unwrap()
    }
}
fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}
fn input(input: &RequestValue, name: &str) -> Json {
    match input.get(name) {
        Some(RequestValue::Value(value)) => value.clone(),
        _ => Json::Null,
    }
}
fn validation_data(
    input_value: &RequestValue,
) -> Result<Data, openrust::validation::ValidationError> {
    Data::new(
        json!({"name":input(input_value,"name"),"email":input(input_value,"email"),"password":input(input_value,"password"),"password_confirmation":input(input_value,"password_confirmation"),"current_password":input(input_value,"current_password")}),
    )
}
// Fixture-specific account policy. Fortify delegates these checks to application actions.
fn policy(values: &RequestValue, bag: &str, email: bool, password: bool) -> Result<(), HttpError> {
    let mut errors = openrust::support::MessageBag::default();
    if email
        && !input(values, "email")
            .as_str()
            .is_some_and(|v| v.ends_with("@example.test"))
    {
        errors.add("email", "This fixture accepts addresses at example.test.");
    }
    if password && input(values, "password") != input(values, "password_confirmation") {
        errors.add(
            "password",
            "The password field confirmation does not match.",
        );
    }
    if errors.is_empty() {
        return Ok(());
    }
    let mut error = ValidationException::new(errors);
    error.errorBag(bag);
    Err(HttpError::Validation(Box::new(error)))
}
struct Actions {
    hasher: Arc<dyn Hasher>,
}
impl contracts::CreatesNewUsers for Actions {
    fn create<'a>(
        &'a self,
        values: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn contracts::User>, HttpError>> {
        Box::pin(async move {
            Validator::make(
                validation_data(values)?,
                RuleSet::parse([
                    ("name", "required|string|max:255"),
                    (
                        "email",
                        "required|string|unique:fortify_account_users,email",
                    ),
                    ("password", "required|string|min:8"),
                ])?,
            )?
            .validate()
            .await?;
            policy(values, "default", true, true)?;
            let hash = self
                .hasher
                .make(
                    input(values, "password").as_str().unwrap().as_bytes(),
                    &HashOptions::new(),
                )
                .await
                .map_err(internal)?;
            let mut user = AccountUser::from_state(ModelState::default());
            user.forceFill(
                [
                    ("identity".into(), "001".into()),
                    (
                        "name".into(),
                        input(values, "name").as_str().unwrap().into(),
                    ),
                    (
                        "email".into(),
                        input(values, "email").as_str().unwrap().into(),
                    ),
                    ("password".into(), hash.into()),
                ]
                .into(),
            )
            .map_err(internal)?
            .save()
            .await
            .map_err(internal)?;
            Ok(Box::new(user) as Box<dyn contracts::User>)
        })
    }
}
impl contracts::UpdatesUserProfileInformation for Actions {
    fn update<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        values: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn contracts::User>, HttpError>> {
        Box::pin(async move {
            let mut user = (user as &dyn std::any::Any)
                .downcast_ref::<AccountUser>()
                .unwrap()
                .clone();
            Validator::make(
                validation_data(values)?,
                RuleSet::parse([
                    ("name", "required|string|max:255"),
                    (
                        "email",
                        "required|string|unique:fortify_account_users,email,001,identity",
                    ),
                ])?,
            )?
            .validateWithBag("updateProfileInformation")
            .await?;
            policy(values, "updateProfileInformation", true, false)?;
            let email = input(values, "email");
            let email = email.as_str().unwrap();
            let changed = field(&user, "email") != Some(email);
            user.forceFill(
                [
                    (
                        "name".into(),
                        input(values, "name").as_str().unwrap().into(),
                    ),
                    ("email".into(), email.into()),
                    ("email_verified_at".into(), Value::Null),
                ]
                .into(),
            )
            .map_err(internal)?
            .save()
            .await
            .map_err(internal)?;
            if changed {
                user.sendEmailVerificationNotification().await?;
            }
            Ok(Box::new(user) as Box<dyn contracts::User>)
        })
    }
}
impl contracts::UpdatesUserPasswords for Actions {
    fn update<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        values: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn contracts::User>, HttpError>> {
        Box::pin(async move {
            Validator::make(
                validation_data(values)?,
                RuleSet::parse([
                    ("current_password", "required|string"),
                    ("password", "required|string|min:8"),
                ])?,
            )?
            .validateWithBag("updatePassword")
            .await?;
            policy(values, "updatePassword", false, true)?;
            let password = input(values, "current_password");
            let password = password.as_str().unwrap();
            if !self
                .hasher
                .check(
                    password.as_bytes(),
                    user.get_auth_password().unwrap(),
                    &HashOptions::new(),
                )
                .await
                .map_err(internal)?
            {
                let mut bag = openrust::support::MessageBag::default();
                bag.add(
                    "current_password",
                    "The provided password does not match your current password.",
                );
                let mut error = ValidationException::new(bag);
                error.errorBag("updatePassword");
                return Err(HttpError::Validation(Box::new(error)));
            }
            let mut user = (user as &dyn std::any::Any)
                .downcast_ref::<AccountUser>()
                .unwrap()
                .clone();
            let hash = self
                .hasher
                .make(
                    input(values, "password").as_str().unwrap().as_bytes(),
                    &HashOptions::new(),
                )
                .await
                .map_err(internal)?;
            user.forceFill([("password".into(), hash.into())].into())
                .map_err(internal)?
                .save()
                .await
                .map_err(internal)?;
            Ok(Box::new(user) as Box<dyn contracts::User>)
        })
    }
}
fn current(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request).await?;
        let session = request.session()?;
        let email = user
            .as_ref()
            .map(|u| u.getAuthAttribute("email"))
            .transpose()?
            .flatten();
        let email = match email {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        };
        Response::json(
            StatusCode::OK,
            json!({"user":user.map(|u|u.get_auth_identifier()),"email":email,"id":session.getId(),"csrf":session.token(),"status":session.get("status"),"errors":session.get("errors")}),
        )
    })
}
fn seed_tokens(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request).await?.unwrap();
        let user = user.as_password_reset().unwrap();
        Password::broker(Some("accounts"))?
            .createToken(user)
            .await?;
        Password::broker(Some("users"))?.createToken(user).await?;
        Ok(Response::noContent())
    })
}
fn intended(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        openrust::support::facades::Redirect::setIntendedUrl(request, "/account/settings")?;
        current(request).await
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/state", current)?;
    Route::get("/intended", intended)?;
    Route::group(
        openrust::routing::RouteGroup::new().middleware_name("auth"),
        |router| {
            router.r#match(&[openrust::http::Method::POST], "/seed-tokens", seed_tokens)?;
            Ok(())
        },
    )?;
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
    hasher: Arc<dyn Hasher>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort()
    }
}

struct Settings(Json);
impl openrust::support::ServiceProvider for Settings {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            app.container()
                .get::<dyn openrust::contracts::config::Repository>("config")?
                .set("fortify", self.0.clone())?;
            Ok(())
        })
    }
}
struct Observe(Arc<Mutex<Vec<String>>>);
impl Listener for Observe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            if let Some(registered) = event.downcast_ref::<openrust::auth::events::Registered>() {
                assert!((registered.user.as_ref() as &dyn std::any::Any).is::<AccountUser>());
            }
            if let Some(updated) =
                event.downcast_ref::<openrust_fortify::events::PasswordUpdatedViaController>()
            {
                assert!((updated.user.as_ref() as &dyn std::any::Any).is::<AccountUser>());
            }
            if [
                "Registered",
                "Login",
                "OpenRust\\Fortify\\Events\\PasswordUpdatedViaController",
            ]
            .contains(&event.name())
            {
                self.0.lock().unwrap().push(event.name().into());
            }
            Ok(None)
        })
    }
}
async fn start(settings: Json, callbacks: bool, configure: impl FnOnce(&Application)) -> Server {
    let mut app=Application::configure(std::env::temp_dir()).withConfig(|_,_|Ok([
        ("app".into(),json!({"key":"a".repeat(32),"timezone":"UTC","url":"http://localhost","debug":true})),
        ("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}})),
        ("session".into(),json!({"driver":"database","table":"fortify_account_sessions","cookie":"fortify_session","lottery":[0,100]})),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
        ("auth".into(),json!({"defaults":{"guard":"web","passwords":"users"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"AccountUser"}},"passwords":{"users":{"driver":"database","provider":"users","table":"fortify_default_resets","expire":60},"accounts":{"driver":"database","provider":"users","table":"fortify_named_resets","expire":60}}})),
    ].into())).withProviders(vec![Box::new(Settings(settings)),Box::new(FortifyServiceProvider)]).withRouting(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    let database = app
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let version = database
        .selectOne("SELECT VERSION() AS version", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(version.get("version"),Some(Value::Text(v)) if v.starts_with("8.4.")));
    for sql in [
        "DROP TABLE IF EXISTS fortify_account_users",
        "DROP TABLE IF EXISTS fortify_account_sessions",
        "DROP TABLE IF EXISTS fortify_default_resets",
        "DROP TABLE IF EXISTS fortify_named_resets",
        "CREATE TABLE fortify_account_users(identity VARCHAR(20) PRIMARY KEY,name VARCHAR(255),email VARCHAR(255) UNIQUE,password VARCHAR(255),remember_token VARCHAR(100) NULL,email_verified_at DATETIME NULL)",
        "CREATE TABLE fortify_account_sessions(id VARCHAR(255) PRIMARY KEY,user_id VARCHAR(255) NULL,ip_address VARCHAR(45) NULL,user_agent TEXT NULL,payload LONGTEXT NOT NULL,last_activity BIGINT NOT NULL)",
        "CREATE TABLE fortify_default_resets(email VARCHAR(255) PRIMARY KEY,token VARCHAR(255),created_at DATETIME)",
        "CREATE TABLE fortify_named_resets(email VARCHAR(255) PRIMARY KEY,token VARCHAR(255),created_at DATETIME)",
    ] {
        database.statement(sql, &[]).await.unwrap();
    }
    app.make::<AuthManager>()
        .await
        .unwrap()
        .registerModel::<AccountUser>("AccountUser");
    let fortify = app.make::<Fortify>().await.unwrap();
    let hasher = app.make::<dyn Hasher>().await.unwrap();
    let actions = Arc::new(Actions {
        hasher: hasher.clone(),
    });
    if callbacks {
        fortify.createUsersUsing(actions.clone());
        fortify.updateUserProfileInformationUsing(actions.clone());
        fortify.updateUserPasswordsUsing(actions.clone());
    } else {
        app.container()
            .instance::<dyn contracts::CreatesNewUsers>(actions.clone())
            .unwrap();
        app.container()
            .instance::<dyn contracts::UpdatesUserProfileInformation>(actions.clone())
            .unwrap();
        app.container()
            .instance::<dyn contracts::UpdatesUserPasswords>(actions.clone())
            .unwrap();
    }
    fortify.registerView(
        openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"register_view":true})) })
        })),
    );
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
        fortify,
        events,
        hasher,
    }
}
fn mysql() -> bool {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return false;
    }
    true
}
fn settings() -> Json {
    json!({"features":["registration","update-profile-information","update-passwords"],"passwords":"accounts","lowercase_usernames":true})
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

const REGISTER: &str = r#"{"name":"Tester","email":"A@EXAMPLE.TEST","password":"good:secret","password_confirmation":"good:secret","remember":true}"#;
#[tokio::test]
async fn registration_profile_and_password_updates_use_native_eloquent_sessions_and_named_broker() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    NOTIFICATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    let server = start(settings(), true, |_| {}).await;
    let mut client = Client::new(&server);
    let before = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(
        client.send("GET", "/register", "", 200).await.body,
        json!({"register_view":true})
    );
    client.send("POST", "/register", "{}", 422).await;
    assert!(server.events.lock().unwrap().is_empty());
    let response = client.send("POST", "/register", REGISTER, 201).await;
    assert_eq!(response.body, "");
    assert_eq!(*server.events.lock().unwrap(), ["Registered", "Login"]);
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 1);
    let after = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(after["user"], "001");
    assert_eq!(after["email"], "a@example.test");
    assert_ne!(after["id"], before["id"]);
    assert_ne!(after["csrf"], before["csrf"]);
    assert!(client.cookies.contains_key("remember_web"));
    client.send("POST", "/register", REGISTER, 302).await;
    let mut remembered = Client::new(&server);
    remembered.cookies.insert(
        "remember_web".into(),
        client.cookies["remember_web"].clone(),
    );
    assert_eq!(
        remembered.send("GET", "/state", "", 200).await.body["user"],
        "001"
    );
    client
        .send(
            "PUT",
            "/user/profile-information",
            r#"{"name":"Tester","email":"invalid"}"#,
            422,
        )
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["email"],
        "a@example.test"
    );
    client.html = true;
    client
        .send("PUT", "/user/profile-information", "{}", 302)
        .await;
    client.html = false;
    let errors = client.send("GET", "/state", "", 200).await.body["errors"].clone();
    assert!(errors["updateProfileInformation"].is_object());
    client.html = true;
    client
        .send(
            "PUT",
            "/user/profile-information",
            r#"{"name":"Changed","email":"CHANGED@EXAMPLE.TEST"}"#,
            302,
        )
        .await;
    client.html = false;
    let changed = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(changed["email"], "changed@example.test");
    assert_eq!(changed["status"], "profile-information-updated");
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 2);
    client.send("POST", "/seed-tokens", "{}", 204).await;
    client.send("PUT","/user/password",r#"{"current_password":"wrong","password":"updated:secret","password_confirmation":"updated:secret"}"#,422).await;
    let count = server
        .database
        .selectOne("SELECT COUNT(*) AS count FROM fortify_named_resets", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("count"),
        Some(Value::Integer(1) | Value::Unsigned(1))
    ));
    server.events.lock().unwrap().clear();
    client.send("PUT","/user/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"updated:secret"}"#,200).await;
    assert_eq!(
        *server.events.lock().unwrap(),
        ["OpenRust\\Fortify\\Events\\PasswordUpdatedViaController"]
    );
    let count = server
        .database
        .selectOne("SELECT COUNT(*) AS count FROM fortify_named_resets", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("count"),
        Some(Value::Integer(0) | Value::Unsigned(0))
    ));
    let count = server
        .database
        .selectOne("SELECT COUNT(*) AS count FROM fortify_default_resets", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("count"),
        Some(Value::Integer(1) | Value::Unsigned(1))
    ));
    client.send("POST", "/logout", "{}", 204).await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"changed@example.test","password":"good:secret"}"#,
            422,
        )
        .await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"changed@example.test","password":"updated:secret"}"#,
            200,
        )
        .await;
}

#[tokio::test]
async fn container_account_actions_web_intended_and_broker_failure_preserve_effect_order() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(settings(), false, |_| {}).await;
    server.fortify.registerView(
        openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"replacement_view":true})) })
        })),
    );
    let mut client = Client::new(&server);
    assert_eq!(
        client.send("GET", "/register", "", 200).await.body,
        json!({"replacement_view":true})
    );
    client.send("GET", "/intended", "", 200).await;
    client.html = true;
    let response = client.send("POST", "/register", REGISTER, 302).await;
    assert!(response.headers.contains("/account/settings"));
    client.html = false;
    client.send("POST", "/seed-tokens", "{}", 204).await;
    server
        .database
        .statement("DROP TABLE fortify_named_resets", &[])
        .await
        .unwrap();
    server.events.lock().unwrap().clear();
    client.send("PUT","/user/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"updated:secret"}"#,500).await;
    assert!(server.events.lock().unwrap().is_empty());
    client.send("POST", "/logout", "{}", 204).await;
    client.send("POST", "/login", LOGIN, 422).await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"updated:secret"}"#,
            200,
        )
        .await;
}

struct CustomProfileResponse;
impl contracts::ProfileInformationUpdatedResponse for CustomProfileResponse {}
impl contracts::Response for CustomProfileResponse {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?.unwrap();
            let email = user.getAuthAttribute("email")?.unwrap();
            let Value::Text(email) = email else {
                panic!("native model email")
            };
            Response::json(StatusCode::ACCEPTED, json!({"current_email":email}))
        })
    }
}
#[tokio::test]
async fn configured_account_paths_csrf_and_response_replacements_use_the_updated_guard_user() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let mut options = settings();
    options["views"] = false.into();
    options["prefix"] = "account".into();
    options["paths"] = json!({"register":"/join","user-profile-information":{"update":"/profile"},"user-password":{"update":"/password"}});
    let server = start(options, true, |app| {
        app.container()
            .instance::<dyn contracts::ProfileInformationUpdatedResponse>(Arc::new(
                CustomProfileResponse,
            ))
            .unwrap();
    })
    .await;
    let mut client = Client::new(&server);
    client.fetch = false;
    client.send("POST", "/account/join", REGISTER, 419).await;
    client.fetch = true;
    client.send("GET", "/account/join", "", 405).await;
    client.send("POST", "/register", REGISTER, 404).await;
    client.send("POST", "/account/join", REGISTER, 201).await;
    let updated = client
        .send(
            "PUT",
            "/account/profile",
            r#"{"name":"Changed","email":"CHANGED@EXAMPLE.TEST"}"#,
            202,
        )
        .await;
    assert_eq!(
        updated.body,
        json!({"current_email":"changed@example.test"})
    );
    client.send("PUT","/account/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"mismatch"}"#,422).await;
    client.html = true;
    client.send("PUT","/account/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"updated:secret"}"#,302).await;
    client.html = false;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["status"],
        "password-updated"
    );
}

#[tokio::test]
async fn fixed_php_account_controllers_match_native_mysql_effect_and_event_order() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(settings(), false, |_| {}).await;
    let mut client = Client::new(&server);
    let cases: Vec<Json> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/accounts.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 4);
    let mut session_id = client.send("GET", "/state", "", 200).await.body["id"].clone();
    for case in cases {
        let action = case["action"].as_str().unwrap();
        if action == "password" {
            client.send("POST", "/seed-tokens", "{}", 204).await;
        }
        if case["fail"] == true {
            server.database.unprepared("CREATE TRIGGER fortify_reject_token_delete BEFORE DELETE ON fortify_named_resets FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='fixture rejects token deletion'").await.unwrap();
        }
        server.events.lock().unwrap().clear();
        let (method, path) = match action {
            "register" => ("POST", "/register"),
            "profile" => ("PUT", "/user/profile-information"),
            "password" => ("PUT", "/user/password"),
            _ => panic!("invalid oracle action"),
        };
        client
            .send(
                method,
                path,
                &case["input"].to_string(),
                u16::try_from(case["status"].as_u64().unwrap()).unwrap(),
            )
            .await;
        let state = client.send("GET", "/state", "", 200).await.body;
        assert_eq!(state["email"], case["email"]);
        if action == "register" {
            assert_ne!(state["id"], session_id);
        } else {
            assert_eq!(state["id"], session_id);
        }
        session_id = state["id"].clone();
        let events: Vec<_> = server
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.rsplit('\\').next().unwrap().to_owned())
            .collect();
        assert_eq!(json!(events), case["events"]);
        assert_eq!(
            json!(
                server
                    .database
                    .table("fortify_named_resets")
                    .count("*")
                    .await
                    .unwrap()
            ),
            case["tokens"]
        );
        let row = server
            .database
            .table("fortify_account_users")
            .first()
            .await
            .unwrap()
            .unwrap();
        let Some(Value::Text(hash)) = row.get("password") else {
            panic!("password missing")
        };
        assert!(
            server
                .hasher
                .check(
                    case["password"].as_str().unwrap().as_bytes(),
                    hash,
                    &HashOptions::new()
                )
                .await
                .unwrap()
        );
        if case["fail"] == true {
            server
                .database
                .unprepared("DROP TRIGGER fortify_reject_token_delete")
                .await
                .unwrap();
        }
    }
}

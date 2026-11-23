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
static RESET_NOTIFICATIONS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static MARK_EMAIL_RETURNS_FALSE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static NOTIFICATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[derive(Clone)]
struct AccountUser(ModelState);
impl Model for AccountUser {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("fortify_account_users")
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
fn field<'a>(user: &'a AccountUser, name: &str) -> Option<&'a str> {
    match user.get_attribute(name) {
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
        self.set_attribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        self.get_attribute_value(name)
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
    fn get_email_for_password_reset(&self) -> &str {
        field(self, "email").unwrap()
    }
    fn send_password_reset_notification<'a>(
        &'a self,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            RESET_NOTIFICATIONS.lock().unwrap().push((
                self.get_email_for_password_reset().to_owned(),
                token.to_owned(),
            ));
            Ok(())
        })
    }
}
impl MustVerifyEmail for AccountUser {
    fn has_verified_email(&self) -> bool {
        self.get_attribute("email_verified_at")
            .is_some_and(|v| *v != Value::Null)
    }
    fn mark_email_as_verified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move {
            if MARK_EMAIL_RETURNS_FALSE.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(false);
            }
            if self.has_verified_email() {
                return Ok(false);
            }
            self.set_attribute("email_verified_at", "2026-11-20 00:00:00");
            self.save()
                .await
                .map_err(|error| AuthError::Backend(Box::new(error)))
        })
    }
    fn mark_email_as_unverified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn send_email_verification_notification(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(async {
            NOTIFICATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
    }
    fn get_email_for_verification(&self) -> &str {
        field(self, "email").unwrap()
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
    error.error_bag(bag);
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
                .await?;
            let mut user = AccountUser::from_state(ModelState::default());
            user.force_fill(
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
            )?
            .save()
            .await?;
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
            .validate_with_bag("updateProfileInformation")
            .await?;
            policy(values, "updateProfileInformation", true, false)?;
            let email = input(values, "email");
            let email = email.as_str().unwrap();
            let changed = field(&user, "email") != Some(email);
            user.force_fill(
                [
                    (
                        "name".into(),
                        input(values, "name").as_str().unwrap().into(),
                    ),
                    ("email".into(), email.into()),
                    ("email_verified_at".into(), Value::Null),
                ]
                .into(),
            )?
            .save()
            .await?;
            if changed {
                user.send_email_verification_notification().await?;
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
            .validate_with_bag("updatePassword")
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
                .await?
            {
                let mut bag = openrust::support::MessageBag::default();
                bag.add(
                    "current_password",
                    "The provided password does not match your current password.",
                );
                let mut error = ValidationException::new(bag);
                error.error_bag("updatePassword");
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
                .await?;
            user.force_fill([("password".into(), hash.into())].into())?
                .save()
                .await?;
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
            .map(|u| u.get_auth_attribute("email"))
            .transpose()?
            .flatten();
        let email = match email {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        };
        Response::json(
            StatusCode::OK,
            json!({"user":user.map(|u|u.get_auth_identifier()),"email":email,"id":session.get_id(),"csrf":session.token(),"status":session.get("status"),"errors":session.get("errors")}),
        )
    })
}
fn seed_tokens(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request).await?.unwrap();
        let user = user.as_password_reset().unwrap();
        Password::broker(Some("accounts"))?
            .create_token(user)
            .await?;
        Password::broker(Some("users"))?.create_token(user).await?;
        Ok(Response::no_content())
    })
}
fn intended(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        openrust::support::facades::Redirect::set_intended_url(request, "/account/settings")?;
        current(request).await
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/state", current)?;
    Route::get("/verification-link", verification_link)?;
    Route::get("/intended", intended)?;
    Route::group(
        openrust::routing::RouteGroup::new().middleware_name("auth"),
        |router| {
            router.post("/seed-tokens", seed_tokens)?;
            Ok(())
        },
    )?;
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
    database: openrust::database::Connection,
    fortify: Arc<Fortify>,
    events: Arc<Mutex<Vec<String>>>,
    #[allow(dead_code, reason = "Shared fixture also supports password HTTP tests")]
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
                "PasswordReset",
                "PasswordResetLinkSent",
                "Verified",
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
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([
        ("app".into(),json!({"key":"a".repeat(32),"timezone":"UTC","url":"http://localhost","debug":true})),
        ("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}})),
        ("session".into(),json!({"driver":"database","table":"fortify_account_sessions","cookie":"fortify_session","lottery":[0,100]})),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
        ("auth".into(),json!({"defaults":{"guard":"web","passwords":"users"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"AccountUser"}},"passwords":{"users":{"driver":"database","provider":"users","table":"fortify_default_resets","expire":60,"throttle":60},"accounts":{"driver":"database","provider":"users","table":"fortify_named_resets","expire":60,"throttle":60}}})),
    ].into())).with_providers(vec![Box::new(Settings(settings)),Box::new(FortifyServiceProvider)]).with_routing(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    let passwords: BTreeMap<String, openrust::translation::Line> = serde_json::from_str(
        include_str!("../../../framework/src/translation/stubs/passwords.json"),
    )
    .unwrap();
    app.make::<openrust::translation::Translator>()
        .await
        .unwrap()
        .add_lines(
            passwords
                .into_iter()
                .map(|(key, value)| (format!("passwords.{key}"), value)),
            "en",
            None,
        )
        .unwrap();
    let database = app
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let version = database
        .select_one("SELECT VERSION() AS version", &[])
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
        .register_model::<AccountUser>("AccountUser");
    let fortify = app.make::<Fortify>().await.unwrap();
    fortify.registerEmailVerification::<AccountUser>();
    fortify.registerPasswordReset::<AccountUser>();
    let hasher = app.make::<dyn Hasher>().await.unwrap();
    let actions = Arc::new(Actions {
        hasher: hasher.clone(),
    });
    if callbacks {
        fortify.resetUserPasswordsUsing(actions.clone());
        fortify.createUsersUsing(actions.clone());
        fortify.updateUserProfileInformationUsing(actions.clone());
        fortify.updateUserPasswordsUsing(actions.clone());
    } else {
        app.container()
            .instance::<dyn contracts::ResetsUserPasswords>(actions.clone())
            .unwrap();
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
   socket.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();assert!(status==0 || response.starts_with(&format!("HTTP/1.1 {status} ")),"{method} {path}: {response}");
   let (headers,body)=response.split_once("\r\n\r\n").unwrap();
   for line in headers.lines() {if let Some(cookie)=line.strip_prefix("set-cookie: ") {let (name,value)=cookie.split(';').next().unwrap().split_once('=').unwrap();self.cookies.insert(name.into(),value.into());}}
   Wire{headers:headers.into(),body:serde_json::from_str(body).unwrap_or(Json::String(body.into()))}
  }).await.unwrap()
    }
}
const LOGIN: &str = r#"{"email":"a@example.test","password":"good:secret"}"#;

const REGISTER: &str = r#"{"name":"Tester","email":"A@EXAMPLE.TEST","password":"good:secret","password_confirmation":"good:secret","remember":true}"#;

fn verification_link(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::{
            contracts::routing::UrlGenerator as _,
            routing::{RouteParameter, SignatureMode},
            support::facades::Url,
        };
        let id = request
            .query(Some("id"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("001".into());
        let email = request
            .query(Some("email"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("a@example.test".into());
        let hash: String = aws_lc_rs::digest::digest(
            &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
            email.as_bytes(),
        )
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
        let hash = request
            .query(Some("hash"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or(hash);
        let expires = if request.query(Some("expired"))?.is_some() {
            std::time::UNIX_EPOCH + Duration::from_secs(1)
        } else {
            std::time::SystemTime::now() + Duration::from_secs(600)
        };
        let url = Url::for_request(request)?.temporary_signed_route(
            "verification.verify",
            expires,
            &[
                RouteParameter::named("id", id),
                RouteParameter::named("hash", hash),
            ],
            SignatureMode::Absolute,
        )?;
        Response::json(StatusCode::OK, json!({"url":url}))
    })
}

impl contracts::ResetsUserPasswords for Actions {
    fn reset<'a>(
        &'a self,
        user: &'a mut dyn CanResetPassword,
        values: &'a RequestValue,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            Validator::make(
                validation_data(values)?,
                RuleSet::parse([("password", "required|string|min:8")])?,
            )?
            .validate()
            .await?;
            policy(values, "default", false, true)?;
            let hash = self
                .hasher
                .make(
                    input(values, "password").as_str().unwrap().as_bytes(),
                    &HashOptions::new(),
                )
                .await?;
            let user = (user as &mut dyn std::any::Any)
                .downcast_mut::<AccountUser>()
                .unwrap();
            user.set_attribute("password", hash);
            user.save().await?;
            Ok(())
        })
    }
}

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::{
        auth::Authenticatable,
        container::Container as _,
        events::{Dispatcher as _, Event, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{DatabaseManager, Value},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
};
use openrust_fortify::{
    Fortify, FortifyResult, FortifyServiceProvider, TwoFactorAuthenticatable,
    TwoFactorAuthenticationProvider,
    actions::{
        ConfirmTwoFactorAuthentication, DisableTwoFactorAuthentication,
        EnableTwoFactorAuthentication, GenerateNewRecoveryCodes,
    },
};
use serde_json::{Value as Json, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};
fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}
use openrust::database::eloquent::{Model, ModelMetadata, ModelState};
struct FactorUser(ModelState);

impl Model for FactorUser {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("fortify_factor_users")
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

impl Authenticatable for FactorUser {
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

fn field<'a>(user: &'a FactorUser, name: &str) -> Option<&'a str> {
    match user.get_attribute(name) {
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
    fn generate_secret_key(&self, length: usize) -> FortifyResult<String> {
        self.generated.lock().unwrap().push(length);
        Ok(SECRET.into())
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> String {
        self.engine.qr_code_url(company, account, secret)
    }
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> HttpFuture<'a, FortifyResult<bool>> {
        self.engine.verify(secret, code)
    }
}
struct Fixture {
    fortify: Arc<Fortify>,
    engine: Arc<Engine>,
    events: Arc<Mutex<Vec<String>>>,
}
struct FixtureFacade;
impl Facade for FixtureFacade {
    type Service = Fixture;
    fn get_facade_accessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<Fixture>()
    }
}
struct Ignore;
impl ServiceProvider for Ignore {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Fortify>().await?.ignore_routes();
            Ok(())
        })
    }
}
struct Observe(Arc<Mutex<Vec<String>>>);
impl Listener for Observe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            use openrust_fortify::events::*;
            macro_rules! check {($($kind:ty),+)=>{$(if let Some(event)=event.downcast_ref::<$kind>() {assert_eq!(event.user::<FactorUser>(context).unwrap().get_key(),Some(&Value::Text("001".into())));})+}}
            check!(
                TwoFactorAuthenticationEnabled,
                TwoFactorAuthenticationConfirmed,
                TwoFactorAuthenticationDisabled,
                RecoveryCodesGenerated,
                RecoveryCodeReplaced
            );
            if let Some(event) = event.downcast_ref::<RecoveryCodeReplaced>() {
                assert!(!format!("{event:?}").contains(event.code.as_str()));
            }
            self.0
                .lock()
                .unwrap()
                .push(event.name().rsplit('\\').next().unwrap().into());
            Ok(None)
        })
    }
}
fn action(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fixture = FixtureFacade::get_facade_root().map_err(internal)?;
        let op = request
            .input(Some("op"))
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut user = FactorUser::find("001")
            .await
            .map_err(internal)?
            .ok_or(HttpError::NotFound)?;
        match op.as_str() {
            "enable" => {
                EnableTwoFactorAuthentication::new(fixture.engine.clone(), fixture.fortify.clone())
                    .call(&mut user, request.boolean("force").await?)
                    .await?
            }
            "confirm" => {
                let code = request
                    .input(Some("code"))
                    .await?
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                ConfirmTwoFactorAuthentication::new(
                    fixture.engine.clone(),
                    fixture.fortify.clone(),
                )
                .call(&mut user, &code)
                .await?;
            }
            "generate" => {
                GenerateNewRecoveryCodes::new(fixture.fortify.clone())
                    .call(&mut user)
                    .await?
            }
            "disable" => {
                DisableTwoFactorAuthentication::new(fixture.fortify.clone())
                    .call(&mut user)
                    .await?
            }
            "replace" => {
                let code = request
                    .input(Some("code"))
                    .await?
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                user.replace_recovery_code(&code, &fixture.fortify).await?;
            }
            "status" => {}
            _ => return Err(HttpError::InvalidRequest),
        }
        let stored = FactorUser::find("001").await.map_err(internal)?.unwrap();
        let codes_json = match stored.get_attribute("two_factor_recovery_codes") {
            Some(Value::Text(ciphertext)) => Some(
                String::from_utf8(
                    fixture
                        .fortify
                        .current_encrypter()
                        .map_err(internal)?
                        .decrypt(ciphertext, true)
                        .map_err(internal)?,
                )
                .unwrap(),
            ),
            _ => None,
        };
        let secret = match stored.get_attribute("two_factor_secret") {
            Some(Value::Text(ciphertext)) => Some(
                String::from_utf8(
                    fixture
                        .fortify
                        .current_encrypter()
                        .map_err(internal)?
                        .decrypt(ciphertext, true)
                        .map_err(internal)?,
                )
                .unwrap(),
            ),
            _ => None,
        };
        let codes = if matches!(
            stored.get_attribute("two_factor_recovery_codes"),
            Some(Value::Text(_))
        ) {
            Some(stored.recovery_codes(&fixture.fortify).map_err(internal)?)
        } else {
            None
        };
        let qr = if secret.is_some() {
            Some(
                stored
                    .two_factor_qr_code_url(&fixture.fortify, fixture.engine.as_ref())
                    .map_err(internal)?,
            )
        } else {
            None
        };
        Response::json(
            StatusCode::OK,
            json!({"codes_json":codes_json,"secret":secret,"codes":codes,"qr":qr,"enabled":stored.has_enabled_two_factor_authentication(&fixture.fortify).map_err(internal)?,"confirmed":stored.get_attribute("two_factor_confirmed_at").is_some_and(|v|*v!=Value::Null),"generated":*fixture.engine.generated.lock().unwrap(),"events":*fixture.events.lock().unwrap()}),
        )
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::post("/factors", action)?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: SocketAddr, body: Json, status: u16) -> Json {
    timeout(Duration::from_secs(10),async {
  let mut stream=TcpStream::connect(address).await.unwrap();let body=body.to_string();
  let wire=format!("POST /api/factors HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
  stream.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();assert!(response.starts_with(&format!("HTTP/1.1 {status} ")),"{response}");
  serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
 }).await.unwrap()
}
#[tokio::test]
async fn two_factor_model_actions_persist_encrypted_state_and_events_on_mysql() {
    if std::env::var("OPENRUST_TEST_MYSQL_URL").is_err() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let mut app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
            Ok([
                (
                    "app".into(),
                    json!({"key":"a".repeat(32),"timezone":"Asia/Shanghai","name":"OpenRust App"}),
                ),
                (
                    "cache".into(),
                    json!({"default":"array","stores":{"array":{"driver":"array"}}}),
                ),
                (
                    "database".into(),
                    json!({"default":"mysql","connections":{"mysql":{"url":url}}}),
                ),
                (
                    "fortify".into(),
                    json!({"features":["two-factor-authentication"]}),
                ),
                (
                    "fortify-options".into(),
                    json!({"two-factor-authentication":{"confirm":true,"secret-length":" 16x"}}),
                ),
            ]
            .into())
        })
        .with_providers(vec![Box::new(FortifyServiceProvider), Box::new(Ignore)])
        .with_routing(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let version = db
        .select_one("SELECT VERSION() AS version", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(version.get("version"),Some(Value::Text(value)) if value.starts_with("8.4.")));
    db.statement("DROP TABLE IF EXISTS fortify_factor_users", &[])
        .await
        .unwrap();
    db.statement("CREATE TABLE fortify_factor_users (identity VARCHAR(20) PRIMARY KEY,email VARCHAR(100),password VARCHAR(255),remember_token VARCHAR(100),two_factor_secret TEXT NULL,two_factor_recovery_codes TEXT NULL,two_factor_confirmed_at DATETIME NULL)",&[]).await.unwrap();
    db.statement("INSERT INTO fortify_factor_users (identity,email,password) VALUES ('001','a@example.test','unused')",&[]).await.unwrap();
    let fortify = app.make::<Fortify>().await.unwrap();
    let generated = Arc::new(AtomicUsize::new(0));
    let counter = generated.clone();
    fortify.generate_recovery_codes_using(Some(Arc::new(move || {
        format!("code-{}", counter.fetch_add(1, Ordering::Relaxed))
    })));
    let events = Arc::new(Mutex::new(Vec::new()));
    let engine = Arc::new(Engine {
        engine: TwoFactorAuthenticationProvider::new(None)
            .with_clock(|| Ok(59))
            .with_window(0),
        generated: Mutex::new(Vec::new()),
    });
    app.container()
        .instance(Arc::new(Fixture {
            fortify: fortify.clone(),
            engine: engine.clone(),
            events: events.clone(),
        }))
        .unwrap();
    app.make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            "OpenRust\\Fortify\\Events\\*",
            Arc::new(Observe(events)),
            Delivery::Sync,
        )
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let cases: Vec<Json> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/api-f01/fixtures/two-factor-model.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 13);
    for case in cases {
        let step = case["step"].as_array().unwrap();
        let mut op = step[0].as_str().unwrap();
        if op == "unicode-generate" {
            fortify.generate_recovery_codes_using(Some(Arc::new(|| "é/😀\\x".into())));
            op = "generate";
        }
        if op == "unicode-replace" {
            op = "replace";
        }
        let mut body = json!({"op":op});
        if matches!(op, "confirm" | "replace") {
            body["code"] = step[1].clone();
        }
        if op == "enable" {
            body["force"] = step.get(1).cloned().unwrap_or(json!(false));
        }
        let response = send(
            address,
            body,
            u16::try_from(case["status"].as_u64().unwrap()).unwrap(),
        )
        .await;
        for (key, value) in case["response"].as_object().unwrap() {
            assert_eq!(&response[key], value, "step={step:?}, key={key}");
        }
        if op == "enable" {
            assert!(
                response["qr"]
                    .as_str()
                    .unwrap()
                    .contains("issuer=OpenRust%20App")
            );
            let row = db
                .table("fortify_factor_users")
                .first()
                .await
                .unwrap()
                .unwrap();
            for key in ["two_factor_secret", "two_factor_recovery_codes"] {
                let Some(Value::Text(value)) = row.get(key) else {
                    panic!("encrypted column missing")
                };
                assert!(!value.contains("code-"));
                assert!(!value.contains(SECRET));
            }
        }
    }
    server.abort();
    let _ = server.await;
    db.statement("DROP TABLE fortify_factor_users", &[])
        .await
        .unwrap();
}

#[test]
fn model_feature_checks_encryption_override_and_recovery_generator_are_application_scoped() {
    use openrust::{
        config::Repository,
        contracts::config::Repository as _,
        encryption::{Cipher, Encrypter},
    };
    let config = Arc::new(Repository::new(Default::default()));
    config
        .set("fortify.features", json!(["two-factor-authentication"]))
        .unwrap();
    config
        .set(
            "fortify-options.two-factor-authentication.confirm",
            json!(true),
        )
        .unwrap();
    let one = Fortify::new(config.clone());
    let two = Fortify::new(Arc::new(Repository::new(Default::default())));
    let mut user = FactorUser(ModelState::default());
    assert!(!user.has_enabled_two_factor_authentication(&one).unwrap());
    user.set_attribute("two_factor_secret", "");
    assert!(!user.has_enabled_two_factor_authentication(&one).unwrap());
    user.set_attribute("two_factor_confirmed_at", "");
    assert!(user.has_enabled_two_factor_authentication(&one).unwrap());
    user.set_attribute("two_factor_confirmed_at", Value::Null);
    config
        .set(
            "fortify-options.two-factor-authentication.confirm",
            json!(1),
        )
        .unwrap();
    assert!(user.has_enabled_two_factor_authentication(&one).unwrap());
    let key = Arc::new(Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap());
    one.encrypt_using(Some(key.clone()));
    let cipher = one
        .current_encrypter()
        .unwrap()
        .encrypt(br#"["original"]"#, true)
        .unwrap();
    user.set_attribute("two_factor_recovery_codes", cipher);
    assert_eq!(user.recovery_codes(&one).unwrap(), vec!["original"]);
    two.encrypt_using(Some(Arc::new(
        Encrypter::new([b'b'; 32], Cipher::Aes256Cbc).unwrap(),
    )));
    assert!(user.recovery_codes(&two).is_err());
    one.generate_recovery_codes_using(Some(Arc::new(|| "only-one".into())));
    assert_eq!(openrust_fortify::RecoveryCode::generate(&one), "only-one");
    let generated = openrust_fortify::RecoveryCode::generate(&two);
    assert_eq!(generated.len(), 21);
    assert_eq!(generated.as_bytes()[10], b'-');
    assert!(
        generated
            .chars()
            .all(|v| v == '-' || v.is_ascii_alphanumeric())
    );
    one.generate_recovery_codes_using(None);
    assert_ne!(openrust_fortify::RecoveryCode::generate(&one), "only-one");
    for invalid in ["null", "{}", r#"["original",5]"#] {
        user.set_attribute(
            "two_factor_recovery_codes",
            one.current_encrypter()
                .unwrap()
                .encrypt(invalid.as_bytes(), true)
                .unwrap(),
        );
        assert!(user.recovery_codes(&one).is_err());
    }
    user.set_attribute("two_factor_recovery_codes", Value::Integer(1));
    assert!(matches!(
        user.recovery_codes(&one),
        Err(openrust_fortify::FortifyError::InvalidAttribute(
            "two_factor_recovery_codes"
        ))
    ));
    one.encrypt_using(None);
    assert!(one.current_encrypter().is_err());
}
struct FailedEncrypter;
impl openrust::contracts::encryption::Encrypter for FailedEncrypter {
    fn encrypt(&self, _: &[u8], _: bool) -> Result<String, openrust::encryption::EncryptException> {
        Err(openrust::encryption::EncryptException::EncryptionFailed)
    }
    fn decrypt(&self, _: &str, _: bool) -> Result<Vec<u8>, openrust::encryption::DecryptException> {
        Err(openrust::encryption::DecryptException::AuthenticationFailed)
    }
    fn encrypt_string(
        &self,
        value: &[u8],
    ) -> Result<String, openrust::encryption::EncryptException> {
        self.encrypt(value, false)
    }
    fn decrypt_string(
        &self,
        value: &str,
    ) -> Result<Vec<u8>, openrust::encryption::DecryptException> {
        self.decrypt(value, false)
    }
    fn get_key(&self) -> &[u8] {
        &[]
    }
    fn get_all_keys(&self) -> Vec<&[u8]> {
        vec![]
    }
    fn get_previous_keys(&self) -> &[Vec<u8>] {
        &[]
    }
}
#[tokio::test]
async fn encryption_failures_stop_before_model_mutation_and_preserve_callback_order() {
    use openrust::{config::Repository, contracts::config::Repository as _};
    let config = Arc::new(Repository::new(Default::default()));
    config
        .set("fortify.features", json!(["two-factor-authentication"]))
        .unwrap();
    let fortify = Arc::new(Fortify::new(config));
    fortify.encrypt_using(Some(Arc::new(FailedEncrypter)));
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    fortify.generate_recovery_codes_using(Some(Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
        "recovery".into()
    })));
    let provider = Arc::new(Engine {
        engine: TwoFactorAuthenticationProvider::new(None),
        generated: Mutex::new(Vec::new()),
    });
    let mut user = FactorUser(ModelState::default());
    assert!(
        EnableTwoFactorAuthentication::new(provider.clone(), fortify.clone())
            .call(&mut user, false)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(*provider.generated.lock().unwrap(), vec![16]);
    assert!(user.state().get_attributes().is_empty());
    assert!(
        GenerateNewRecoveryCodes::new(fortify.clone())
            .call(&mut user)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 8);
    assert!(user.state().get_attributes().is_empty());
    user.set_attribute("two_factor_secret", "broken");
    let original = user.state().get_attributes().clone();
    assert!(
        ConfirmTwoFactorAuthentication::new(provider, fortify.clone())
            .call(&mut user, "287082")
            .await
            .is_err()
    );
    assert_eq!(user.state().get_attributes(), &original);
    assert!(
        user.replace_recovery_code("recovery", &fortify)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 8);
}

struct RecoveryCommand(openrust::console::CommandDefinition);
impl openrust::console::Command for RecoveryCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let fortify = Fortify::new(app.config().unwrap());
            let generated = Arc::new(AtomicUsize::new(0));
            let count = generated.clone();
            fortify.generate_recovery_codes_using(Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                "new-code".into()
            })));
            let db = app
                .make::<DatabaseManager>()
                .await
                .unwrap()
                .connection(None)
                .unwrap();
            db.get_schema_builder()
                .drop_if_exists("fortify_factor_users")
                .await
                .unwrap();
            db.get_schema_builder()
                .create("fortify_factor_users", |table| {
                    table.string("identity", 32).primary();
                    table.text("two_factor_recovery_codes");
                })
                .await
                .unwrap();
            let encrypted = fortify
                .current_encrypter()
                .unwrap()
                .encrypt(br#"["old","prefix-old"]"#, true)
                .unwrap();
            db.table("fortify_factor_users")
                .insert(&[[
                    ("identity".into(), "artisan".into()),
                    ("two_factor_recovery_codes".into(), encrypted.into()),
                ]
                .into()])
                .await
                .unwrap();
            let mut user = FactorUser::query_on(&db).first().await.unwrap().unwrap();
            let events = app
                .make::<openrust::events::Dispatcher>()
                .await
                .unwrap()
                .fake(&[r"OpenRust\Fortify\Events\RecoveryCodeReplaced"]);
            user.replace_recovery_code("old", &fortify).await.unwrap();
            assert_eq!(generated.load(Ordering::SeqCst), 1);
            assert_eq!(
                user.recovery_codes(&fortify).unwrap(),
                vec!["new-code", "prefix-new-code"]
            );
            assert_eq!(
                FactorUser::query_on(&db)
                    .first()
                    .await
                    .unwrap()
                    .unwrap()
                    .recovery_codes(&fortify)
                    .unwrap(),
                vec!["new-code", "prefix-new-code"]
            );
            events.assert_dispatched_once(r"OpenRust\Fortify\Events\RecoveryCodeReplaced");
            let event = events.dispatched(r"OpenRust\Fortify\Events\RecoveryCodeReplaced");
            let event = event[0]
                .downcast_ref::<openrust_fortify::events::RecoveryCodeReplaced>()
                .unwrap();
            assert_eq!(event.user, AuthIdentifier::String("artisan".into()));
            assert_eq!(event.code.as_str(), "old");
            db.get_schema_builder()
                .drop("fortify_factor_users")
                .await
                .unwrap();
            assert!(
                user.replace_recovery_code("new-code", &fortify)
                    .await
                    .is_err()
            );
            assert_eq!(generated.load(Ordering::SeqCst), 2);
            events.assert_dispatched_once(r"OpenRust\Fortify\Events\RecoveryCodeReplaced");
            Ok(0)
        })
    }
}
fn recovery_commands() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(RecoveryCommand(
        openrust::console::CommandDefinition::new(clap::Command::new("recovery:probe")),
    )))
}
#[tokio::test]
async fn native_artisan_recovery_replacement_keeps_string_semantics_and_failure_order() {
    let mut app = openrust::testing::TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                let driver = std::env::var("RECOVERY_TEST_DRIVER").unwrap_or("sqlite".into());
                let connection = match driver.as_str() {
                    "sqlite" => json!({"driver":"sqlite","database":":memory:"}),
                    "mysql" => json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}),
                    "pgsql" => json!({"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()}),
                    _ => panic!("未知恢复码验收驱动"),
                };
                Ok([
                    ("app".into(), json!({"key":"a".repeat(32)})),
                    (
                        "database".into(),
                        json!({"default":"recovery","connections":{"recovery":connection}}),
                    ),
                ]
                .into())
            })
            .with_routing(Routing::default().commands(recovery_commands))
    })
    .unwrap();
    app.artisan(["recovery:probe"]).await.assert_successful();
}

#[tokio::test]
async fn transactional_two_factor_actions_share_user_connection_and_rollback_cancelled_changes() {
    use openrust::support::facades::App;
    let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("需要项目内临时目录"))
        .join(format!("two-factor-transaction-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=Application::configure(root.clone()).with_config(|_,_|{
        let settings=match std::env::var("OPENRUST_TWO_FACTOR_TRANSACTION_DRIVER").as_deref().unwrap_or("sqlite") {
            "sqlite"=>json!({"driver":"sqlite","database":":memory:"}),
            driver @ ("mysql"|"pgsql")=>json!({"driver":driver,"url":std::env::var("OPENRUST_TWO_FACTOR_TRANSACTION_URL").expect("必须提供独立双因素验收数据库")}),
            _=>panic!("不支持的双因素验收数据库"),
        };
        Ok([("app".into(),json!({"key":"a".repeat(32),"timezone":"UTC","name":"Transaction test"})),("database".into(),json!({"default":"factors","connections":{"factors":settings.clone(),"other":settings}})),("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),("fortify".into(),json!({"features":["two-factor-authentication"]})),("fortify-options".into(),json!({"two-factor-authentication":{"confirm":true}}))].into())
    }).with_providers(vec![Box::new(FortifyServiceProvider),Box::new(Ignore)]).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let db = app.make::<DatabaseManager>().await.unwrap();
        let connection = db.connection(None).unwrap();
        connection
            .get_schema_builder()
            .create("fortify_factor_users", |table| {
                table.string("identity", 32).primary();
                table.text("email");
                table.text("two_factor_secret").nullable();
                table.text("two_factor_recovery_codes").nullable();
                table.timestamp("two_factor_confirmed_at", None).nullable();
            })
            .await
            .unwrap();
        connection
            .table("fortify_factor_users")
            .insert(&[[
                ("identity".into(), "001".into()),
                ("email".into(), "factors@example.test".into()),
            ]
            .into()])
            .await
            .unwrap();
        let fortify = app.make::<Fortify>().await.unwrap();
        let engine = Arc::new(Engine {
            engine: TwoFactorAuthenticationProvider::new(None)
                .with_clock(|| Ok(59))
                .with_window(0),
            generated: Mutex::new(Vec::new()),
        });
        let enable = EnableTwoFactorAuthentication::new(engine.clone(), fortify.clone());
        let confirm = ConfirmTwoFactorAuthentication::new(engine.clone(), fortify.clone());
        let disable = DisableTwoFactorAuthentication::new(fortify.clone());
        let regenerate = GenerateNewRecoveryCodes::new(fortify.clone());
        let mut user = FactorUser::find("001").await.unwrap().unwrap();
        let mut foreign = db
            .connection(Some("other"))
            .unwrap()
            .begin_transaction()
            .await
            .unwrap();
        assert!(
            enable
                .call_in_transaction(&mut user, &mut foreign, false)
                .await
                .is_err()
        );
        assert!(engine.generated.lock().unwrap().is_empty());
        foreign.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        enable
            .call_in_transaction(&mut user, &mut tx, false)
            .await
            .unwrap();
        assert_eq!(user.recovery_codes(&fortify).unwrap().len(), 8);
        tx.roll_back(None).await.unwrap();
        let mut user = FactorUser::find("001").await.unwrap().unwrap();
        assert!(field(&user, "two_factor_secret").is_none());
        let mut tx = connection.begin_transaction().await.unwrap();
        enable
            .call_in_transaction(&mut user, &mut tx, false)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let stored = field(&user, "two_factor_secret").unwrap().to_owned();
        assert_ne!(stored, SECRET);
        let old_codes = user.recovery_codes(&fortify).unwrap();
        let mut foreign = db
            .connection(Some("other"))
            .unwrap()
            .begin_transaction()
            .await
            .unwrap();
        assert!(
            enable
                .call_in_transaction(&mut user, &mut foreign, false)
                .await
                .is_err()
        );
        assert!(
            confirm
                .call_in_transaction(&mut user, &mut foreign, "287082")
                .await
                .is_err()
        );
        assert!(
            disable
                .call_in_transaction(&mut user, &mut foreign)
                .await
                .is_err()
        );
        assert!(
            regenerate
                .call_in_transaction(&mut user, &mut foreign)
                .await
                .is_err()
        );
        foreign.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            confirm
                .call_in_transaction(&mut user, &mut tx, "wrong")
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        confirm
            .call_in_transaction(&mut user, &mut tx, "287082")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(
            user.has_enabled_two_factor_authentication(&fortify)
                .unwrap()
        );
        let mut tx = connection.begin_transaction().await.unwrap();
        regenerate
            .call_in_transaction(&mut user, &mut tx)
            .await
            .unwrap();
        tx.roll_back(None).await.unwrap();
        let mut user = FactorUser::find("001").await.unwrap().unwrap();
        assert_eq!(user.recovery_codes(&fortify).unwrap(), old_codes);
        let mut tx = connection.begin_transaction().await.unwrap();
        regenerate
            .call_in_transaction(&mut user, &mut tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_ne!(user.recovery_codes(&fortify).unwrap(), old_codes);
        let mut tx = connection.begin_transaction().await.unwrap();
        disable
            .call_in_transaction(&mut user, &mut tx)
            .await
            .unwrap();
        tx.roll_back(None).await.unwrap();
        let mut user = FactorUser::find("001").await.unwrap().unwrap();
        assert!(
            user.has_enabled_two_factor_authentication(&fortify)
                .unwrap()
        );
        let mut tx = connection.begin_transaction().await.unwrap();
        disable
            .call_in_transaction(&mut user, &mut tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(
            !user
                .has_enabled_two_factor_authentication(&fortify)
                .unwrap()
        );
        assert!(field(&user, "two_factor_recovery_codes").is_none());
        FactorUser::register_model_event(
            &app,
            openrust::database::eloquent::concerns::ModelEvent::Saving,
            CancelFactorSave,
        )
        .unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            enable
                .call_in_transaction(&mut user, &mut tx, false)
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        assert!(
            field(
                &FactorUser::find("001").await.unwrap().unwrap(),
                "two_factor_secret"
            )
            .is_none()
        );
    })
    .await;
    std::fs::remove_dir_all(root).unwrap();
}
struct CancelFactorSave;
impl openrust::database::eloquent::concerns::ModelEventListener<FactorUser> for CancelFactorSave {
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        _: &'a mut FactorUser,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async { Ok(openrust::database::eloquent::concerns::ModelEventResponse::Cancel) })
    }
}

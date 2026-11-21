#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

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
struct Fixture {
    fortify: Arc<Fortify>,
    engine: Arc<Engine>,
    events: Arc<Mutex<Vec<String>>>,
}
struct FixtureFacade;
impl Facade for FixtureFacade {
    type Service = Fixture;
    fn getFacadeAccessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<Fixture>()
    }
}
struct Ignore;
impl ServiceProvider for Ignore {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Fortify>().await?.ignoreRoutes();
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
            macro_rules! check {($($kind:ty),+)=>{$(if let Some(event)=event.downcast_ref::<$kind>() {assert_eq!(event.user::<FactorUser>(context).unwrap().getKey(),Some(&Value::Text("001".into())));})+}}
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
        let fixture = FixtureFacade::getFacadeRoot().map_err(internal)?;
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
                    .call(&mut user, request.boolean("force", false, false).await?)
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
                user.replaceRecoveryCode(&code, &fixture.fortify).await?;
            }
            "status" => {}
            _ => return Err(HttpError::InvalidRequest),
        }
        let stored = FactorUser::find("001").await.map_err(internal)?.unwrap();
        let codes_json = match stored.getAttribute("two_factor_recovery_codes") {
            Some(Value::Text(ciphertext)) => Some(
                String::from_utf8(
                    fixture
                        .fortify
                        .currentEncrypter()
                        .map_err(internal)?
                        .decrypt(ciphertext, true)
                        .map_err(internal)?,
                )
                .unwrap(),
            ),
            _ => None,
        };
        let secret = match stored.getAttribute("two_factor_secret") {
            Some(Value::Text(ciphertext)) => Some(
                String::from_utf8(
                    fixture
                        .fortify
                        .currentEncrypter()
                        .map_err(internal)?
                        .decrypt(ciphertext, true)
                        .map_err(internal)?,
                )
                .unwrap(),
            ),
            _ => None,
        };
        let codes = if matches!(
            stored.getAttribute("two_factor_recovery_codes"),
            Some(Value::Text(_))
        ) {
            Some(stored.recoveryCodes(&fixture.fortify).map_err(internal)?)
        } else {
            None
        };
        let qr = if secret.is_some() {
            Some(
                stored
                    .twoFactorQrCodeUrl(&fixture.fortify, fixture.engine.as_ref())
                    .map_err(internal)?,
            )
        } else {
            None
        };
        Response::json(
            StatusCode::OK,
            json!({"codes_json":codes_json,"secret":secret,"codes":codes,"qr":qr,"enabled":stored.hasEnabledTwoFactorAuthentication(&fixture.fortify).map_err(internal)?,"confirmed":stored.getAttribute("two_factor_confirmed_at").is_some_and(|v|*v!=Value::Null),"generated":*fixture.engine.generated.lock().unwrap(),"events":*fixture.events.lock().unwrap()}),
        )
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::r#match(&[openrust::http::Method::POST], "/factors", action)?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
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
        .withConfig(|_, _| {
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
        .withProviders(vec![Box::new(FortifyServiceProvider), Box::new(Ignore)])
        .withRouting(Routing::default().api(routes))
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
        .selectOne("SELECT VERSION() AS version", &[])
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
    fortify.generateRecoveryCodesUsing(Some(Arc::new(move || {
        format!("code-{}", counter.fetch_add(1, Ordering::Relaxed))
    })));
    let events = Arc::new(Mutex::new(Vec::new()));
    let engine = Arc::new(Engine {
        engine: TwoFactorAuthenticationProvider::new(None)
            .withClock(|| Ok(59))
            .withWindow(0),
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
        "../../../../.agents/project/audits/api-f01/fixtures/two-factor-model.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 13);
    for case in cases {
        let step = case["step"].as_array().unwrap();
        let mut op = step[0].as_str().unwrap();
        if op == "unicode-generate" {
            fortify.generateRecoveryCodesUsing(Some(Arc::new(|| "é/😀\\x".into())));
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
    assert!(!user.hasEnabledTwoFactorAuthentication(&one).unwrap());
    user.setAttribute("two_factor_secret", "");
    assert!(!user.hasEnabledTwoFactorAuthentication(&one).unwrap());
    user.setAttribute("two_factor_confirmed_at", "");
    assert!(user.hasEnabledTwoFactorAuthentication(&one).unwrap());
    user.setAttribute("two_factor_confirmed_at", Value::Null);
    config
        .set(
            "fortify-options.two-factor-authentication.confirm",
            json!(1),
        )
        .unwrap();
    assert!(user.hasEnabledTwoFactorAuthentication(&one).unwrap());
    let key = Arc::new(Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap());
    one.encryptUsing(Some(key.clone()));
    let cipher = one
        .currentEncrypter()
        .unwrap()
        .encrypt(br#"["original"]"#, true)
        .unwrap();
    user.setAttribute("two_factor_recovery_codes", cipher);
    assert_eq!(user.recoveryCodes(&one).unwrap(), vec!["original"]);
    two.encryptUsing(Some(Arc::new(
        Encrypter::new([b'b'; 32], Cipher::Aes256Cbc).unwrap(),
    )));
    assert!(user.recoveryCodes(&two).is_err());
    one.generateRecoveryCodesUsing(Some(Arc::new(|| "only-one".into())));
    assert_eq!(openrust_fortify::RecoveryCode::generate(&one), "only-one");
    let generated = openrust_fortify::RecoveryCode::generate(&two);
    assert_eq!(generated.len(), 21);
    assert_eq!(generated.as_bytes()[10], b'-');
    assert!(
        generated
            .chars()
            .all(|v| v == '-' || v.is_ascii_alphanumeric())
    );
    one.generateRecoveryCodesUsing(None);
    assert_ne!(openrust_fortify::RecoveryCode::generate(&one), "only-one");
    for invalid in ["null", "{}", r#"["original",5]"#] {
        user.setAttribute(
            "two_factor_recovery_codes",
            one.currentEncrypter()
                .unwrap()
                .encrypt(invalid.as_bytes(), true)
                .unwrap(),
        );
        assert!(user.recoveryCodes(&one).is_err());
    }
    user.setAttribute("two_factor_recovery_codes", Value::Integer(1));
    assert!(matches!(
        user.recoveryCodes(&one),
        Err(openrust_fortify::FortifyError::InvalidAttribute(
            "two_factor_recovery_codes"
        ))
    ));
    one.encryptUsing(None);
    assert!(one.currentEncrypter().is_err());
}
struct FailedEncrypter;
impl openrust::contracts::encryption::Encrypter for FailedEncrypter {
    fn encrypt(&self, _: &[u8], _: bool) -> Result<String, openrust::encryption::EncryptException> {
        Err(openrust::encryption::EncryptException::EncryptionFailed)
    }
    fn decrypt(&self, _: &str, _: bool) -> Result<Vec<u8>, openrust::encryption::DecryptException> {
        Err(openrust::encryption::DecryptException::AuthenticationFailed)
    }
    fn encryptString(
        &self,
        value: &[u8],
    ) -> Result<String, openrust::encryption::EncryptException> {
        self.encrypt(value, false)
    }
    fn decryptString(
        &self,
        value: &str,
    ) -> Result<Vec<u8>, openrust::encryption::DecryptException> {
        self.decrypt(value, false)
    }
    fn getKey(&self) -> &[u8] {
        &[]
    }
    fn getAllKeys(&self) -> Vec<&[u8]> {
        vec![]
    }
    fn getPreviousKeys(&self) -> &[Vec<u8>] {
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
    fortify.encryptUsing(Some(Arc::new(FailedEncrypter)));
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    fortify.generateRecoveryCodesUsing(Some(Arc::new(move || {
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
    assert!(user.state().getAttributes().is_empty());
    assert!(
        GenerateNewRecoveryCodes::new(fortify.clone())
            .call(&mut user)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 8);
    assert!(user.state().getAttributes().is_empty());
    user.setAttribute("two_factor_secret", "broken");
    let original = user.state().getAttributes().clone();
    assert!(
        ConfirmTwoFactorAuthentication::new(provider, fortify.clone())
            .call(&mut user, "287082")
            .await
            .is_err()
    );
    assert_eq!(user.state().getAttributes(), &original);
    assert!(
        user.replaceRecoveryCode("recovery", &fortify)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 8);
}

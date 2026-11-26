//! 认证标识与模型主键分离时，从公开通知、认证和验证请求入口实测。
use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{Auth, AuthFuture, AuthIdentifier, AuthResult, notifications::VerifyEmail},
    contracts::{
        auth::{Authenticatable, MustVerifyEmail},
        events::{Dispatcher as _, Event, Listener},
        http::Kernel as _,
    },
    database::{
        DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError, SignatureMode},
    support::facades::{Date, Url},
};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
};

#[derive(Clone)]
pub(crate) struct Principal<const K: u8>(ModelState);
impl<const K: u8> Model for Principal<K> {
    fn metadata() -> &'static ModelMetadata {
        static NUM: ModelMetadata = ModelMetadata::new("verification_numbers")
            .primary_key("identity")
            .incrementing(false)
            .timestamps(None, None);
        static UUID: ModelMetadata = ModelMetadata::new("verification_uuids")
            .primary_key("identity")
            .incrementing(false)
            .timestamps(None, None);
        static ULID: ModelMetadata = ModelMetadata::new("verification_ulids")
            .primary_key("identity")
            .incrementing(false)
            .timestamps(None, None);
        match K {
            0 => &NUM,
            1 => &UUID,
            _ => &ULID,
        }
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
impl<const K: u8> Authenticatable for Principal<K> {
    fn get_auth_identifier_name(&self) -> &str {
        "email"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::String(self.get_email_for_verification().into())
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn as_email_verification(&self) -> Option<&dyn MustVerifyEmail> {
        Some(self)
    }
}
impl<const K: u8> MustVerifyEmail for Principal<K> {
    fn get_email_verification_identifier(&self) -> AuthResult<AuthIdentifier> {
        self.get_key()
            .ok_or(openrust::auth::AuthError::InvalidIdentity)?
            .try_into()
    }
    fn has_verified_email(&self) -> bool {
        self.get_attribute("email_verified_at")
            .is_some_and(|v| *v != Value::Null)
    }
    fn mark_email_as_verified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(openrust::auth::email_verification::set_verification(
            self,
            true,
            "email",
            "email_verified_at",
            "updated_at",
        ))
    }
    fn mark_email_as_unverified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(openrust::auth::email_verification::set_verification(
            self,
            false,
            "email",
            "email_verified_at",
            "updated_at",
        ))
    }
    fn get_email_for_verification(&self) -> &str {
        match self.get_attribute("email") {
            Some(Value::Text(value)) => value,
            _ => "",
        }
    }
    fn send_email_verification_notification(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(VerifyEmail::send(self))
    }
}
struct Shadow<const K: u8>(Principal<K>);
impl<const K: u8> Authenticatable for Shadow<K> {
    fn get_auth_identifier_name(&self) -> &str {
        self.0.get_auth_identifier_name()
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.0.get_auth_identifier()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
}
impl<const K: u8> MustVerifyEmail for Shadow<K> {
    fn get_email_verification_identifier(&self) -> AuthResult<AuthIdentifier> {
        self.0.get_email_verification_identifier()
    }
    fn has_verified_email(&self) -> bool {
        self.0.has_verified_email()
    }
    fn mark_email_as_verified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        self.0.mark_email_as_verified()
    }
    fn mark_email_as_unverified(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        self.0.mark_email_as_unverified()
    }
    fn get_email_for_verification(&self) -> &str {
        self.0.get_email_for_verification()
    }
    fn send_email_verification_notification(&self) -> AuthFuture<'_, AuthResult<()>> {
        self.0.send_email_verification_notification()
    }
}
struct CountVerified(Arc<AtomicUsize>);
impl Listener for CountVerified {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        })
    }
}
async fn principal<const K: u8>(request: &mut Request) -> Result<Principal<K>, HttpError> {
    let user = Auth::user(request)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    (user.as_ref() as &dyn std::any::Any)
        .downcast_ref::<Principal<K>>()
        .cloned()
        .ok_or(HttpError::Unauthenticated)
}
fn links<const K: u8>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = principal::<K>(request).await?;
        user.send_email_verification_notification().await?;
        let key = user.get_email_verification_identifier()?;
        let key = match key {
            AuthIdentifier::String(value) => value,
            AuthIdentifier::Integer(value) => value.to_string(),
            AuthIdentifier::UnsignedInteger(value) => value.to_string(),
        };
        let hash = aws_lc_rs::digest::digest(
            &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
            user.get_email_for_verification().as_bytes(),
        );
        let hash = hash
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let at = Date::now(None)
            .expect("应用时钟")
            .as_date_time()
            .timestamp()
            + jiff::SignedDuration::from_secs(3600);
        let shadow = Url::temporary_signed_route(
            "verification.other",
            at.into(),
            &[
                RouteParameter::named("id", key),
                RouteParameter::named("hash", hash),
            ],
            SignatureMode::Absolute,
        )?;
        Response::json(
            StatusCode::OK,
            json!({"url":VerifyEmail::url(&user)?,"other":shadow}),
        )
    })
}
fn fulfill<const K: u8>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut user = principal::<K>(request).await?;
        openrust::foundation::auth::EmailVerificationRequest::fulfill(request, &mut user).await?;
        Ok(Response::no_content())
    })
}
fn other<const K: u8>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut user = Shadow(principal::<K>(request).await?);
        openrust::foundation::auth::EmailVerificationRequest::fulfill(request, &mut user).await?;
        Ok(Response::no_content())
    })
}
pub(crate) trait Flow {
    fn providers<const K: u8>() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
        Vec::new()
    }
    fn register_routes<const K: u8>(router: &mut Router) -> Result<(), RoutingError> {
        router
            .get("/email/verify/{id}/{hash}", fulfill::<K>)?
            .middleware_name("auth:api")
            .middleware_name("signed")
            .name("verification.verify");
        Ok(())
    }
}
fn routes<const K: u8>(router: &mut Router) -> Result<(), RoutingError> {
    router
        .get("/verification-link", links::<K>)?
        .middleware_name("auth:api");
    router
        .get("/other/verify/{id}/{hash}", other::<K>)?
        .middleware_name("auth:api")
        .middleware_name("signed")
        .name("verification.other");
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::http::Request<axum::body::Body>,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn exchange(address: SocketAddr, path: &str, token: &str) -> (u16, String) {
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
  let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
  let mut wire=String::new();stream.read_to_string(&mut wire).await.unwrap();let(headers,body)=wire.split_once("\r\n\r\n").unwrap();assert!(!headers.to_ascii_lowercase().contains("set-cookie:"));(headers.split_whitespace().nth(1).unwrap().parse().unwrap(),body.into())
 }).await.unwrap()
}
pub(crate) async fn verify<const K: u8, F: Flow>() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let prefix = format!(
        "verify_identity_{}_{}_",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let configured = if let Ok(driver) = std::env::var("VERIFICATION_KEY_DATABASE") {
        json!({"driver":driver,"url":std::env::var("VERIFICATION_KEY_DATABASE_URL").unwrap(),"prefix":prefix,"prefix_indexes":true})
    } else {
        json!({"driver":"sqlite","database":":memory:"})
    };
    let root = std::env::temp_dir().join(&prefix);
    std::fs::create_dir_all(&root).unwrap();
    struct Root(std::path::PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _root = Root(root.clone());
    let values:std::collections::BTreeMap<String,serde_json::Value>=[

  ("app".into(),json!({"url":origin,"key":"0123456789abcdef0123456789abcdef"})),
  ("database".into(),json!({"default":"verify","connections":{"verify":configured}})),
  ("auth".into(),json!({"defaults":{"guard":"api"},"guards":{"web":{"driver":"session","provider":"people"},"api":{"driver":"token","provider":"people","storage_key":"api_token","hash":false}},"providers":{"people":{"driver":"eloquent","model":"Principal"}}})),
  ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
  ("fortify".into(),json!({"guard":"web","views":false,"middleware":[],"features":["email-verification"],"auth_middleware":"auth:api","verification_auth_middleware":"auth:api","limiters":{"verification":"100,1"}})),
  ("mail".into(),json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test","name":"身份验证"}})),

 ].into();
    std::fs::write(
        root.join("configuration.json"),
        serde_json::to_vec(&values).unwrap(),
    )
    .unwrap();
    let mut app = Application::configure(root)
        .with_config(configuration)
        .with_providers(F::providers::<K>())
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            routes::<K>(&mut router)?;
            F::register_routes::<K>(&mut router)?;
            Ok(router)
        }))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.make::<openrust::auth::AuthManager>()
        .await
        .unwrap()
        .register_model::<Principal<K>>("Principal");
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let table = Principal::<K>::from_state(Default::default())
        .get_table()
        .to_owned();
    db.get_schema_builder()
        .create(&table, |blueprint| {
            if K == 0 {
                blueprint.big_integer("identity").primary();
            } else {
                blueprint.string("identity", 64).primary();
            }
            blueprint.string("email", 255).unique();
            blueprint.string("api_token", 255);
            blueprint.timestamp("email_verified_at", None).nullable();
            blueprint.timestamp("updated_at", None).nullable();
        })
        .await
        .unwrap();
    let keys = match K {
        0 => [Value::Integer(123), Value::Integer(124)],
        1 => [
            Value::from("018ee174-0d5b-7a6f-9231-012345678901"),
            Value::from("018ee174-0d5b-7a6f-9231-012345678902"),
        ],
        _ => [
            Value::from("01J9Z3H7B7E6B7B7J89VV7TPFZ"),
            Value::from("01J9Z3H7B7E6B7B7J89VV7TPFY"),
        ],
    };
    for (index, key) in keys.iter().enumerate() {
        db.table(&table)
            .insert(&[[
                ("identity".into(), key.clone()),
                ("email".into(), format!("user{index}@example.test").into()),
                ("api_token".into(), format!("fixture-{index}").into()),
            ]
            .into()])
            .await
            .unwrap();
    }
    let clock = app.make::<openrust::support::DateFactory>().await.unwrap();
    let at = "2030-01-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
    clock.set_test_now(Some(openrust::support::Date::from_utc(
        at,
        jiff::tz::TimeZone::UTC,
    )));
    let events = Arc::new(AtomicUsize::new(0));
    app.make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            "Verified",
            Arc::new(CountVerified(events.clone())),
            Delivery::Sync,
        )
        .unwrap();
    let mailer = app
        .make::<openrust::mail::MailManager>()
        .await
        .unwrap()
        .mailer(None)
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    struct Stop(tokio::task::JoinHandle<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Stop(task);
    let (status, body) = exchange(address, "/verification-link", "fixture-0").await;
    assert_eq!(status, 200, "{body}");
    let links: serde_json::Value = serde_json::from_str(&body).unwrap();
    let link = links["url"].as_str().unwrap();
    let other = links["other"]
        .as_str()
        .unwrap()
        .strip_prefix(&format!("http://{address}"))
        .unwrap();
    assert!(!link.contains("user0"));
    assert_eq!(mailer.sent().unwrap().len(), 1);
    let link = link.strip_prefix(&format!("http://{address}")).unwrap();
    let (status, body) = exchange(address, link, "fixture-1").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(exchange(address, other, "fixture-0").await.0, 403);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    let (a, b) = tokio::join!(
        exchange(address, link, "fixture-0"),
        exchange(address, link, "fixture-0")
    );
    assert_eq!([a.0, b.0], [204, 204]);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    let row = db
        .table(&table)
        .r#where("identity", "=", keys[0].clone())
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get("email_verified_at"),
        Some(&Value::from("2030-01-01 00:00:00"))
    );
    assert_eq!(exchange(address, link, "fixture-0").await.0, 204);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    db.table(&table)
        .r#where("identity", "=", keys[0].clone())
        .update(
            [
                ("email".into(), "changed@example.test".into()),
                ("email_verified_at".into(), Value::Null),
            ]
            .into(),
        )
        .await
        .unwrap();
    assert_eq!(exchange(address, link, "fixture-0").await.0, 403);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    let (status, body) = exchange(address, "/verification-link", "fixture-0").await;
    assert_eq!(status, 200, "{body}");
    let links: serde_json::Value = serde_json::from_str(&body).unwrap();
    let link = links["url"]
        .as_str()
        .unwrap()
        .strip_prefix(&format!("http://{address}"))
        .unwrap();
    assert_eq!(exchange(address, link, "fixture-0").await.0, 204);
    assert_eq!(events.load(Ordering::SeqCst), 2);
    clock.set_test_now(Some(openrust::support::Date::from_utc(
        at + jiff::SignedDuration::from_secs(3601),
        jiff::tz::TimeZone::UTC,
    )));
    assert_eq!(exchange(address, link, "fixture-0").await.0, 403);
    db.get_schema_builder().drop(&table).await.unwrap();
}

fn configuration(
    base: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(serde_json::from_slice(&std::fs::read(base.join("configuration.json")).unwrap()).unwrap())
}

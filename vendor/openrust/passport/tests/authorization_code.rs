use axum::{extract::State, handler::Handler as _};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::{
    auth::{AuthIdentifier, AuthManager, Credentials},
    config::{ConfigError, Environment, Value as Json},
    contracts::{
        auth::{Authenticatable, StatefulGuard as _},
        container::Container as _,
        foundation::Application as _,
        http::Kernel as _,
        session::Session as _,
    },
    database::{
        Connection, DatabaseManager, Value,
        eloquent::{Attributes, Model, ModelMetadata, ModelState},
        migrations::{MigrationEntry, Migrator, RunOptions},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    support::facades::{Auth, Hash, Route},
};
use openrust_passport::{
    ClientRepository, Passport, PassportServiceProvider, http::AuthorizationViewParameters,
    migrations,
};
use serde_json::{Value as J, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const CALLBACK: &str = "https://client.test/cb?existing=1";
static MIGRATIONS: &[MigrationEntry] = &[
    MigrationEntry::new(
        "2024_06_01_000001",
        &migrations::oauth_device_codes::Definition,
    ),
    MigrationEntry::new("2016_06_01_000004", &migrations::oauth_clients::Definition),
    MigrationEntry::new(
        "2016_06_01_000001",
        &migrations::oauth_auth_codes::Definition,
    ),
    MigrationEntry::new(
        "2016_06_01_000002",
        &migrations::oauth_access_tokens::Definition,
    ),
    MigrationEntry::new(
        "2016_06_01_000003",
        &migrations::oauth_refresh_tokens::Definition,
    ),
];
struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("users")
            .guarded(&[])
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
impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "subject"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.getAttribute("subject").unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        match self.getAttribute("password") {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        }
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Json>, ConfigError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let prefix = format!(
        "authcode_{}_{}_",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").expect("MySQL wrapper required");
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"key":"k".repeat(32),"name":"OAuth test","env":"testing","host":"127.0.0.1","port":0}),
        ),
        (
            "database".into(),
            json!({"default":"main","connections":{"main":{"url":url,"prefix":prefix}},"migrations":{"table":"history"}}),
        ),
        (
            "session".into(),
            json!({"driver":"array","cookie":"oauth_session","lottery":[0,100]}),
        ),
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
        (
            "auth".into(),
            json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"},"api":{"driver":"passport","provider":"users"},"other":{"driver":"passport","provider":"other_users"}},"providers":{"users":{"driver":"eloquent","model":"OAuthUser"},"other_users":{"driver":"eloquent","model":"OAuthUser"}}}),
        ),
        (
            "hashing".into(),
            json!({"driver":"bcrypt","bcrypt":{"rounds":4}}),
        ),
    ]))
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    default_grants::routes()?;
    Route::get("/session", session)?;
    Route::r#match(
        &[Method::POST],
        "/custom-token",
        openrust_passport::http::controllers::AccessTokenController::issueToken,
    )?;
    Route::group(
        openrust::routing::RouteGroup::new().middleware_name("auth:api"),
        |router| {
            router.get("/resource", resource)?;
            router.r#match(&[Method::DELETE], "/resource", revoke_resource)?;
            Ok(())
        },
    )?;
    Route::group(
        openrust::routing::RouteGroup::new().middleware_name("auth:other"),
        |router| {
            router.get("/resource-other", resource_other)?;
            Ok(())
        },
    )?;
    Route::r#match(&[Method::POST], "/login", login)
}
fn session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({"csrf":request.session()?.token(),"user":Auth::guard(request,None)?.id().await?}),
        )
    })
}
fn login(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.input(None).await?.unwrap();
        let credentials = Credentials::new(
            [("email", Value::from(input["email"].as_str().unwrap()))],
            input["password"].as_str().unwrap(),
        );
        if !Auth::sessionGuard(request, None)?
            .attempt(&credentials, false)
            .await?
        {
            return Err(HttpError::Unauthenticated);
        }
        session(request).await
    })
}
fn view(
    request: &mut Request,
    params: AuthorizationViewParameters,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({"auth_token":params.auth_token,"csrf":request.session()?.token(),"client":params.client.toArray().await.unwrap(),"user":params.user.get_auth_identifier(),"scopes":params.scopes}),
        )
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}
struct Fixture {
    root: PathBuf,
    base: String,
    client: reqwest::Client,
    db: Connection,
    passport: Arc<Passport>,
    encryption: Arc<openrust::encryption::Encrypter>,
    veto: Arc<std::sync::atomic::AtomicBool>,
    server: tokio::task::JoinHandle<()>,
    kernel: Arc<Kernel>,
    public: String,
    confidential: String,
    secret: String,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_device(true, None).await
    }
    async fn with_device(enabled: bool, prefix: Option<&str>) -> Self {
        Self::with_device_routes(enabled, prefix, true).await
    }
    async fn with_device_routes(enabled: bool, prefix: Option<&str>, register: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "passport-grant-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("storage")).unwrap();
        std::fs::write(
            root.join("storage/oauth-private.key"),
            include_str!("fixtures/oauth-private.pem"),
        )
        .unwrap();
        std::fs::write(
            root.join("storage/oauth-public.key"),
            include_str!("fixtures/oauth-public.pem"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(
                root.join("storage/oauth-private.key"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
            std::fs::set_permissions(
                root.join("storage/oauth-public.key"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        let mut app = Application::configure(root.clone())
            .withConfig(config)
            .withProviders(vec![
                Box::new(device_authorization::Options {
                    enabled,
                    register,
                    prefix: prefix.map(str::to_owned),
                }),
                Box::new(PassportServiceProvider),
            ])
            .withMigrations(MIGRATIONS)
            .withRouting(Routing::default().web(routes))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let passport = app.make::<Passport>().await.unwrap();
        passport.authorizationView(view);
        passport.deviceAuthorizationView(view);
        passport.deviceUserCodeView(device_authorization::code_view);
        passport
            .deviceCodeGrantEnabled
            .store(enabled, Ordering::Relaxed);
        if let Some(prefix) = prefix {
            app.container()
                .get::<dyn openrust::contracts::config::Repository>("config")
                .unwrap()
                .set("passport.path", json!(prefix))
                .unwrap();
        }
        passport.tokensCan(BTreeMap::from([
            ("read".into(), "Read data".into()),
            ("write".into(), "Write data".into()),
        ]));
        app.make::<AuthManager>()
            .await
            .unwrap()
            .registerModel::<User>("OAuthUser");
        let encryption = app.make::<openrust::encryption::Encrypter>().await.unwrap();
        let db = app
            .make::<DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        app.make::<Migrator>()
            .await
            .unwrap()
            .run(None, &[], RunOptions::default())
            .await
            .unwrap();
        db.getSchemaBuilder()
            .create("users", |t| {
                t.id();
                t.bigInteger("subject").unique();
                t.string("email", None);
                t.string("password", None);
            })
            .await
            .unwrap();
        let (public, confidential, secret) = app
            .call(
                openrust::container::Callback::new(&[], |_| async {
                    let hash = Hash::make("password", &Default::default()).await.unwrap();
                    for (id, subject, email) in [(1, 77, "alice@test"), (2, 88, "bob@test")] {
                        let mut user = User::newInstance(Attributes::new()).unwrap();
                        user.forceFill(Attributes::from([
                            ("id".into(), Value::Integer(id)),
                            ("subject".into(), Value::Integer(subject)),
                            ("email".into(), email.into()),
                            ("password".into(), hash.clone().into()),
                        ]))
                        .unwrap();
                        user.save().await.unwrap();
                    }
                    let repo = openrust::container::Container::getInstance()
                        .make::<ClientRepository>()
                        .unwrap();
                    let public = repo
                        .createAuthorizationCodeGrantClient(
                            "Public",
                            &[CALLBACK.into()],
                            false,
                            None,
                            false,
                        )
                        .await
                        .unwrap();
                    let private = repo
                        .createAuthorizationCodeGrantClient(
                            "Confidential",
                            &[CALLBACK.into()],
                            true,
                            None,
                            false,
                        )
                        .await
                        .unwrap();
                    let id = |v: Value| match v {
                        Value::Text(v) => v,
                        _ => panic!("UUID expected"),
                    };
                    let secret = private
                        .model::<openrust_passport::Client>()
                        .unwrap()
                        .lock()
                        .await
                        .plainSecret
                        .as_ref()
                        .unwrap()
                        .to_string();
                    Ok((
                        id(public.getKey().await.unwrap()),
                        id(private.getKey().await.unwrap()),
                        secret,
                    ))
                }),
                Default::default(),
            )
            .await
            .unwrap();
        let veto = Arc::new(std::sync::atomic::AtomicBool::new(false));
        SelectedToken::observe(&app, SaveGate(veto.clone())).unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let kernel = Arc::new(kernel);
        let transport_kernel = kernel.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(transport_kernel).into_make_service(),
            )
            .await
            .unwrap();
        });
        Self {
            root,
            base,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            db,
            passport,
            encryption,
            veto,
            server,
            kernel,
            public,
            confidential,
            secret,
        }
    }
    async fn wire(
        &self,
        method: reqwest::Method,
        path: &str,
        cookie: &str,
        body: Option<J>,
    ) -> reqwest::Response {
        let mut r = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("Accept", "application/json");
        if !cookie.is_empty() {
            r = r.header("Cookie", cookie);
        }
        if let Some(body) = body {
            r = r
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }
        r.send().await.unwrap()
    }
    async fn login(&self, email: &str) -> Browser {
        let initial = self.wire(reqwest::Method::GET, "/session", "", None).await;
        assert_eq!(initial.status(), 200);
        let cookie = session_cookie(&initial);
        let data = json_body(initial).await;
        let response = self
            .wire(
                reqwest::Method::POST,
                "/login",
                &cookie,
                Some(json!({"email":email,"password":"password","_token":data["csrf"]})),
            )
            .await;
        assert_eq!(response.status(), 200);
        let cookie = session_cookie(&response);
        let data = json_body(response).await;
        Browser {
            cookie,
            csrf: data["csrf"].as_str().unwrap().to_owned(),
        }
    }
    async fn authorize(&self, b: &Browser, id: &str, pkce: bool) -> J {
        let mut pairs = vec![
            ("response_type", "code"),
            ("client_id", id),
            ("redirect_uri", CALLBACK),
            ("scope", "read"),
            ("state", "opaque & state"),
            ("prompt", "consent"),
        ];
        if pkce {
            pairs.extend([
                ("code_challenge", CHALLENGE),
                ("code_challenge_method", "S256"),
            ]);
        }
        let response = self
            .wire(
                reqwest::Method::GET,
                &format!(
                    "/oauth/authorize?{}",
                    url::form_urlencoded::Serializer::new(String::new())
                        .extend_pairs(pairs)
                        .finish()
                ),
                &b.cookie,
                None,
            )
            .await;
        let status = response.status();
        let data = json_body(response).await;
        assert_eq!(status, 200, "{data}");
        assert_eq!(data["user"], 77);
        assert!(data["client"].get("secret").is_none());
        data
    }
    async fn approve(&self, b: &Browser, view: &J) -> String {
        let response = self
            .wire(
                reqwest::Method::POST,
                "/oauth/authorize",
                &b.cookie,
                Some(json!({"auth_token":view["auth_token"],"_token":b.csrf})),
            )
            .await;
        assert_eq!(response.status(), 302, "{}", response.status());
        let location = response.headers()["location"].to_str().unwrap();
        let url = url::Url::parse(location).unwrap();
        assert_eq!(
            url.query_pairs().find(|(k, _)| k == "existing").unwrap().1,
            "1"
        );
        assert_eq!(
            url.query_pairs().find(|(k, _)| k == "state").unwrap().1,
            "opaque & state"
        );
        url.query_pairs()
            .find(|(k, _)| k == "code")
            .unwrap()
            .1
            .into_owned()
    }
    async fn token(
        &self,
        id: &str,
        code: &str,
        verifier: Option<&str>,
        secret: Option<&str>,
    ) -> reqwest::Response {
        let mut data = json!({"grant_type":"authorization_code","client_id":id,"code":code,"redirect_uri":CALLBACK});
        if let Some(value) = verifier {
            data["code_verifier"] = json!(value);
        }
        if let Some(value) = secret {
            data["client_secret"] = json!(value);
        }
        self.wire(reqwest::Method::POST, "/oauth/token", "", Some(data))
            .await
    }
    async fn cleanup(&self) {
        for table in [
            "oauth_device_codes",
            "oauth_refresh_tokens",
            "oauth_access_tokens",
            "oauth_auth_codes",
            "oauth_clients",
            "users",
            "history",
        ] {
            self.db
                .getSchemaBuilder()
                .dropIfExists(table)
                .await
                .unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
struct Browser {
    cookie: String,
    csrf: String,
}
fn session_cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().split(';').next().unwrap())
        .find(|v| v.starts_with("oauth_session="))
        .unwrap()
        .to_owned()
}
async fn json_body(response: reqwest::Response) -> J {
    let bytes = response.bytes().await.unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("Non-JSON response: {}", String::from_utf8_lossy(&bytes)))
}
fn required() -> bool {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_some() {
        true
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        eprintln!("SKIP: MySQL URL absent");
        false
    }
}

#[tokio::test]
async fn mysql_real_session_consent_pkce_confidential_and_single_use_code() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    let missing = f.token(&f.public, &code, None, None).await;
    assert_eq!(missing.status(), 400);
    assert_eq!(json_body(missing).await["error"], "invalid_request");
    let wrong = f.token(&f.public, &code, Some(&"a".repeat(43)), None).await;
    assert_eq!(wrong.status(), 400);
    assert_eq!(json_body(wrong).await["error"], "invalid_grant");
    let response = f.token(&f.public, &code, Some(VERIFIER), None).await;
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let data = json_body(response).await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(data["token_type"], "Bearer");
    assert!(data["refresh_token"].as_str().is_some());
    let jwt = data["access_token"].as_str().unwrap();
    let parts: Vec<_> = jwt.split('.').collect();
    let claims: J = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["sub"], "77");
    assert_eq!(claims["aud"], f.public);
    assert_eq!(claims["scopes"], json!(["read"]));
    {
        use rsa::{pkcs8::DecodePublicKey as _, traits::PublicKeyParts as _};
        let key = rsa::RsaPublicKey::from_public_key_pem(include_str!("fixtures/oauth-public.pem"))
            .unwrap();
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        aws_lc_rs::signature::RsaPublicKeyComponents {
            n: key.n().to_bytes_be(),
            e: key.e().to_bytes_be(),
        }
        .verify(
            &aws_lc_rs::signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &signature,
        )
        .unwrap();
    }
    let replay = f.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(replay.status(), 400);
    assert_eq!(json_body(replay).await["error"], "invalid_grant");
    let view = f.authorize(&b, &f.confidential, false).await;
    let code = f.approve(&b, &view).await;
    let wrong = f.token(&f.confidential, &code, None, Some("wrong")).await;
    assert_eq!(wrong.status(), 401);
    assert_eq!(json_body(wrong).await["error"], "invalid_client");
    let response = f.token(&f.confidential, &code, None, Some(&f.secret)).await;
    let status = response.status();
    let data = json_body(response).await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        2
    );
    assert_eq!(
        f.db.table("oauth_auth_codes")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        0
    );
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_concurrent_exchange_has_one_winner_and_failure_rolls_back() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    let (a, b) = tokio::join!(
        f.token(&f.public, &code, Some(VERIFIER), None),
        f.token(&f.public, &code, Some(VERIFIER), None)
    );
    let statuses = [a.status().as_u16(), b.status().as_u16()];
    assert!(
        statuses == [200, 400] || statuses == [400, 200],
        "{statuses:?}"
    );
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        1
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        1
    );
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_rejects_bad_redirect_pkce_consent_csrf_and_cross_session() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let guest=f.wire(reqwest::Method::GET,&format!("/oauth/authorize?response_type=code&client_id={}&prompt=none&code_challenge={CHALLENGE}",f.public),"",None).await;
    assert_eq!(guest.status(), 302);
    assert!(
        guest.headers()["location"]
            .to_str()
            .unwrap()
            .contains("error=login_required")
    );
    let b = f.login("alice@test").await;
    f.passport.setDefaultScope(vec!["read".into()]);
    let bad=f.wire(reqwest::Method::GET,&format!("/oauth/authorize?response_type=code&client_id={}&redirect_uri=https%3A%2F%2Fevil.test&code_challenge={CHALLENGE}",f.public),&b.cookie,None).await;
    assert_eq!(bad.status(), 401);
    assert!(bad.headers().get("location").is_none());
    let bad = f
        .wire(
            reqwest::Method::GET,
            &format!("/oauth/authorize?response_type=code&client_id={}", f.public),
            &b.cookie,
            None,
        )
        .await;
    assert_eq!(bad.status(), 400);
    assert_eq!(json_body(bad).await["error"], "invalid_request");
    let none=f.wire(reqwest::Method::GET,&format!("/oauth/authorize?response_type=code&client_id={}&prompt=none&code_challenge={CHALLENGE}",f.public),&b.cookie,None).await;
    assert_eq!(none.status(), 302);
    assert!(
        none.headers()["location"]
            .to_str()
            .unwrap()
            .contains("error=consent_required")
    );
    let view = f.authorize(&b, &f.public, true).await;
    let csrf = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token":view["auth_token"]})),
        )
        .await;
    assert_eq!(csrf.status(), 419);
    let other = f.login("bob@test").await;
    let cross = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &other.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":other.csrf})),
        )
        .await;
    assert_eq!(cross.status(), 400);
    let wrong = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token":"incorrect","_token":b.csrf})),
        )
        .await;
    assert_eq!(wrong.status(), 400);
    let replay = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":b.csrf})),
        )
        .await;
    assert_eq!(replay.status(), 400);
    let view = f.authorize(&b, &f.public, true).await;
    let deny=f.wire(reqwest::Method::DELETE,"/oauth/authorize",&b.cookie,Some(json!({"auth_token":view["auth_token"],"_token":b.csrf,"redirect_uri":"https://evil.test"}))).await;
    assert_eq!(deny.status(), 302);
    let location = deny.headers()["location"].to_str().unwrap();
    assert!(location.starts_with(CALLBACK));
    assert!(location.contains("error=access_denied"));
    assert!(!location.contains("evil.test"));
    assert_eq!(f.db.table("oauth_auth_codes").count("*").await.unwrap(), 0);
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_code_bindings_revocation_and_database_failure_never_issue_tokens() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    let other = f
        .token(&f.confidential, &code, Some(VERIFIER), Some(&f.secret))
        .await;
    assert_eq!(other.status(), 400);
    assert_eq!(json_body(other).await["error"], "invalid_grant");
    let redirect=f.wire(reqwest::Method::POST,"/oauth/token","",Some(json!({"grant_type":"authorization_code","client_id":f.public,"code":code,"code_verifier":VERIFIER,"redirect_uri":"https://evil.test"}))).await;
    assert_eq!(redirect.status(), 400);
    assert_eq!(json_body(redirect).await["error"], "invalid_request");
    let missing=f.wire(reqwest::Method::POST,"/oauth/token","",Some(json!({"grant_type":"authorization_code","client_id":f.public,"code":code,"code_verifier":VERIFIER}))).await;
    assert_eq!(missing.status(), 400);
    let tampered = f
        .token(&f.public, &format!("{code}tamper"), Some(VERIFIER), None)
        .await;
    assert_eq!(tampered.status(), 400);
    // Force an actual SQL failure after access-token save. The transaction must retain the code and roll back the token.
    f.db.getSchemaBuilder()
        .dropIfExists("oauth_refresh_tokens")
        .await
        .unwrap();
    let failed = f.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(failed.status(), 500);
    assert_eq!(json_body(failed).await["error"], "server_error");
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        f.db.table("oauth_auth_codes")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    // Restore the exact four production columns for the injected-failure fixture, then retry the same code.
    f.db.getSchemaBuilder()
        .create("oauth_refresh_tokens", |t| {
            t.char("id", 80).primary();
            t.char("access_token_id", 80).index();
            t.boolean("revoked");
            t.dateTime("expires_at", None).nullable();
        })
        .await
        .unwrap();
    let success = f.token(&f.public, &code, Some(VERIFIER), None).await;
    let status = success.status();
    let data = json_body(success).await;
    assert_eq!(status, 200, "{data}");
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    f.db.table("oauth_clients")
        .r#where("id", "=", f.public.clone())
        .update(Attributes::from([("revoked".into(), true.into())]))
        .await
        .unwrap();
    let revoked = f.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(revoked.status(), 401);
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_form_basic_auth_and_previously_granted_consent() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.confidential, false).await;
    let code = f.approve(&b, &view).await;
    let wrong = f
        .client
        .post(format!("{}/oauth/token", f.base))
        .header("authorization", "Basic YmFkOmJhZA==")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("redirect_uri", CALLBACK),
                ])
                .finish(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    assert_eq!(wrong.headers()["www-authenticate"], "Basic realm=\"OAuth\"");
    let response = f
        .client
        .post(format!("{}/oauth/token", f.base))
        .header(
            "authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", f.confidential, f.secret))
            ),
        )
        .header("content-type", "application/x-www-form-urlencoded")
        .body(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("redirect_uri", CALLBACK),
                ])
                .finish(),
        )
        .send()
        .await
        .unwrap();
    let status = response.status();
    let data = json_body(response).await;
    assert_eq!(status, 200, "{data}");
    let none = f
        .wire(
            reqwest::Method::GET,
            &format!(
                "/oauth/authorize?response_type=code&client_id={}&scope=read&prompt=none",
                f.confidential
            ),
            &b.cookie,
            None,
        )
        .await;
    assert_eq!(none.status(), 302);
    assert!(
        none.headers()["location"]
            .to_str()
            .unwrap()
            .contains("code=")
    );
    assert!(
        !none.headers()["location"]
            .to_str()
            .unwrap()
            .contains("error=")
    );
    let expanded = f
        .wire(
            reqwest::Method::GET,
            &format!(
                "/oauth/authorize?response_type=code&client_id={}&scope=write&prompt=none",
                f.confidential
            ),
            &b.cookie,
            None,
        )
        .await;
    assert_eq!(expanded.status(), 302);
    assert!(
        expanded.headers()["location"]
            .to_str()
            .unwrap()
            .contains("error=consent_required")
    );
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_expired_envelope_and_other_application_reject_even_matching_rows_and_key() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    // Both apps deliberately share APP_KEY, user subjects, scopes and RSA keys. Mirror the actual rows as well.
    for table in ["oauth_clients", "oauth_auth_codes"] {
        let row = if table == "oauth_clients" {
            f.db.table(table)
                .r#where("id", "=", f.public.clone())
                .first()
                .await
                .unwrap()
                .unwrap()
        } else {
            f.db.table(table).first().await.unwrap().unwrap()
        };
        other
            .db
            .table(table)
            .insert(&[row.columns().iter().cloned().collect()])
            .await
            .unwrap();
    }
    let cross = other.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(cross.status(), 400);
    assert_eq!(json_body(cross).await["error"], "invalid_grant");
    assert_eq!(
        other
            .db
            .table("oauth_access_tokens")
            .count("*")
            .await
            .unwrap(),
        0
    );
    // The test authority backdates a correctly authenticated envelope; no wall-clock sleep or forged unauthenticated payload.
    let plaintext = zeroize::Zeroizing::new(f.encryption.decryptString(&code).unwrap());
    let mut payload: J = serde_json::from_slice(&plaintext).unwrap();
    payload["expire_time"] = json!(1);
    let expired = f
        .encryption
        .encryptString(payload.to_string().as_bytes())
        .unwrap();
    let response = f.token(&f.public, &expired, Some(VERIFIER), None).await;
    assert_eq!(response.status(), 400);
    let data = json_body(response).await;
    assert_eq!(data["error"], "invalid_grant");
    assert_eq!(data["hint"], "Authorization code has expired");
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    let success = f.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(success.status(), 200);
    f.cleanup().await;
    other.cleanup().await;
}

#[tokio::test]
async fn mysql_consent_rechecks_changed_redirect_and_revoked_client() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    f.db.table("oauth_clients")
        .r#where("id", "=", f.public.clone())
        .update(Attributes::from([(
            "redirect_uris".into(),
            Value::Json(serde_json::json!(["https://changed.test/callback"])),
        )]))
        .await
        .unwrap();
    let response = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token": view["auth_token"], "_token": b.csrf})),
        )
        .await;
    assert_eq!(response.status(), 401);
    assert!(response.headers().get("location").is_none());
    assert_eq!(f.db.table("oauth_auth_codes").count("*").await.unwrap(), 0);
    let view = f.authorize(&b, &f.confidential, false).await;
    f.db.table("oauth_clients")
        .r#where("id", "=", f.confidential.clone())
        .update(Attributes::from([("revoked".into(), true.into())]))
        .await
        .unwrap();
    let response = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &b.cookie,
            Some(json!({"auth_token": view["auth_token"], "_token": b.csrf})),
        )
        .await;
    assert_eq!(response.status(), 401);
    assert!(response.headers().get("location").is_none());
    assert_eq!(f.db.table("oauth_auth_codes").count("*").await.unwrap(), 0);
    f.cleanup().await;
}

struct SelectedToken(ModelState);
impl Model for SelectedToken {
    fn metadata() -> &'static ModelMetadata {
        openrust_passport::Token::metadata()
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        openrust_passport::Token::casts()
    }
    fn getKeyType(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
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
impl openrust_passport::TokenModel for SelectedToken {}
struct SaveGate(Arc<std::sync::atomic::AtomicBool>);
impl openrust::database::eloquent::Observer<SelectedToken> for SaveGate {
    const EVENTS: &'static [openrust::database::eloquent::concerns::ModelEvent] =
        &[openrust::database::eloquent::concerns::ModelEvent::Saving];
    fn saving<'a>(
        &'a self,
        _: &'a mut SelectedToken,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            Ok(if self.0.load(Ordering::Relaxed) {
                openrust::database::eloquent::concerns::ModelEventResponse::Cancel
            } else {
                openrust::database::eloquent::concerns::ModelEventResponse::Continue
            })
        })
    }
}
#[tokio::test]
async fn mysql_selected_token_model_event_veto_rolls_back_and_retry_uses_same_code() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    f.passport.useTokenModel::<SelectedToken>();
    f.veto.store(true, Ordering::Relaxed);
    let b = f.login("alice@test").await;
    let view = f.authorize(&b, &f.public, true).await;
    let code = f.approve(&b, &view).await;
    let response = f.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(response.status(), 500);
    assert_eq!(json_body(response).await["error"], "server_error");
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        f.db.table("oauth_auth_codes")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    f.veto.store(false, Ordering::Relaxed);
    assert_eq!(
        f.token(&f.public, &code, Some(VERIFIER), None)
            .await
            .status(),
        200
    );
    let response = f.wire(reqwest::Method::GET, &format!("/oauth/authorize?response_type=code&client_id={}&scope=read&prompt=none&code_challenge={CHALLENGE}", f.public), &b.cookie, None).await;
    assert_eq!(response.status(), 302);
    assert!(
        response.headers()["location"]
            .to_str()
            .unwrap()
            .contains("code=")
    );
    f.cleanup().await;
}

fn resource(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    resource_for(request, "api")
}
fn resource_other(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    resource_for(request, "other")
}
fn resource_for<'a>(
    request: &'a mut Request,
    guard: &'static str,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request).await?.unwrap();
        let token = openrust_passport::guards::TokenGuard::token(request, guard).unwrap();
        Response::json(
            StatusCode::OK,
            json!({"user":user.get_auth_identifier(),"token":token.oauth_access_token_id,"scopes":token.oauth_scopes,"read":token.can("read").unwrap(),"write":token.can("write").unwrap(),"transient":token.transient()}),
        )
    })
}
fn revoke_resource(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let token = openrust_passport::guards::TokenGuard::token(request, "api")
            .unwrap()
            .clone();
        assert!(token.revoke().await.unwrap());
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
impl Fixture {
    async fn issued(&self) -> J {
        let b = self.login("alice@test").await;
        let view = self.authorize(&b, &self.public, true).await;
        let code = self.approve(&b, &view).await;
        let response = self.token(&self.public, &code, Some(VERIFIER), None).await;
        assert_eq!(response.status(), 200);
        json_body(response).await
    }
    async fn resource_token(&self, path: &str, token: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap()
    }
    async fn refresh(&self, token: &str, scope: Option<&str>) -> reqwest::Response {
        let mut body =
            json!({"grant_type":"refresh_token","client_id":self.public,"refresh_token":token});
        if let Some(scope) = scope {
            body["scope"] = json!(scope);
        }
        self.wire(reqwest::Method::POST, "/oauth/token", "", Some(body))
            .await
    }
    async fn purge(&self, args: &[&str]) -> (i32, openrust::console::Output) {
        let args = args
            .iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        self.kernel
            .get_application()
            .call(
                openrust::container::Callback::new(&[], move |_| {
                    let args = args.clone();
                    async move {
                        let mut output = openrust::console::Output::default();
                        let result = openrust::support::facades::Artisan::call(
                            "passport:purge",
                            &args,
                            &mut output,
                        )
                        .await;
                        Ok((
                            result.unwrap_or_else(|error| {
                                output.stderr.push_str(&error.to_string());
                                1
                            }),
                            output,
                        ))
                    }
                }),
                Default::default(),
            )
            .await
            .unwrap()
    }
}
fn jwt_claims(token: &str) -> J {
    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap()
}
// Trusted test authority signs altered claims; this does not simulate an attacker forging RSA.
fn sign_claims(claims: &J) -> String {
    use rsa::{
        pkcs1::DecodeRsaPrivateKey as _,
        pkcs8::{DecodePrivateKey as _, EncodePrivateKey as _},
    };
    let key = rsa::RsaPrivateKey::from_pkcs8_pem(include_str!("fixtures/oauth-private.pem"))
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(include_str!("fixtures/oauth-private.pem")))
        .unwrap()
        .to_pkcs8_der()
        .unwrap();
    let key = aws_lc_rs::signature::RsaKeyPair::from_pkcs8(key.as_bytes()).unwrap();
    let message = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap())
    );
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &aws_lc_rs::signature::RSA_PKCS1_SHA256,
        &aws_lc_rs::rand::SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
}
#[tokio::test]
async fn mysql_refresh_rotates_credentials_and_guard_uses_real_user_and_scopes() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    f.passport
        .tokensExpireIn(Some(openrust_passport::GrantLifetime::Seconds(60)));
    f.passport
        .refreshTokensExpireIn(Some(openrust_passport::GrantLifetime::Seconds(180)));
    let data = f.issued().await;
    assert_eq!(data["expires_in"], 60);
    let old = data["access_token"].as_str().unwrap();
    let resource = json_body(f.resource_token("/resource", old).await).await;
    assert_eq!(resource["user"], 77);
    assert_eq!(resource["read"], true);
    assert_eq!(resource["write"], false);
    assert_eq!(resource["transient"], false);
    let claims = jwt_claims(old);
    assert_eq!(
        claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
        60
    );
    let plaintext = zeroize::Zeroizing::new(
        f.encryption
            .decryptString(data["refresh_token"].as_str().unwrap())
            .unwrap(),
    );
    let refresh: J = serde_json::from_slice(&plaintext).unwrap();
    assert_eq!(
        refresh["expire_time"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
        180
    );
    let expanded = f
        .refresh(data["refresh_token"].as_str().unwrap(), Some("write"))
        .await;
    assert_eq!(expanded.status(), 400);
    assert_eq!(json_body(expanded).await["error"], "invalid_scope");
    let response = f
        .refresh(data["refresh_token"].as_str().unwrap(), None)
        .await;
    let status = response.status();
    let next = json_body(response).await;
    assert_eq!(status, 200, "{next}");
    assert_ne!(data["refresh_token"], next["refresh_token"]);
    assert_eq!(f.resource_token("/resource", old).await.status(), 401);
    assert_eq!(
        f.resource_token("/resource", next["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        f.refresh(data["refresh_token"].as_str().unwrap(), None)
            .await
            .status(),
        400
    );
    let narrow = f
        .refresh(next["refresh_token"].as_str().unwrap(), Some(""))
        .await;
    assert_eq!(narrow.status(), 200);
    let narrow = json_body(narrow).await;
    assert_eq!(
        jwt_claims(narrow["access_token"].as_str().unwrap())["scopes"],
        json!(["read"])
    );
    assert_eq!(
        f.db.table("oauth_access_tokens")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_refresh_competition_and_event_failure_leave_atomic_database_state() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let data = f.issued().await;
    let refresh = data["refresh_token"].as_str().unwrap();
    f.passport.useTokenModel::<SelectedToken>();
    f.veto.store(true, Ordering::Relaxed);
    assert_eq!(f.refresh(refresh, None).await.status(), 500);
    assert_eq!(
        f.resource_token("/resource", data["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    f.veto.store(false, Ordering::Relaxed);
    let (a, b) = tokio::join!(f.refresh(refresh, None), f.refresh(refresh, None));
    let statuses = [a.status().as_u16(), b.status().as_u16()];
    assert!(
        statuses == [200, 400] || statuses == [400, 200],
        "{statuses:?}"
    );
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        2
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        2
    );
    assert_eq!(
        f.db.table("oauth_access_tokens")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens")
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_refresh_rejects_wrong_client_expiry_corruption_and_other_application() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let data = f.issued().await;
    let refresh = data["refresh_token"].as_str().unwrap();
    let wrong=f.wire(reqwest::Method::POST,"/oauth/token","",Some(json!({"grant_type":"refresh_token","client_id":f.confidential,"client_secret":f.secret,"refresh_token":refresh}))).await;
    assert_eq!(wrong.status(), 400);
    assert_eq!(
        json_body(wrong).await["hint"],
        "Token is not linked to client"
    );
    let row =
        f.db.table("oauth_clients")
            .r#where("id", "=", f.public.clone())
            .first()
            .await
            .unwrap()
            .unwrap();
    other
        .db
        .table("oauth_clients")
        .insert(&[row.columns().iter().cloned().collect()])
        .await
        .unwrap();
    let cross = other
        .wire(
            reqwest::Method::POST,
            "/oauth/token",
            "",
            Some(
                json!({"grant_type":"refresh_token","client_id":f.public,"refresh_token":refresh}),
            ),
        )
        .await;
    assert_eq!(cross.status(), 400);
    let plaintext = zeroize::Zeroizing::new(f.encryption.decryptString(refresh).unwrap());
    let mut payload: J = serde_json::from_slice(&plaintext).unwrap();
    payload["expire_time"] = json!(1);
    let expired = f
        .encryption
        .encryptString(payload.to_string().as_bytes())
        .unwrap();
    let response = f.refresh(&expired, None).await;
    assert_eq!(response.status(), 400);
    assert_eq!(json_body(response).await["hint"], "Token has expired");
    assert_eq!(
        f.refresh(&format!("{refresh}tamper"), None).await.status(),
        400
    );
    assert_eq!(f.refresh(refresh, None).await.status(), 200);
    assert_eq!(
        other
            .db
            .table("oauth_access_tokens")
            .count("*")
            .await
            .unwrap(),
        0
    );
    f.cleanup().await;
    other.cleanup().await;
}
#[tokio::test]
async fn mysql_resource_guard_checks_signatures_dates_provider_client_user_and_failures() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let data = f.issued().await;
    let token = data["access_token"].as_str().unwrap();
    let original = jwt_claims(token);
    assert_eq!(f.resource_token("/resource", token).await.status(), 200);
    assert_eq!(f.resource_token("/resource", "invalid").await.status(), 401);
    let mut bytes = token.as_bytes().to_vec();
    let index = token.rfind('.').unwrap() + 4;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    assert_eq!(
        f.resource_token("/resource", std::str::from_utf8(&bytes).unwrap())
            .await
            .status(),
        401
    );
    let mut none = token.split('.').map(str::to_owned).collect::<Vec<_>>();
    none[0] = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    assert_eq!(
        f.resource_token("/resource", &none.join("."))
            .await
            .status(),
        401
    );
    for (name, value) in [
        ("iat", original["exp"].clone()),
        ("nbf", original["exp"].clone()),
        ("exp", json!(1)),
        ("exp", json!(null)),
        ("exp", json!("NaN")),
        ("exp", json!("invalid")),
        ("exp", json!(1e300)),
        ("exp", json!("9223372036854775808")),
    ] {
        let mut claims = original.clone();
        claims[name] = value;
        assert_eq!(
            f.resource_token("/resource", &sign_claims(&claims))
                .await
                .status(),
            401,
            "{name}"
        );
    }
    let mut loose = original.clone();
    for key in ["iat", "nbf", "exp"] {
        loose.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(
        f.resource_token("/resource", &sign_claims(&loose))
            .await
            .status(),
        200
    );
    let mut multi = original.clone();
    multi["aud"] = json!([f.public, "other-audience"]);
    assert_eq!(
        f.resource_token("/resource", &sign_claims(&multi))
            .await
            .status(),
        200
    );
    for subject in ["077", "+77", "77.0", "77e0", "77junk"] {
        let mut altered = original.clone();
        altered["sub"] = json!(subject);
        assert_eq!(
            f.resource_token("/resource", &sign_claims(&altered))
                .await
                .status(),
            401,
            "{subject}"
        );
    }
    let mut missing = original.clone();
    missing["sub"] = json!("999999");
    assert_eq!(
        f.resource_token("/resource", &sign_claims(&missing))
            .await
            .status(),
        401
    );
    f.db.table("oauth_clients")
        .r#where("id", "=", f.public.clone())
        .update(Attributes::from([("provider".into(), "users".into())]))
        .await
        .unwrap();
    assert_eq!(
        f.resource_token("/resource-other", token).await.status(),
        401
    );
    assert_eq!(f.resource_token("/resource", token).await.status(), 200);
    f.db.table("users")
        .r#where("subject", "=", 77_i64)
        .delete()
        .await
        .unwrap();
    assert_eq!(f.resource_token("/resource", token).await.status(), 401);
    f.db.getSchemaBuilder()
        .rename("users", "hidden_users")
        .await
        .unwrap();
    assert_eq!(f.resource_token("/resource", token).await.status(), 401);
    f.db.getSchemaBuilder()
        .rename("hidden_users", "users")
        .await
        .unwrap();
    f.db.getSchemaBuilder()
        .dropIfExists("oauth_access_tokens")
        .await
        .unwrap();
    assert_eq!(f.resource_token("/resource", token).await.status(), 500);
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_access_revocation_and_refresh_revocation_have_distinct_lifecycles() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let data = f.issued().await;
    let response = f
        .client
        .delete(format!("{}/resource", f.base))
        .bearer_auth(data["access_token"].as_str().unwrap())
        .header("X-CSRF-TOKEN", f.login("alice@test").await.csrf)
        .send()
        .await
        .unwrap();
    // This route is in the web group; bearer identity does not waive its CSRF check.
    assert_eq!(response.status(), 419);
    let browser = f.login("alice@test").await;
    let response = f
        .client
        .delete(format!("{}/resource", f.base))
        .bearer_auth(data["access_token"].as_str().unwrap())
        .header("Cookie", &browser.cookie)
        .header("X-CSRF-TOKEN", &browser.csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    assert_eq!(
        f.resource_token("/resource", data["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    let next = json_body(
        f.refresh(data["refresh_token"].as_str().unwrap(), None)
            .await,
    )
    .await;
    let plain = zeroize::Zeroizing::new(
        f.encryption
            .decryptString(next["refresh_token"].as_str().unwrap())
            .unwrap(),
    );
    let payload: J = serde_json::from_slice(&plain).unwrap();
    let mut refresh = openrust_passport::RefreshToken::queryOn(&f.db)
        .find(payload["refresh_token_id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    openrust_passport::RefreshTokenModel::revoke(&mut refresh)
        .await
        .unwrap();
    assert_eq!(
        f.refresh(next["refresh_token"].as_str().unwrap(), None)
            .await
            .status(),
        400
    );
    assert_eq!(
        f.resource_token("/resource", next["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    f.db.table("oauth_clients")
        .r#where("id", "=", f.public.clone())
        .update(Attributes::from([("revoked".into(), true.into())]))
        .await
        .unwrap();
    assert_eq!(
        f.resource_token("/resource", next["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_purge_preserves_active_and_unbounded_refreshes_and_honors_flags() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    for (id, revoked, expires) in [
        ("active-refresh", true, "2000-01-01 00:00:00"),
        ("null-refresh", false, "2000-01-01 00:00:00"),
        ("expired", false, "2000-01-01 00:00:00"),
        ("revoked-future", true, "2090-01-01 00:00:00"),
        ("valid", false, "2090-01-01 00:00:00"),
    ] {
        f.db.table("oauth_access_tokens")
            .insert(&[Attributes::from([
                ("id".into(), id.into()),
                ("client_id".into(), f.public.clone().into()),
                ("user_id".into(), 77_i64.into()),
                ("scopes".into(), "[]".into()),
                ("revoked".into(), revoked.into()),
                ("expires_at".into(), expires.into()),
            ])])
            .await
            .unwrap();
    }
    for (id, access, revoked, expires) in [
        (
            "refresh1",
            "active-refresh",
            false,
            Some("2090-01-01 00:00:00"),
        ),
        ("refresh2", "null-refresh", false, None),
        ("refresh3", "expired", false, Some("2000-01-01 00:00:00")),
    ] {
        f.db.table("oauth_refresh_tokens")
            .insert(&[Attributes::from([
                ("id".into(), id.into()),
                ("access_token_id".into(), access.into()),
                ("revoked".into(), revoked.into()),
                ("expires_at".into(), expires.map_or(Value::Null, Into::into)),
            ])])
            .await
            .unwrap();
    }
    let rows = (0..1001)
        .map(|i| {
            Attributes::from([
                ("id".into(), format!("batch-{i:05}").into()),
                ("client_id".into(), f.public.clone().into()),
                ("user_id".into(), 77_i64.into()),
                ("scopes".into(), "[]".into()),
                ("revoked".into(), true.into()),
                ("expires_at".into(), "2090-01-01 00:00:00".into()),
            ])
        })
        .collect::<Vec<_>>();
    f.db.table("oauth_access_tokens")
        .insert(&rows)
        .await
        .unwrap();
    f.db.table("oauth_auth_codes")
        .insert(&[Attributes::from([
            ("id".into(), "purge-code".into()),
            ("client_id".into(), f.public.clone().into()),
            ("user_id".into(), 77_i64.into()),
            ("scopes".into(), "[]".into()),
            ("revoked".into(), true.into()),
            ("expires_at".into(), "2090-01-01 00:00:00".into()),
        ])])
        .await
        .unwrap();
    f.db.table("oauth_device_codes")
        .insert(&[Attributes::from([
            ("id".into(), "purge-device".into()),
            ("user_code".into(), "USER0001".into()),
            ("client_id".into(), f.public.clone().into()),
            ("user_id".into(), 77_i64.into()),
            ("scopes".into(), "[]".into()),
            ("revoked".into(), true.into()),
            ("expires_at".into(), "2090-01-01 00:00:00".into()),
        ])])
        .await
        .unwrap();
    f.passport
        .deviceCodeGrantEnabled
        .store(false, Ordering::Relaxed);
    let (status, output) = f.purge(&["--revoked"]).await;
    assert_eq!(status, 0, "{}", output.stderr);
    assert!(output.stdout.contains("Purged revoked items."));
    assert_eq!(f.db.table("oauth_auth_codes").count("*").await.unwrap(), 0);
    assert_eq!(
        f.db.table("oauth_device_codes").count("*").await.unwrap(),
        1
    );
    f.passport
        .deviceCodeGrantEnabled
        .store(true, Ordering::Relaxed);
    assert_eq!(
        f.db.table("oauth_access_tokens")
            .r#where("id", "=", "active-refresh")
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.db.table("oauth_access_tokens")
            .r#where("id", "=", "revoked-future")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        f.db.table("oauth_access_tokens")
            .r#where("id", "=", "expired")
            .count("*")
            .await
            .unwrap(),
        1
    );
    let (status, output) = f.purge(&["--expired", "--hours=0"]).await;
    assert_eq!(status, 0, "{}", output.stderr);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        3
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        2
    );
    let (status, output) = f.purge(&[]).await;
    assert_eq!(status, 0, "{}", output.stderr);
    assert!(output.stdout.contains("1 week"));
    assert_eq!(
        f.db.table("oauth_device_codes").count("*").await.unwrap(),
        0
    );
    let (status, output) = f.purge(&["--expired", "--hours=960"]).await;
    assert_eq!(status, 0);
    assert!(output.stdout.contains("1 month"), "{}", output.stdout);
    let (status, output) = f.purge(&["--expired", "--hours=9600"]).await;
    assert_eq!(status, 0);
    assert!(output.stdout.contains("1 year"), "{}", output.stdout);
    let (status, _) = f.purge(&["--hours=invalid"]).await;
    assert_ne!(status, 0);
    let (status, _) = f.purge(&["--revoked", "--hours=invalid"]).await;
    assert_eq!(status, 0);
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_authorization_wildcard_is_recognized_and_removed_before_code_token_issuance() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let response=f.wire(reqwest::Method::GET,&format!("/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope=%2A&state=opaque%20%26%20state&code_challenge={CHALLENGE}&code_challenge_method=S256",f.public,url::form_urlencoded::byte_serialize(CALLBACK.as_bytes()).collect::<String>()),&b.cookie,None).await;
    assert_eq!(response.status(), 200);
    let view = json_body(response).await;
    assert_eq!(view["scopes"], json!([]));
    let code = f.approve(&b, &view).await;
    let response = f.token(&f.public, &code, Some(VERIFIER), None).await;
    assert_eq!(response.status(), 200);
    let data = json_body(response).await;
    assert_eq!(
        jwt_claims(data["access_token"].as_str().unwrap())["scopes"],
        json!([])
    );
    let response=f.wire(reqwest::Method::GET,&format!("/oauth/authorize?response_type=code&client_id={}&scope=%2A&prompt=none&code_challenge={CHALLENGE}",f.public),&b.cookie,None).await;
    assert_eq!(response.status(), 302);
    assert!(
        response.headers()["location"]
            .to_str()
            .unwrap()
            .contains("code=")
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_expired_access_interval_does_not_expire_its_independent_refresh() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    f.passport
        .tokensExpireIn(Some(openrust_passport::GrantLifetime::Interval(
            openrust::support::DateInterval {
                microseconds: -1_000_000,
                ..Default::default()
            },
        )));
    assert_eq!(
        other.passport.tokensExpireIn(None),
        openrust_passport::GrantLifetime::CalendarYears(1)
    );
    let data = f.issued().await;
    assert_eq!(data["expires_in"], -1);
    assert_eq!(
        f.resource_token("/resource", data["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    f.passport
        .tokensExpireIn(Some(openrust_passport::GrantLifetime::Seconds(60)));
    let response = f
        .refresh(data["refresh_token"].as_str().unwrap(), None)
        .await;
    assert_eq!(response.status(), 200);
    let next = json_body(response).await;
    assert_eq!(
        f.resource_token("/resource", next["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    f.cleanup().await;
    other.cleanup().await;
}

struct AlternateAccess(ModelState);
impl Model for AlternateAccess {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("custom_access")
            .incrementing(false)
            .guarded(&[])
            .timestamps(None, None);
        &META
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        openrust_passport::Token::casts()
    }
    fn getKeyType(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
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
impl openrust_passport::TokenModel for AlternateAccess {}

struct AlternateRefresh(ModelState);
impl Model for AlternateRefresh {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("custom_refresh")
            .incrementing(false)
            .guarded(&[])
            .timestamps(None, None);
        &META
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        openrust_passport::RefreshToken::casts()
    }
    fn getKeyType(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
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
impl openrust_passport::RefreshTokenModel for AlternateRefresh {}

#[tokio::test]
async fn mysql_selected_tables_support_refresh_resource_and_purge_in_application_timezone() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let config = f
        .kernel
        .get_application()
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    config.set("app.timezone", json!("Asia/Shanghai")).unwrap();
    f.db.getSchemaBuilder()
        .rename("oauth_access_tokens", "custom_access")
        .await
        .unwrap();
    f.db.getSchemaBuilder()
        .rename("oauth_refresh_tokens", "custom_refresh")
        .await
        .unwrap();
    f.passport.useTokenModel::<AlternateAccess>();
    f.passport.useRefreshTokenModel::<AlternateRefresh>();
    let data = f.issued().await;
    let token = data["access_token"].as_str().unwrap();
    assert_eq!(f.resource_token("/resource", token).await.status(), 200);
    let claims = jwt_claims(token);
    let expected = chrono::DateTime::from_timestamp(claims["exp"].as_i64().unwrap(), 0)
        .unwrap()
        .with_timezone(
            &"Asia/Shanghai"
                .parse::<openrust::support::DateTimezone>()
                .unwrap(),
        )
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let row =
        f.db.table("custom_access")
            .r#where("id", "=", claims["jti"].as_str().unwrap())
            .first()
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.get("expires_at"), Some(&Value::Text(expected)));
    let next = f
        .refresh(data["refresh_token"].as_str().unwrap(), None)
        .await;
    assert_eq!(next.status(), 200);
    let next = json_body(next).await;
    assert_eq!(f.resource_token("/resource", token).await.status(), 401);
    assert_eq!(
        f.resource_token("/resource", next["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(f.purge(&["--revoked"]).await.0, 0);
    assert_eq!(f.db.table("custom_access").count("*").await.unwrap(), 1);
    assert_eq!(f.db.table("custom_refresh").count("*").await.unwrap(), 1);
    f.db.getSchemaBuilder()
        .rename("custom_access", "oauth_access_tokens")
        .await
        .unwrap();
    f.db.getSchemaBuilder()
        .rename("custom_refresh", "oauth_refresh_tokens")
        .await
        .unwrap();
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_refresh_can_narrow_two_granted_scopes_without_expanding_again() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let response=f.wire(reqwest::Method::GET,&format!("/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope=read%20write&state=opaque%20%26%20state&code_challenge={CHALLENGE}&code_challenge_method=S256",f.public,url::form_urlencoded::byte_serialize(CALLBACK.as_bytes()).collect::<String>()),&b.cookie,None).await;
    assert_eq!(response.status(), 200);
    let view = json_body(response).await;
    let code = f.approve(&b, &view).await;
    let data = json_body(f.token(&f.public, &code, Some(VERIFIER), None).await).await;
    assert_eq!(
        jwt_claims(data["access_token"].as_str().unwrap())["scopes"],
        json!(["read", "write"])
    );
    let next = f
        .refresh(data["refresh_token"].as_str().unwrap(), Some("read"))
        .await;
    assert_eq!(next.status(), 200);
    let next = json_body(next).await;
    assert_eq!(
        jwt_claims(next["access_token"].as_str().unwrap())["scopes"],
        json!(["read"])
    );
    assert_eq!(
        f.refresh(next["refresh_token"].as_str().unwrap(), Some("read write"))
            .await
            .status(),
        400
    );
    assert_eq!(
        f.refresh(next["refresh_token"].as_str().unwrap(), None)
            .await
            .status(),
        200
    );
    f.cleanup().await;
}

#[path = "device_authorization/mod.rs"]
mod device_authorization;

#[path = "default_grants/mod.rs"]
mod default_grants;

use axum::{extract::State, handler::Handler as _};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::{
    auth::{AuthIdentifier, AuthManager, Credentials},
    config::{ConfigError, Environment, Value as Json},
    contracts::{
        auth::{Authenticatable, StatefulGuard as _},
        container::Container as _,
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
            json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"OAuthUser"}}}),
        ),
        (
            "hashing".into(),
            json!({"driver":"bcrypt","bcrypt":{"rounds":4}}),
        ),
    ]))
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/session", session)?;
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
    public: String,
    confidential: String,
    secret: String,
}
impl Fixture {
    async fn new() -> Self {
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
            .withProviders(vec![Box::new(PassportServiceProvider)])
            .withMigrations(MIGRATIONS)
            .withRouting(Routing::default().web(routes))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let passport = app.make::<Passport>().await.unwrap();
        passport.authorizationView(view);
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
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
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

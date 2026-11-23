#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
//! Controlled OP fixture and two native RP applications; this is not a production OP.
#[path = "support/local_login.rs"]
mod recipe;
#[path = "support/local_logout.rs"]
mod recipe_logout;
#[path = "support/user.rs"]
mod user;
use axum::{
    Router,
    extract::State,
    handler::Handler as _,
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, jwk::JwkSet,
};
use openrust::{
    auth::{Auth, AuthManager},
    contracts::{container::Container as _, http::Kernel as _, session::Session as _},
    database::DatabaseManager,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Route,
};
use openrust_oidc::{OidcServiceProvider, facades::Oidc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
type Fields = BTreeMap<String, String>;
fn fields(q: &str) -> Fields {
    url::form_urlencoded::parse(q.as_bytes())
        .into_owned()
        .collect()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn random() -> String {
    let mut b = [0u8; 32];
    aws_lc_rs::rand::fill(&mut b).unwrap();
    URL_SAFE_NO_PAD.encode(b)
}
fn hash(v: &str) -> String {
    URL_SAFE_NO_PAD
        .encode(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, v.as_bytes()).as_ref())
}
fn cookie(headers: &http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|s| s.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, v)| v.to_owned())
}
#[derive(Clone)]
struct Registration {
    redirect: String,
    post_logout: String,
    secret: String,
}
struct Code {
    fields: Fields,
    auth_time: u64,
    subject: String,
}
struct Op {
    base: String,
    clients: Mutex<HashMap<String, Registration>>,
    sessions: Mutex<HashMap<String, u64>>,
    codes: Mutex<HashMap<String, Code>>,
    tokens: Mutex<Vec<(String, String)>>,
    mode: Mutex<String>,
    signins: AtomicUsize,
    logouts: AtomicUsize,
}
async fn discovery(State(op): State<Arc<Op>>) -> axum::response::Response {
    let b = &op.base;
    let mode = op.mode.lock().unwrap().clone();
    if mode == "unavailable" {
        return (StatusCode::SERVICE_UNAVAILABLE, "offline").into_response();
    }
    let mut v = json!({"issuer":b,"authorization_endpoint":format!("{b}/authorize"),"token_endpoint":format!("{b}/token"),"jwks_uri":format!("{b}/jwks"),"userinfo_endpoint":format!("{b}/userinfo"),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["client_secret_post"],"end_session_endpoint":format!("{b}/end-session")});
    match mode.as_str() {
        "no-logout" => {
            v.as_object_mut().unwrap().remove("end_session_endpoint");
        }
        "logout-query" => {
            v["end_session_endpoint"] = json!(format!("{b}/end-session?state=reserved"))
        }
        _ => {}
    }
    axum::Json(v).into_response()
}
fn issue(op: &Op, p: Fields, auth_time: u64) -> axum::response::Response {
    let Some(client) = op.clients.lock().unwrap().get(&p["client_id"]).cloned() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if p.get("redirect_uri") != Some(&client.redirect)
        || p.get("code_challenge_method").map(String::as_str) != Some("S256")
        || p.get("response_type").map(String::as_str) != Some("code")
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let code = random();
    let state = p["state"].clone();
    let subject = if *op.mode.lock().unwrap() == "other-subject" {
        "other"
    } else {
        "007"
    }
    .to_owned();
    op.codes.lock().unwrap().insert(
        code.clone(),
        Code {
            fields: p,
            auth_time,
            subject,
        },
    );
    let mut url = url::Url::parse(&client.redirect).unwrap();
    url.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &state)
        .append_pair("iss", &op.base);
    (StatusCode::FOUND, [("location", url.to_string())]).into_response()
}
async fn authorize(
    State(op): State<Arc<Op>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let q = request.uri().query().unwrap();
    let p = fields(q);
    let authenticated = cookie(request.headers(), "op_session")
        .and_then(|id| op.sessions.lock().unwrap().get(&id).copied());
    if p.get("prompt").map(String::as_str) != Some("login")
        && let Some(time) = authenticated
    {
        return issue(&op, p, time);
    }
    if p.get("prompt").map(String::as_str) == Some("none") {
        let registration = op.clients.lock().unwrap()[&p["client_id"]].clone();
        let mut url = url::Url::parse(&registration.redirect).unwrap();
        url.query_pairs_mut()
            .append_pair("error", "login_required")
            .append_pair("state", &p["state"]);
        return (StatusCode::FOUND, [("location", url.to_string())]).into_response();
    }
    let continuation = URL_SAFE_NO_PAD.encode(q.as_bytes());
    axum::response::Html(format!("<!doctype html><title>Test identity provider</title><form method=post action=/signin><input type=hidden name=continuation value='{continuation}'><label>Username<input name=username></label><label>Password<input type=password name=password></label><button>Sign in</button></form>")).into_response()
}
async fn signin(
    State(op): State<Arc<Op>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let p = fields(std::str::from_utf8(&body).unwrap());
    if p.get("username").map(String::as_str) != Some("alice")
        || p.get("password").map(String::as_str) != Some("correct")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(q) = URL_SAFE_NO_PAD.decode(&p["continuation"]) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(q) = std::str::from_utf8(&q) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = random();
    let time = now();
    op.sessions.lock().unwrap().insert(id.clone(), time);
    op.signins.fetch_add(1, Ordering::SeqCst);
    let mut response = issue(&op, fields(q), time);
    response.headers_mut().insert(
        http::header::SET_COOKIE,
        http::HeaderValue::from_str(&format!("op_session={id}; Path=/; HttpOnly; SameSite=Lax"))
            .unwrap(),
    );
    response
}
async fn token(
    State(op): State<Arc<Op>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let p = fields(std::str::from_utf8(&body).unwrap());
    let Some(client) = op.clients.lock().unwrap().get(&p["client_id"]).cloned() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(code) = op.codes.lock().unwrap().remove(&p["code"]) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if p["client_secret"] != client.secret
        || p["redirect_uri"] != client.redirect
        || p["client_id"] != code.fields["client_id"]
        || hash(&p["code_verifier"]) != code.fields["code_challenge"]
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let access = random();
    op.tokens
        .lock()
        .unwrap()
        .push((access.clone(), code.subject.clone()));
    let t = now();
    let mut claims = json!({"iss":op.base,"sub":code.subject,"aud":p["client_id"],"iat":t,"exp":t+300,"nonce":code.fields["nonce"],"auth_time":code.auth_time,"email":"shared@test","email_verified":true});
    match op.mode.lock().unwrap().as_str() {
        "other-issuer" => claims["iss"] = json!(format!("{}/other-tenant", op.base)),
        "old-auth" => claims["auth_time"] = json!(t - 100),
        _ => {}
    }
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("test-key".into());
    let jwt = encode(
        &h,
        &claims,
        &EncodingKey::from_rsa_der(include_bytes!("fixtures/private.der")),
    )
    .unwrap();
    axum::Json(json!({"access_token":access,"id_token":jwt,"token_type":"Bearer"})).into_response()
}
async fn jwks() -> axum::Json<Value> {
    axum::Json(
        serde_json::from_str::<Value>(include_str!("fixtures/keys.json")).unwrap()["keys"].clone(),
    )
}
async fn userinfo(State(op): State<Arc<Op>>, headers: http::HeaderMap) -> axum::response::Response {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let subject = op
        .tokens
        .lock()
        .unwrap()
        .iter()
        .find(|(t, _)| Some(t.as_str()) == token)
        .map(|(_, s)| s.clone());
    match subject {
        Some(sub) => axum::Json(json!({"sub":sub,"email":"shared@test","email_verified":true}))
            .into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}
async fn end_session(
    State(op): State<Arc<Op>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let p = fields(request.uri().query().unwrap());
    let Some(client) = op.clients.lock().unwrap().get(&p["client_id"]).cloned() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if p.get("post_logout_redirect_uri") != Some(&client.post_logout) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let keys: JwkSet = serde_json::from_value(
        serde_json::from_str::<Value>(include_str!("fixtures/keys.json")).unwrap()["keys"].clone(),
    )
    .unwrap();
    let key = DecodingKey::from_jwk(&keys.keys[0]).unwrap();
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[&op.base]);
    validation.set_audience(&[&p["client_id"]]);
    validation.validate_exp = false;
    if decode::<Value>(&p["id_token_hint"], &key, &validation).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if let Some(id) = cookie(request.headers(), "op_session") {
        op.sessions.lock().unwrap().remove(&id);
    }
    op.logouts.fetch_add(1, Ordering::SeqCst);
    let mut url = url::Url::parse(&client.post_logout).unwrap();
    url.query_pairs_mut().append_pair("state", &p["state"]);
    (
        StatusCode::FOUND,
        [
            ("location", url.to_string()),
            (
                "set-cookie",
                "op_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".into(),
            ),
        ],
    )
        .into_response()
}
async fn control(State(op): State<Arc<Op>>, request: axum::extract::Request) -> axum::Json<Value> {
    let p = fields(request.uri().query().unwrap_or(""));
    if let Some(mode) = p.get("mode") {
        *op.mode.lock().unwrap() = mode.clone();
    }
    if p.get("revoke").map(String::as_str) == Some("all") {
        op.tokens.lock().unwrap().clear();
    }
    axum::Json(
        json!({"signins":op.signins.load(Ordering::SeqCst),"logouts":op.logouts.load(Ordering::SeqCst),"last_token":op.tokens.lock().unwrap().last().map(|(t,_)|t.clone())}),
    )
}
async fn op() -> (Arc<Op>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let state = Arc::new(Op {
        base: format!("http://{}", listener.local_addr().unwrap()),
        clients: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
        codes: Mutex::new(HashMap::new()),
        tokens: Mutex::new(Vec::new()),
        mode: Mutex::new(String::new()),
        signins: AtomicUsize::new(0),
        logouts: AtomicUsize::new(0),
    });
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/authorize", get(authorize))
        .route("/signin", post(signin))
        .route("/token", post(token))
        .route("/jwks", get(jwks))
        .route("/userinfo", get(userinfo))
        .route("/end-session", get(end_session))
        .route("/fixture/control", get(control))
        .with_state(state.clone());
    (
        state,
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}
fn internal(e: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(e),
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/login", login)?;
    Route::get("/callback", callback)?;
    Route::get("/who", who)?;
    Route::get("/signed-out", signed_out)?;
    Route::post("/logout/local", local_logout)?;
    Route::post("/logout/federated", federated_logout)?;
    Ok(())
}
fn login(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let force = r.input(Some("force")).await?.is_some();
        let silent = r.input(Some("silent")).await?.is_some();
        let fresh = r.input(Some("fresh")).await?.is_some();
        let mut p = Oidc::provider("work", r).map_err(internal)?;
        if force {
            p.prompt("login").max_age(0);
        }
        if silent {
            p.prompt("none");
        }
        if fresh {
            p.max_age(60);
        }
        Ok(p.redirect().await.map_err(internal)?.into())
    })
}
fn callback(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let result = Oidc::provider("work", r).map_err(internal)?.user().await;
        match result {
            Ok(mut a) => {
                if !recipe::login_bound_user(r, &a.identity).await? {
                    return Response::json(StatusCode::FORBIDDEN, json!({"error":"unbound"}));
                }
                a.tokens.id_token.clear(); // Public token fields cannot forge the captured verified hint.
                Oidc::provider("work", r)
                    .map_err(internal)?
                    .remember_logout(&a)
                    .map_err(internal)?;
                Response::json(StatusCode::OK, json!({"authenticated":true}))
            }
            Err(_) => Response::json(StatusCode::BAD_REQUEST, json!({"error":"callback"})),
        }
    })
}
fn who(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(r).await.map_err(internal)?;
        Response::json(
            StatusCode::OK,
            json!({"user":user.map(|u|u.get_auth_identifier().to_string()),"csrf":r.session()?.token()}),
        )
    })
}
fn local_logout(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    logout(r, false)
}
fn federated_logout(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    logout(r, true)
}
fn logout(r: &mut Request, federated: bool) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(recipe_logout::logout(r, federated))
}
struct LogoutRace(tokio::sync::Barrier);
fn signed_out(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if r.input(Some("race")).await?.is_some() {
            openrust::container::Container::get_instance()
                .make::<LogoutRace>()
                .map_err(internal)?
                .0
                .wait()
                .await;
        }
        if let Some(mode) = r.input(Some("alter")).await? {
            let mut attempt = r.session_mut()?.pull("oidc.logout.work").unwrap();
            match mode.as_str() {
                Some("expired") => attempt["issued_at"] = json!(1),
                Some("config") => attempt["config"] = json!("other-client"),
                _ => attempt["redirect"] = json!("https://other.test/exit"),
            }
            r.session_mut()?.put("oidc.logout.work", attempt)?;
        }
        match Oidc::provider("work", r)
            .map_err(internal)?
            .complete_logout()
            .await
        {
            Ok(()) => Response::json(StatusCode::OK, json!({"local":true,"federated":true})),
            Err(_) => Response::json(StatusCode::BAD_REQUEST, json!({"error":"logout-state"})),
        }
    })
}
struct Settings(BTreeMap<String, Value>);
impl openrust::support::ServiceProvider for Settings {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        use openrust::contracts::foundation::Application as _;
        Box::pin(async move {
            let c = app
                .container()
                .get::<dyn openrust::contracts::config::Repository>("config")?;
            for (k, v) in &self.0 {
                c.set(k, v.clone())?;
            }
            Ok(())
        })
    }
}
struct Rp {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
async fn rp(op: &Arc<Op>, mysql: &str, name: &str, user_id: i64) -> Rp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let client = format!("client-{name}");
    let redirect = format!("{base}/callback");
    let post_logout = format!("{base}/signed-out");
    op.clients.lock().unwrap().insert(
        client.clone(),
        Registration {
            redirect: redirect.clone(),
            post_logout: post_logout.clone(),
            secret: format!("secret-{name}"),
        },
    );
    let settings=[("app".into(),json!({"key":name.repeat(32),"env":"testing"})),("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":mysql}}})),("session".into(),json!({"driver":"database","cookie":format!("rp_{name}_session"),"table":"sessions","lottery":[0,100]})),("cache".into(),json!({"default":"database","prefix":format!("sso-{name}:"),"stores":{"database":{"driver":"database","table":"cache"}}})),("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"SsoUser"}}})),("oidc".into(),json!({"providers":{"work":{"issuer":op.base,"client_id":client,"client_secret":format!("secret-{name}"),"client_auth":"client_secret_post","redirect":redirect,"post_logout_redirect":post_logout,"leeway":1,"allow_loopback_http":true}}}))].into();
    let mut app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| Ok(Default::default()))
        .with_providers(vec![
            Box::new(Settings(settings)),
            Box::new(OidcServiceProvider),
        ])
        .with_routing(Routing::default().web(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    {
        use openrust::contracts::foundation::Application as _;
        app.container()
            .instance::<LogoutRace>(Arc::new(LogoutRace(tokio::sync::Barrier::new(2))))
            .unwrap();
    }
    app.make::<AuthManager>()
        .await
        .unwrap()
        .register_model::<user::User>("SsoUser");
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    for sql in [
        "CREATE TABLE sessions(id VARCHAR(255) PRIMARY KEY,user_id VARCHAR(255) NULL,ip_address VARCHAR(45) NULL,user_agent TEXT NULL,payload LONGTEXT NOT NULL,last_activity BIGINT NOT NULL)",
        "CREATE TABLE cache(`key` VARCHAR(255) PRIMARY KEY,value MEDIUMTEXT NOT NULL,expiration BIGINT NOT NULL)",
        "CREATE TABLE users(id BIGINT UNSIGNED PRIMARY KEY,email VARCHAR(255),password VARCHAR(255),remember_token VARCHAR(100) NULL)",
        "CREATE TABLE oidc_identities(issuer VARBINARY(2048) NOT NULL,subject VARBINARY(255) NOT NULL,user_id BIGINT UNSIGNED NOT NULL,UNIQUE(issuer,subject),FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    db.statement(
        "INSERT INTO users VALUES(?,'shared@test','unused',NULL)",
        &[user_id.into()],
    )
    .await
    .unwrap();
    db.statement(
        "INSERT INTO oidc_identities VALUES(?,'007',?)",
        &[op.base.clone().into(), user_id.into()],
    )
    .await
    .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        State(k): State<Arc<Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handle_transport(r).await
    }
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    Rp { base, task }
}
#[tokio::test]
async fn mysql_browser_two_rps_sso_reauthentication_and_logout_boundaries() {
    let Ok(mysql) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let node =
        std::env::var("OIDC_TEST_NODE").expect("OIDC_TEST_NODE required for browser acceptance");
    let mut admin=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}}))].into())).create().unwrap();
    admin.boot().await.unwrap();
    let db = admin
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
    assert!(
        matches!(version.get("version"),Some(openrust::database::Value::Text(v))if v.starts_with("8.4."))
    );
    for schema in ["oidc_sso_a", "oidc_sso_b"] {
        db.statement(&format!("CREATE DATABASE {schema}"), &[])
            .await
            .unwrap();
    }
    let (op, op_task) = op().await;
    let mut a_url = url::Url::parse(&mysql).unwrap();
    a_url.set_path("/oidc_sso_a");
    let mut b_url = a_url.clone();
    b_url.set_path("/oidc_sso_b");
    let a = rp(&op, a_url.as_str(), "a", 101).await;
    let b = rp(&op, b_url.as_str(), "b", 201).await;
    let op_base = op.base.clone();
    let a_base = a.base.clone();
    let b_base = b.base.clone();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/browser/sso.mjs");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(node)
            .arg(script)
            .env("OIDC_OP", op_base)
            .env("OIDC_RP_A", a_base)
            .env("OIDC_RP_B", b_base)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "Browser failure:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    a.task.abort();
    b.task.abort();
    op_task.abort();
    for schema in ["oidc_sso_a", "oidc_sso_b"] {
        db.statement(&format!("DROP DATABASE {schema}"), &[])
            .await
            .unwrap();
    }
}

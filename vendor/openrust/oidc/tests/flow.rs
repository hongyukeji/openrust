#[path = "support/local_login.rs"]
mod recipe;
#[path = "support/user.rs"]
mod user;
use axum::{
    Router,
    extract::State,
    handler::Handler as _,
    response::IntoResponse,
    routing::{get, post},
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openrust::{
    contracts::{container::Container as _, http::Kernel as _, session::Session as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
    support::facades::Route,
};
use openrust_oidc::{OidcManager, OidcServiceProvider, facades::Oidc};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
type Fields = BTreeMap<String, String>;
fn fields(query: &str) -> Fields {
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}
fn hash(v: &str) -> String {
    URL_SAFE_NO_PAD
        .encode(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, v.as_bytes()).as_ref())
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
struct Op {
    base: String,
    mode: Mutex<String>,
    codes: Mutex<BTreeMap<String, Fields>>,
    calls: AtomicUsize,
    rotated: Mutex<bool>,
}
async fn discovery(State(op): State<Arc<Op>>) -> axum::response::Response {
    let b = &op.base;
    let mode = op.mode.lock().unwrap().clone();
    if mode == "discovery-error" {
        return (StatusCode::SERVICE_UNAVAILABLE, "private").into_response();
    }
    let mut v = json!({"issuer":b,"authorization_endpoint":format!("{b}/authorize"),"token_endpoint":format!("{b}/token"),"jwks_uri":format!("{b}/jwks"),"userinfo_endpoint":format!("{b}/userinfo"),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post","none"]});
    match mode.as_str() {
        "discovery-issuer" => v["issuer"] = json!("https://wrong.test"),
        "discovery-origin" => v["token_endpoint"] = json!("https://other.test/token"),
        "discovery-query" => {
            v["authorization_endpoint"] = json!(format!("{b}/authorize?state=duplicate"))
        }
        "discovery-pkce" => v["code_challenge_methods_supported"] = json!(["plain"]),
        _ => {}
    }
    axum::Json(v).into_response()
}
async fn authorize(
    State(op): State<Arc<Op>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let p = fields(request.uri().query().unwrap());
    assert_eq!(p["client_id"], "client:+ ");
    assert_eq!(p["redirect_uri"], "https://rp.test/callback");
    assert_eq!(p["response_type"], "code");
    assert_eq!(p["code_challenge_method"], "S256");
    assert!(p["scope"].split(' ').any(|s| s == "openid"));
    assert_eq!(p["nonce"].len(), 43);
    assert_eq!(p["state"].len(), 43);
    let code = format!("code-{}", p["state"]);
    let state = p["state"].clone();
    op.codes.lock().unwrap().insert(code.clone(), p);
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("code", &code)
        .append_pair("state", &state)
        .append_pair("iss", &op.base)
        .finish();
    (
        StatusCode::FOUND,
        [("location", format!("https://rp.test/callback?{query}"))],
    )
        .into_response()
}
async fn token(
    State(op): State<Arc<Op>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    op.calls.fetch_add(1, Ordering::SeqCst);
    let auth = request
        .headers()
        .get("authorization")
        .map(|h| h.to_str().unwrap().to_owned());
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let p = fields(std::str::from_utf8(&body).unwrap());
    assert_eq!(p["grant_type"], "authorization_code");
    assert_eq!(p["redirect_uri"], "https://rp.test/callback");
    if let Some(auth) = auth {
        assert_eq!(
            auth,
            format!("Basic {}", STANDARD.encode("client%3A%2B+:secret%3A%2B+"))
        );
        assert!(!p.contains_key("client_secret"));
    } else {
        assert_eq!(p["client_id"], "client:+ ");
        if let Some(secret) = p.get("client_secret") {
            assert_eq!(secret, "secret:+ ");
        }
    }
    let Some(attempt) = op.codes.lock().unwrap().remove(&p["code"]) else {
        return (StatusCode::BAD_REQUEST, "used code").into_response();
    };
    assert_eq!(hash(&p["code_verifier"]), attempt["code_challenge"]);
    assert!((43..=128).contains(&p["code_verifier"].len()));
    let mode = op.mode.lock().unwrap().clone();
    if mode == "token-error" {
        return (StatusCode::UNAUTHORIZED, "private-token").into_response();
    }
    if mode == "token-json" {
        return "private-malformed".into_response();
    }
    let t = now();
    let mut c = json!({"iss":op.base,"sub":"007","aud":"client:+ ","exp":t+300,"iat":t,"nonce":attempt["nonce"],"auth_time":t,"email":"old@test","email_verified":true});
    match mode.as_str() {
        "nonce" => c["nonce"] = json!("wrong"),
        "issuer" => c["iss"] = json!("https://wrong.test"),
        "audience" => c["aud"] = json!("other"),
        "expired" => c["exp"] = json!(t - 1),
        "old-auth" => c["auth_time"] = json!(t - 100),
        "numeric" => c["sub"] = json!("7"),
        "unknown" => c["sub"] = json!("0007"),
        _ => {}
    }
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("test-key".into());
    let rotated = *op.rotated.lock().unwrap();
    let pem = if rotated {
        include_bytes!("fixtures/second.der").as_slice()
    } else {
        include_bytes!("fixtures/private.der").as_slice()
    };
    let jwt = encode(&h, &c, &EncodingKey::from_rsa_der(pem)).unwrap();
    axum::Json(json!({"access_token":"access","id_token":jwt,"token_type":"Bearer","refresh_token":"refresh","expires_in":300})).into_response()
}
async fn jwks(State(op): State<Arc<Op>>) -> axum::response::Response {
    if *op.mode.lock().unwrap() == "jwks-error" {
        return (StatusCode::SERVICE_UNAVAILABLE, "private-jwks").into_response();
    }
    let v: Value = serde_json::from_str(include_str!("fixtures/keys.json")).unwrap();
    axum::Json(
        v[if *op.rotated.lock().unwrap() {
            "rotated_keys"
        } else {
            "keys"
        }]
        .clone(),
    )
    .into_response()
}
async fn userinfo(State(op): State<Arc<Op>>, h: http::HeaderMap) -> axum::response::Response {
    assert_eq!(h["authorization"], "Bearer access");
    match op.mode.lock().unwrap().as_str() {
        "userinfo-error" => (StatusCode::UNAUTHORIZED, "private-profile").into_response(),
        "userinfo-sub" => axum::Json(json!({"sub":"other"})).into_response(),
        "numeric" => {
            axum::Json(json!({"sub":"7","email":"old@test","email_verified":true})).into_response()
        }
        "unknown" => axum::Json(json!({"sub":"0007"})).into_response(),
        _ => axum::Json(json!({"sub":"007","email":"new@test"})).into_response(),
    }
}
async fn op() -> (Arc<Op>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let op = Arc::new(Op {
        base: format!("http://{}", listener.local_addr().unwrap()),
        mode: Mutex::new(String::new()),
        codes: Mutex::new(BTreeMap::new()),
        calls: AtomicUsize::new(0),
        rotated: Mutex::new(false),
    });
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .route("/jwks", get(jwks))
        .route("/userinfo", get(userinfo))
        .with_state(op.clone());
    (
        op,
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/login", login)?;
    Route::get("/callback", callback)?;
    Route::get("/alter", alter)?;
    Route::get("/who", who)?;
    Ok(())
}
fn internal(e: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(e),
    }
}
fn login(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        match Oidc::provider("work", r)
            .map_err(internal)?
            .max_age(60)
            .redirect()
            .await
        {
            Ok(v) => Ok(v.into()),
            Err(_) => Response::json(StatusCode::BAD_REQUEST, json!({"error":"login"})),
        }
    })
}
fn alter(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = r.input(Some("mode")).await?.unwrap();
        let mut attempt = r.session()?.get("oidc.attempts.work").unwrap().clone();
        match mode.as_str().unwrap() {
            "expired" => attempt["issued_at"] = json!(1),
            "config" => attempt["config"] = json!("other"),
            _ => attempt["verifier"] = json!("short"),
        }
        r.session_mut()?.put("oidc.attempts.work", attempt)?;
        Ok(Response::no_content())
    })
}
fn who(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let u = openrust::auth::Auth::user(r).await.map_err(internal)?;
        Response::json(
            StatusCode::OK,
            json!({"user":u.map(|u|u.get_auth_identifier().to_string())}),
        )
    })
}
struct BoundLogin;
fn callback(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if let Ok(barrier) = openrust::container::Container::get_instance().make::<ReplayBarrier>()
        {
            barrier.0.wait().await;
        }
        let result = Oidc::provider("work", r).map_err(internal)?.user().await;
        match result {
            Ok(a) => {
                if openrust::container::Container::get_instance()
                    .make::<BoundLogin>()
                    .is_ok()
                    && !recipe::login_bound_user(r, &a.identity).await?
                {
                    return Response::json(StatusCode::FORBIDDEN, json!({"error":"unbound"}));
                }
                Response::json(
                    StatusCode::OK,
                    json!({"issuer":a.identity.issuer(),"subject":a.identity.subject(),"email":a.identity.email(),"verified":a.identity.email_verified(),"refresh":a.tokens.refresh_token.is_some()}),
                )
            }
            Err(e) => Response::json(StatusCode::BAD_REQUEST, json!({"error":e.to_string()})),
        }
    })
}
struct ReplayBarrier(tokio::sync::Barrier);
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
    manager: Arc<OidcManager>,
}
async fn rp(
    op: &str,
    method: &str,
    mysql: Option<&str>,
    barrier: Option<Arc<ReplayBarrier>>,
) -> Rp {
    let mut settings:BTreeMap<String,Value>=[("app".into(),json!({"key":"k".repeat(32),"env":"testing"})),("oidc".into(),json!({"providers":{"work":{"issuer":op,"client_id":"client:+ ","client_secret":"secret:+ ","client_auth":method,"redirect":"https://rp.test/callback","allow_loopback_http":true}}})),("session".into(),json!({"driver":"array","cookie":"oidc_session","lottery":[0,100]})),("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}}))].into();
    if let Some(url) = mysql {
        settings.insert("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"OidcUser"}}}));
        settings.insert(
            "database".into(),
            json!({"default":"mysql","connections":{"mysql":{"url":url}}}),
        );
        settings.insert("session".into(),json!({"driver":"database","table":"sessions","cookie":"oidc_session","lottery":[0,100]}));
        settings.insert("cache".into(),json!({"default":"database","prefix":"oidc-test:","stores":{"database":{"driver":"database","table":"cache"}}}));
    }
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
    if mysql.is_some() {
        use openrust::contracts::foundation::Application as _;
        app.container()
            .instance::<BoundLogin>(Arc::new(BoundLogin))
            .unwrap();
        app.make::<openrust::auth::AuthManager>()
            .await
            .unwrap()
            .register_model::<user::User>("OidcUser");
    }
    let manager = app.make::<OidcManager>().await.unwrap();
    if let Some(barrier) = barrier {
        use openrust::contracts::foundation::Application as _;
        app.container().instance::<ReplayBarrier>(barrier).unwrap();
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        State(k): State<Arc<Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handle_transport(r).await
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
    Rp {
        base,
        task,
        manager,
    }
}
async fn call(base: &str, path: &str, cookie: Option<&str>) -> openrust::http::client::Response {
    let http = Factory::new().unwrap();
    let mut r = http.pending_request().without_redirecting();
    if let Some(c) = cookie {
        r = r.with_headers(http::HeaderMap::from_iter([(
            http::header::COOKIE,
            http::HeaderValue::from_str(c).unwrap(),
        )]));
    }
    r.get(&format!("{base}{path}")).await.unwrap()
}
async fn begin(rp: &Rp) -> (String, String) {
    let response = call(&rp.base, "/login", None).await;
    assert_eq!(response.status(), 302, "{:?}", response.body());
    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("oidc_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let location = response.header("location").unwrap().to_str().unwrap();
    let url = url::Url::parse(location).unwrap();
    let r = Factory::new()
        .unwrap()
        .pending_request()
        .without_redirecting()
        .get(url.as_str())
        .await
        .unwrap();
    assert_eq!(r.status(), 302);
    let callback = url::Url::parse(r.header("location").unwrap().to_str().unwrap()).unwrap();
    (cookie, format!("/callback?{}", callback.query().unwrap()))
}
#[tokio::test]
async fn real_http_code_pkce_session_rotation_and_protocol_failures() {
    let (op, server) = op().await;
    let one = rp(&op.base, "client_secret_basic", None, None).await;
    for rotated in [false, true] {
        *op.rotated.lock().unwrap() = rotated;
        let (c, path) = begin(&one).await;
        let result = call(&one.base, &path, Some(&c)).await;
        assert_eq!(result.status(), 200, "{:?}", result.body());
        let v: Value = result.json().unwrap();
        assert_eq!(v["subject"], "007");
        assert_eq!(v["email"], "new@test");
        assert_eq!(v["verified"], false);
        assert_eq!(v["refresh"], true);
        let count = op.calls.load(Ordering::SeqCst);
        assert_eq!(call(&one.base, &path, Some(&c)).await.status(), 400);
        assert_eq!(op.calls.load(Ordering::SeqCst), count);
    }
    for method in ["none", "client_secret_post"] {
        let app = rp(&op.base, method, None, None).await;
        let (c, path) = begin(&app).await;
        assert_eq!(call(&app.base, &path, Some(&c)).await.status(), 200);
        app.task.abort();
    }
    for mode in [
        "nonce",
        "issuer",
        "audience",
        "expired",
        "old-auth",
        "token-error",
        "token-json",
        "jwks-error",
        "userinfo-error",
        "userinfo-sub",
    ] {
        *op.mode.lock().unwrap() = String::new();
        let (c, path) = begin(&one).await;
        *op.mode.lock().unwrap() = mode.into();
        let r = call(&one.base, &path, Some(&c)).await;
        assert_eq!(r.status(), 400, "{mode}");
        let body = r.body().unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private"));
    }
    for mode in [
        "discovery-error",
        "discovery-issuer",
        "discovery-origin",
        "discovery-query",
        "discovery-pkce",
    ] {
        *op.mode.lock().unwrap() = mode.into();
        assert_eq!(
            call(&one.base, "/login", None).await.status(),
            400,
            "{mode}"
        );
    }
    *op.mode.lock().unwrap() = String::new();
    let (c, path) = begin(&one).await;
    let count = op.calls.load(Ordering::SeqCst);
    let wrong = path.replace("state=", "state=wrong");
    assert_eq!(call(&one.base, &wrong, Some(&c)).await.status(), 400);
    assert_eq!(op.calls.load(Ordering::SeqCst), count);
    for mode in ["expired", "config", "verifier"] {
        let (c, path) = begin(&one).await;
        assert_eq!(
            call(&one.base, &format!("/alter?mode={mode}"), Some(&c))
                .await
                .status(),
            204
        );
        let count = op.calls.load(Ordering::SeqCst);
        assert_eq!(call(&one.base, &path, Some(&c)).await.status(), 400);
        assert_eq!(op.calls.load(Ordering::SeqCst), count);
    }
    let (c, path) = begin(&one).await;
    let two = rp(&op.base, "none", None, None).await;
    assert_eq!(call(&two.base, &path, Some(&c)).await.status(), 400);
    assert_eq!(call(&one.base, &path, Some(&c)).await.status(), 200);
    let (c, path) = begin(&one).await;
    let p = fields(path.split_once('?').unwrap().1);
    let denial = format!(
        "/callback?state={}&error=access_denied&error_description=private",
        p["state"]
    );
    let count = op.calls.load(Ordering::SeqCst);
    assert_eq!(call(&one.base, &denial, Some(&c)).await.status(), 400);
    assert_eq!(op.calls.load(Ordering::SeqCst), count);
    // Invalid names never select nested configuration.
    let (parts, body) = http::Request::new(openrust::http::Body::empty()).into_parts();
    let mut r = Request::from_parts(parts, body);
    assert!(one.manager.provider("work.bad", &mut r).is_err());
    one.task.abort();
    two.task.abort();
    server.abort();
}
#[tokio::test]
async fn mysql_shared_session_two_replicas_atomically_claim_callback() {
    use openrust::database::DatabaseManager;
    let Ok(mysql) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let mut admin=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([("database".into(),json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}}))].into())).create().unwrap();
    admin.boot().await.unwrap();
    let db = admin
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let row = db
        .select_one("SELECT VERSION() AS version", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(row.get("version"),Some(openrust::database::Value::Text(v)) if v.starts_with("8.4."))
    );
    for sql in [
        "CREATE TABLE sessions(id VARCHAR(255) PRIMARY KEY,user_id VARCHAR(255) NULL,ip_address VARCHAR(45) NULL,user_agent TEXT NULL,payload LONGTEXT NOT NULL,last_activity BIGINT NOT NULL)",
        "CREATE TABLE cache(`key` VARCHAR(255) PRIMARY KEY,value MEDIUMTEXT NOT NULL,expiration BIGINT NOT NULL)",
        "CREATE TABLE users(id BIGINT UNSIGNED PRIMARY KEY,email VARCHAR(255),password VARCHAR(255),remember_token VARCHAR(100) NULL)",
        "CREATE TABLE oidc_identities(issuer VARBINARY(2048) NOT NULL,subject VARBINARY(255) NOT NULL,user_id BIGINT UNSIGNED NOT NULL,UNIQUE(issuer,subject),FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE)",
        "INSERT INTO users VALUES(101,'old@test','unused',NULL),(102,'new@test','unused',NULL)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let (op, server) = op().await;
    db.statement(
        "INSERT INTO oidc_identities(issuer,subject,user_id) VALUES(?,?,101)",
        &[op.base.clone().into(), "007".into()],
    )
    .await
    .unwrap();
    let barrier = Arc::new(ReplayBarrier(tokio::sync::Barrier::new(2)));
    let one = rp(
        &op.base,
        "client_secret_basic",
        Some(&mysql),
        Some(barrier.clone()),
    )
    .await;
    let two = rp(&op.base, "client_secret_basic", Some(&mysql), Some(barrier)).await;
    let (c, path) = begin(&one).await;
    let (a, b) = tokio::join!(
        call(&one.base, &path, Some(&c)),
        call(&two.base, &path, Some(&c))
    );
    let mut statuses = [a.status().as_u16(), b.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 400]);
    assert_eq!(op.calls.load(Ordering::SeqCst), 1);
    let success = if a.status() == 200 { &a } else { &b };
    let cookie = success
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("oidc_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let who: Value = call(&two.base, "/who", Some(cookie)).await.json().unwrap();
    assert_eq!(who["user"], "101");
    let old: Value = call(&one.base, "/who", Some(&c)).await.json().unwrap();
    assert_eq!(old["user"], Value::Null);
    // Exact subject matching never merges by the verified email or numeric resemblance.
    for mode in ["numeric", "unknown"] {
        *op.mode.lock().unwrap() = mode.into();
        let (c1, p1) = begin(&one).await;
        let (c2, p2) = begin(&two).await;
        let (a, b) = tokio::join!(
            call(&one.base, &p1, Some(&c1)),
            call(&two.base, &p2, Some(&c2))
        );
        assert_eq!(a.status(), 403);
        assert_eq!(b.status(), 403);
    }
    let count = db
        .select_one("SELECT COUNT(*) AS n FROM users", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("n"),
        Some(openrust::database::Value::Integer(2) | openrust::database::Value::Unsigned(2))
    ));
    one.task.abort();
    two.task.abort();
    server.abort();
    for table in ["oidc_identities", "users", "sessions", "cache"] {
        db.statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

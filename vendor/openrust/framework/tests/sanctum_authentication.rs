#![cfg(feature = "sanctum")]

use std::sync::{Arc, Mutex};

use axum::{extract::State, handler::Handler as _};
use chrono::{TimeDelta, Utc};
use openrust::{
    auth::{AuthIdentifier, AuthManager},
    contracts::{
        auth::{Authenticatable, StatefulGuard as _},
        config::Repository,
        container::Container as _,
        foundation::Application as _,
        http::{Kernel as _, Middleware},
    },
    database::{
        Connection, DatabaseFuture, DatabaseManager, DatabaseResult, Value as SqlValue,
        eloquent::{
            Attributes, Builder, Model, ModelMetadata, ModelState, Scope,
            casts::{Attribute, AttributeCast},
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::{MorphMap, MorphMapEntry},
        },
    },
    events::Dispatcher,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    sanctum::{AccessToken, Sanctum, TokenAuthenticated},
    support::facades::{Auth, Route},
};
use reqwest::{Client, header};
use serde_json::{Value, json};
use tokio::net::TcpListener;

struct User<const SERVICE: bool = false>(ModelState);

struct CustomToken(ModelState);
impl Model for CustomToken {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("custom_sanctum_tokens").connection("token_fixture");
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("last_used_at", AttributeCast::Timestamp)]
    }

    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "name" => Some(Attribute::make(
                Some(|value, _| {
                    let Some(SqlValue::Text(name)) = value else {
                        panic!("expected token name")
                    };
                    Ok(name.strip_prefix("stored:").unwrap().into())
                }),
                Some(|value, _| {
                    let SqlValue::Text(name) = value else {
                        panic!("expected token name")
                    };
                    Ok(Attributes::from([(
                        "name".into(),
                        format!("stored:{name}").into(),
                    )]))
                }),
            )),
            "abilities" => Some(Attribute::get(|value, _| {
                let mut abilities: Vec<String> = match value {
                    Some(SqlValue::Text(value)) => serde_json::from_str(value).unwrap(),
                    Some(SqlValue::Json(value)) => serde_json::from_value(value.clone()).unwrap(),
                    _ => panic!("expected serialized token abilities"),
                };
                if abilities.iter().any(|ability| ability == "read") {
                    abilities.push("write".into());
                }
                Ok(SqlValue::Json(json!(abilities)))
            })),
            _ => None,
        }
    }

    #[allow(non_snake_case, reason = "Implements the Eloquent scope hook")]
    fn globalScopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("visible", &VisibleToken)]
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

struct VisibleToken;

impl Scope<CustomToken> for VisibleToken {
    fn apply(&self, query: &mut Builder<'_, CustomToken>) -> DatabaseResult<()> {
        query.r#where("name", "!=", "stored:hidden");
        Ok(())
    }
}

#[derive(Clone, Default)]
struct TokenEvents(Arc<Mutex<Vec<ModelEvent>>>);

impl TokenEvents {
    fn register(&self, app: &Application) {
        use ModelEvent::*;
        for event in [
            Saving, Creating, Created, Saved, Retrieved, Updating, Updated, Deleting, Deleted,
        ] {
            CustomToken::registerModelEvent(app, event, self.clone()).unwrap();
        }
    }

    fn take(&self) -> Vec<ModelEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl ModelEventListener<CustomToken> for TokenEvents {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        token: &'a mut CustomToken,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(event);
            let name = token.getAttribute("name");
            if (event == ModelEvent::Creating && name == Some(&SqlValue::from("stored:cancel")))
                || (event == ModelEvent::Deleting && name == Some(&SqlValue::from("stored:read")))
            {
                return Ok(ModelEventResponse::Cancel);
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}

struct CustomCsrf;

impl Middleware for CustomCsrf {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.method() == Method::POST && !request.headers().contains_key("x-custom-csrf")
            {
                return Ok(Response::empty(StatusCode::IM_A_TEAPOT));
            }
            Ok(next.run(request).await)
        })
    }
}

impl<const SERVICE: bool> Model for User<SERVICE> {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("sanctum_users").timestamps(None, None);
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

impl<const SERVICE: bool> Authenticatable for User<SERVICE> {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.getKey().unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        match self.getAttribute("password") {
            Some(openrust::database::Value::Text(password)) => Some(password),
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

fn api() -> Result<(), RoutingError> {
    Route::group(
        RouteGroup::new().middleware_name("auth:restricted"),
        |router| {
            router.get("/restricted", whoami)?;
            Ok(())
        },
    )?;
    Route::group(
        RouteGroup::new().middleware_name("auth:sanctum"),
        |router| {
            router.get("/user", whoami)?;
            router
                .get("/all", whoami)?
                .middleware_name("abilities:read,write");
            router
                .get("/any", whoami)?
                .middleware_name("ability:write,read");
            router.r#match(&[Method::POST], "/user", whoami)?;
            Ok(())
        },
    )
}

fn web() -> Result<(), RoutingError> {
    Route::r#match(&[Method::POST], "/login", login)
}

fn login(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut guard = Auth::sessionGuard(request, Some("web"))?;
        guard
            .login_using_id(&AuthIdentifier::UnsignedInteger(2), false)
            .await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn whoami(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let token = match AccessToken::currentAccessToken(request) {
            Some(AccessToken::Personal(token)) => json!(token.id),
            Some(AccessToken::Transient) => json!("session"),
            None => Value::Null,
        };
        Response::json(
            StatusCode::OK,
            json!({"user":user.get_auth_identifier(),"token":token}),
        )
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(app: Application) -> (String, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}"), server)
}

fn cookie(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .find(|line| line.starts_with(&format!("{name}=")))
        .unwrap()
        .to_owned()
}

fn value(cookie: &str) -> &str {
    cookie.split(';').next().unwrap().split_once('=').unwrap().1
}

fn xsrf_header(cookie: &str) -> String {
    percent_encoding::percent_decode_str(value(cookie))
        .decode_utf8()
        .unwrap()
        .into_owned()
}

async fn verify(database: Value, stateful: bool, custom_csrf: bool) {
    let root = tempfile::tempdir().unwrap();
    if custom_csrf {
        std::fs::write(root.path().join("custom-csrf"), "").unwrap();
    }
    let mut app = Application::configure(root.path().to_path_buf())
        .withConfig(|root, _| Ok([
            ("app".into(), json!({"key":"a".repeat(32)})),
            ("session".into(), json!({"driver":"array","cookie":"sanctum_test","lottery":[0,100],"same_site":"strict","http_only":false})),
            ("sanctum".into(), json!({"stateful":format!("spa.example:8443{}", Sanctum::currentRequestHost()),"expiration":5,
                "middleware": if root.join("custom-csrf").is_file() {
                    json!({"validate_csrf_token":"custom-csrf"})
                } else { json!({}) }})),
            ("auth".into(), json!({
                "guards":{"web":{"driver":"session","provider":"users"},"restricted":{"driver":"sanctum","provider":"users"}},
                "providers":{"users":{"driver":"eloquent","model":"SanctumUser"}}
            })),
        ].into()))
        .withMiddleware(move |middleware| {
            if stateful { middleware.statefulApi(); }
            middleware.alias("custom-csrf", Arc::new(CustomCsrf));
        })
        .withRouting(Routing::default().api(api).web(web))
        .create().unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config.set("database.default", json!("fixture")).unwrap();
    config
        .set("database.connections.fixture", database.clone())
        .unwrap();
    let auth = app.container().make::<AuthManager>().unwrap();
    auth.registerModel::<User>("SanctumUser");
    auth.registerModel::<User<true>>("ServiceAccount");
    MorphMap::morphMap(
        &app,
        [
            MorphMapEntry::model::<User>("users_alias"),
            MorphMapEntry::model::<User<true>>("services_alias"),
        ],
        true,
    )
    .unwrap();
    if custom_csrf {
        config
            .set("database.connections.token_fixture", database.clone())
            .unwrap();
    }
    if custom_csrf {
        config.set("app.timezone", json!("Asia/Shanghai")).unwrap();
    }
    let fixture_config = database;
    let database = app.container().make::<DatabaseManager>().unwrap();
    let db = database.connection(None).unwrap();
    let sanctum = app.container().make::<Sanctum>().unwrap();
    let model_events = TokenEvents::default();
    let token_table = if custom_csrf {
        model_events.register(&app);
        sanctum.usePersonalAccessTokenModel::<CustomToken>();
        "custom_sanctum_tokens"
    } else {
        "personal_access_tokens"
    };
    db.statement("DROP TABLE IF EXISTS sanctum_users", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE sanctum_users (id BIGINT PRIMARY KEY, password VARCHAR(255) NOT NULL)",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "INSERT INTO sanctum_users VALUES (1, 'first-hash'), (2, 'old-hash')",
        &[],
    )
    .await
    .unwrap();
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists(token_table).await.unwrap();
    schema
        .create(token_table, |table| {
            table.id();
            table.string("tokenable_type", 255);
            table.unsignedBigInteger("tokenable_id");
            table.text("name");
            table.string("token", 64).unique();
            table.text("abilities");
            table.timestamp("last_used_at", 0).nullable();
            table.timestamp("expires_at", 0).nullable();
            table.timestamps();
        })
        .await
        .unwrap();
    let tokens = sanctum.personalAccessTokens(&database, "").unwrap();
    let read = tokens
        .createToken("users_alias", 1, "read", &["read"], None)
        .await
        .unwrap();
    if custom_csrf {
        use ModelEvent::*;
        assert_eq!(model_events.take(), [Saving, Creating, Created, Saved]);
        assert_eq!(read.access_token.name, "read");
        assert!(read.access_token.can("write"));
        let stored_name = db
            .table(token_table)
            .r#where("id", "=", SqlValue::Unsigned(read.access_token.id))
            .value("name")
            .await
            .unwrap();
        assert_eq!(stored_name, "stored:read".into());
        assert!(
            tokens
                .createToken("users_alias", 1, "cancel", &["read"], None)
                .await
                .is_err()
        );
        assert_eq!(model_events.take(), [Saving, Creating]);
        assert_eq!(db.table(token_table).count("*").await.unwrap(), 1);
    }
    let full = tokens
        .createToken("SanctumUser", 1, "full", &["*"], None)
        .await
        .unwrap();
    let expired = tokens
        .createToken(
            "SanctumUser",
            1,
            "expired",
            &["*"],
            Some(Utc::now() - TimeDelta::seconds(10)),
        )
        .await
        .unwrap();
    let aged = tokens
        .createToken("SanctumUser", 1, "aged", &["*"], None)
        .await
        .unwrap();
    db.statement(
        &format!("UPDATE {token_table} SET created_at = '2020-01-01 00:00:00' WHERE id = ?"),
        &[openrust::database::Value::Unsigned(aged.access_token.id)],
    )
    .await
    .unwrap();
    let wrong_type = tokens
        .createToken("AnotherUser", 1, "wrong", &["*"], None)
        .await
        .unwrap();
    let service = tokens
        .createToken("services_alias", 2, "service", &["*"], None)
        .await
        .unwrap();
    let missing = tokens
        .createToken("SanctumUser", 3, "missing", &["*"], None)
        .await
        .unwrap();

    let events = app
        .container()
        .make::<Dispatcher>()
        .unwrap()
        .fake(&["TokenAuthenticated"]);
    let (base, server) = start(app).await;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(7))
        .build()
        .unwrap();
    let get = |path: &str| {
        client
            .get(format!("{base}{path}"))
            .header(header::ACCEPT, "application/json")
    };
    if custom_csrf {
        use ModelEvent::*;
        model_events.take();
        assert_eq!(
            get("/api/user")
                .bearer_auth(read.plain_text_token.as_str())
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            model_events.take(),
            [Retrieved, Saving, Updating, Updated, Saved]
        );
        assert!(
            tokens
                .findToken(read.plain_text_token.as_str())
                .await
                .unwrap()
                .unwrap()
                .last_used_at
                .is_some()
        );
    }
    for (path, credential, status) in [
        ("/api/user", read.plain_text_token.as_str(), 200),
        ("/api/user", read.plain_text_token.as_str(), 200),
        (
            "/api/all",
            read.plain_text_token.as_str(),
            if custom_csrf { 200 } else { 403 },
        ),
        ("/api/any", read.plain_text_token.as_str(), 200),
        ("/api/all", full.plain_text_token.as_str(), 200),
        ("/api/user", expired.plain_text_token.as_str(), 401),
        ("/api/user", aged.plain_text_token.as_str(), 401),
        ("/api/user", wrong_type.plain_text_token.as_str(), 401),
        ("/api/user", missing.plain_text_token.as_str(), 401),
        ("/api/user", "bad|secret", 401),
        ("/api/user", "1|wrong-secret", 401),
    ] {
        let response = get(path).bearer_auth(credential).send().await.unwrap();
        assert_eq!(response.status().as_u16(), status, "{path}");
        if status == 200 {
            assert_eq!(response.json::<Value>().await.unwrap()["user"], 1);
        }
    }
    assert!(
        tokens
            .findToken(read.plain_text_token.as_str())
            .await
            .unwrap()
            .unwrap()
            .last_used_at
            .is_some()
    );
    assert!(
        tokens
            .findToken(expired.plain_text_token.as_str())
            .await
            .unwrap()
            .unwrap()
            .last_used_at
            .is_none()
    );
    model_events.take();
    assert!(
        tokens
            .revokeToken("SanctumUser", 1, full.access_token.id)
            .await
            .unwrap()
    );
    if custom_csrf {
        use ModelEvent::*;
        assert_eq!(model_events.take(), [Retrieved, Deleting, Deleted]);
        assert!(
            !tokens
                .revokeToken("SanctumUser", 1, full.access_token.id)
                .await
                .unwrap()
        );
        assert!(model_events.take().is_empty());
        assert!(
            !tokens
                .revokeToken("users_alias", 1, read.access_token.id)
                .await
                .unwrap()
        );
        assert_eq!(model_events.take(), [Retrieved, Deleting]);
        assert!(
            tokens
                .findToken(read.plain_text_token.as_str())
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(model_events.take(), [Retrieved]);
    }
    assert_eq!(
        get("/api/user")
            .bearer_auth(full.plain_text_token.as_str())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    events.assertDispatchedTimes("TokenAuthenticated", if custom_csrf { 6 } else { 5 });
    let first = events.dispatched("TokenAuthenticated");
    let first = first[0].downcast_ref::<TokenAuthenticated>().unwrap();
    assert_eq!(first.access_token.id, read.access_token.id);
    assert!(first.access_token.last_used_at.is_none());

    sanctum.getAccessTokenFromRequestUsing(Some(Arc::new(|request| {
        request
            .headers()
            .get("x-access-token")?
            .to_str()
            .ok()
            .map(str::to_owned)
    })));
    sanctum.authenticateAccessTokensUsing(|token, valid| valid && token.name != "service");
    for (token, expected) in [(&read, 200), (&service, 401), (&expired, 401)] {
        assert_eq!(
            get("/api/user")
                .header("x-access-token", token.plain_text_token.as_str())
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert_eq!(
        get("/api/user")
            .bearer_auth(read.plain_text_token.as_str())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    sanctum.getAccessTokenFromRequestUsing(None);
    sanctum.authenticateAccessTokensUsing(|_, valid| valid);

    for (path, credential, expected) in [
        ("/api/user", &service, 200),
        ("/api/restricted", &service, 401),
        ("/api/restricted", &read, 200),
    ] {
        let response = get(path)
            .bearer_auth(credential.plain_text_token.as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["user"],
                credential.access_token.tokenable_id
            );
        }
    }

    sanctum.authenticateAccessTokensUsing(|token, valid| valid || token.name == "expired");
    assert_eq!(
        get("/api/user")
            .bearer_auth(expired.plain_text_token.as_str())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    sanctum.authenticateAccessTokensUsing(|_, valid| valid);

    let csrf = get("/sanctum/csrf-cookie").send().await.unwrap();
    assert_eq!(csrf.status(), 204);
    let xsrf = cookie(&csrf, "XSRF-TOKEN");
    let initial = cookie(&csrf, "sanctum_test");
    let login = client
        .post(format!("{base}/login"))
        .header(header::COOKIE, format!("sanctum_test={}", value(&initial)))
        .header("X-XSRF-TOKEN", xsrf_header(&xsrf))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 204);
    let login_cookie = cookie(&login, "sanctum_test");
    let login_xsrf = cookie(&login, "XSRF-TOKEN");
    let session = format!("sanctum_test={}", value(&login_cookie));
    let origin = "https://spa.example:8443";
    let send = |method: reqwest::Method| {
        client
            .request(method, format!("{base}/api/user"))
            .header(header::COOKIE, &session)
            .header(header::ORIGIN, origin)
    };
    let response = send(reqwest::Method::GET)
        .bearer_auth(read.plain_text_token.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    if stateful {
        let cookie = cookie(&response, "sanctum_test").to_ascii_lowercase();
        assert!(
            cookie.contains("; httponly") && cookie.contains("; samesite=lax"),
            "stateful session cookie must be HttpOnly and SameSite=Lax"
        );
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({"user":2,"token":"session"})
        );
        assert_eq!(
            get("/api/user")
                .header(header::ORIGIN, &base)
                .header(header::COOKIE, &session)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            get("/api/user")
                .header(header::ORIGIN, format!("{base}1"))
                .header(header::COOKIE, &session)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        let (missing_status, header_name, header_value) = if custom_csrf {
            (418, "X-CUSTOM-CSRF", "yes".to_owned())
        } else {
            (419, "X-XSRF-TOKEN", xsrf_header(&login_xsrf))
        };
        assert_eq!(
            send(reqwest::Method::POST).send().await.unwrap().status(),
            missing_status
        );
        assert_eq!(
            send(reqwest::Method::POST)
                .header(header_name, header_value)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            get("/api/all")
                .header(header::ORIGIN, origin)
                .header(header::COOKIE, &session)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        db.statement(
            "UPDATE sanctum_users SET password = 'new-hash' WHERE id = 2",
            &[],
        )
        .await
        .unwrap();
        assert_eq!(
            send(reqwest::Method::GET)
                .bearer_auth(read.plain_text_token.as_str())
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            send(reqwest::Method::GET).send().await.unwrap().status(),
            401
        );
        assert_eq!(
            send(reqwest::Method::GET)
                .bearer_auth(read.plain_text_token.as_str())
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()["user"],
            1
        );
    } else {
        assert_eq!(response.json::<Value>().await.unwrap()["user"], 1);
        assert_eq!(
            send(reqwest::Method::POST).send().await.unwrap().status(),
            401
        );
    }
    for (other, referer) in [
        ("https://spa.example", None),
        (origin, Some("https://spa.example:8443.attacker.test/")),
    ] {
        let request = get("/api/user")
            .header(header::ORIGIN, other)
            .header(header::COOKIE, &session);
        let request = if let Some(referer) = referer {
            request.header(header::REFERER, referer)
        } else {
            request
        };
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            0
        );
    }
    assert_eq!(
        client
            .post(format!("{base}/api/user"))
            .header(header::ORIGIN, "https://unlisted.example")
            .bearer_auth(read.plain_text_token.as_str())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());

    if custom_csrf {
        let hidden = tokens
            .createToken(
                "SanctumUser",
                1,
                "hidden",
                &["*"],
                Some(Utc::now() - TimeDelta::minutes(2)),
            )
            .await
            .unwrap();
        assert!(
            tokens
                .findToken(hidden.plain_text_token.as_str())
                .await
                .unwrap()
                .is_none()
        );
        let prune_events = TokenEvents::default();
        let mut console = Application::configure(root.path().to_path_buf())
            .create()
            .unwrap();
        console.boot().await.unwrap();
        prune_events.register(&console);
        let config = console.container().get::<dyn Repository>("config").unwrap();
        config.set("database.default", json!("unused")).unwrap();
        config.set("app.timezone", json!("Asia/Shanghai")).unwrap();
        config
            .set("database.connections.token_fixture", fixture_config)
            .unwrap();
        console
            .container()
            .make::<Sanctum>()
            .unwrap()
            .usePersonalAccessTokenModel::<CustomToken>();
        assert_eq!(
            console
                .handleCommand(&["sanctum:prune-expired".into(), "--hours=0".into()])
                .await
                .unwrap(),
            0
        );
        assert!(prune_events.take().is_empty());
        assert!(
            db.table(token_table)
                .r#where("id", "=", SqlValue::Unsigned(hidden.access_token.id))
                .exists()
                .await
                .unwrap()
        );
        assert!(
            tokens
                .findToken(expired.plain_text_token.as_str())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            tokens
                .findToken(read.plain_text_token.as_str())
                .await
                .unwrap()
                .is_some()
        );
    }
}

#[tokio::test]
async fn bearer_and_opt_in_spa_over_http_with_sqlite() {
    for stateful in [true, false] {
        verify(
            json!({"driver":"sqlite","database":":memory:"}),
            stateful,
            false,
        )
        .await;
    }
}

#[tokio::test]
async fn bearer_and_spa_over_http_with_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_SANCTUM_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SANCTUM_MYSQL").is_none());
        return;
    };
    for custom_csrf in [false, true] {
        verify(json!({"url":url}), true, custom_csrf).await;
    }
}

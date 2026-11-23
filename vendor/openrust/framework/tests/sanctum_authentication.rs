#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
#![cfg(feature = "sanctum")]

use std::sync::{Arc, Mutex};

use axum::{extract::State, handler::Handler as _};
use jiff::{SignedDuration, Timestamp};
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
            Attributes, Builder, Model, ModelMetadata, ModelState, Scope, UniqueId,
            casts::{Attribute, AttributeCast},
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::{MorphMap, MorphMapEntry},
        },
    },
    events::Dispatcher,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    sanctum::{
        AccessToken, DefaultPersonalAccessToken, HasApiTokens, PersonalAccessTokenModel, Sanctum,
        TokenAuthenticated,
    },
    support::facades::{Auth, Route},
    testing::TestApp,
};
use reqwest::{Client, header};
use serde_json::{Value, json};
use tokio::net::TcpListener;

struct User<const SERVICE: bool = false, const TEXT: u8 = 0>(ModelState);

struct CustomToken<const KEY: u8 = 0>(ModelState);
impl<const KEY: u8> Model for CustomToken<KEY> {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("custom_sanctum_tokens")
            .connection("token_fixture")
            .hidden(&["updated_at"])
            .appends(&["display_name"]);
        static UUID_META: ModelMetadata = ModelMetadata::new("custom_sanctum_tokens")
            .connection("token_fixture")
            .hidden(&["updated_at"])
            .appends(&["display_name"])
            .primaryKey("token_id")
            .uniqueId(UniqueId::Uuid);
        static ULID_META: ModelMetadata = ModelMetadata::new("custom_sanctum_tokens")
            .connection("token_fixture")
            .hidden(&["updated_at"])
            .appends(&["display_name"])
            .primaryKey("token_id")
            .uniqueId(UniqueId::Ulid);
        match KEY {
            1 => &UUID_META,
            2 => &ULID_META,
            _ => &META,
        }
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("last_used_at", AttributeCast::Timestamp)]
    }

    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "display_name" => Some(Attribute::get(|_, values| {
                let Some(SqlValue::Text(name)) = values.get("name") else {
                    return Ok(SqlValue::Null);
                };
                Ok(format!("Token {}", name.trim_start_matches("stored:")).into())
            })),
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

impl<const KEY: u8> PersonalAccessTokenModel for CustomToken<KEY> {
    #[allow(non_snake_case, reason = "Implements the Sanctum model lookup hook")]
    fn findToken<'a>(
        mut query: Builder<'static, Self>,
        connection: &'a Connection,
        credential: &'a str,
    ) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send + 'a {
        if let Some(credential) = credential.strip_prefix("lookup:") {
            query.r#where("name", "=", "stored:read");
            Self::findTokenByDigest(query, connection, credential)
        } else {
            Self::findTokenByDigest(query, connection, credential)
        }
    }
}

struct VisibleToken;

impl<const KEY: u8> Scope<CustomToken<KEY>> for VisibleToken {
    fn apply(&self, query: &mut Builder<'_, CustomToken<KEY>>) -> DatabaseResult<()> {
        query.r#where("name", "!=", "stored:hidden");
        Ok(())
    }
}

#[derive(Clone, Default)]
struct TokenEvents(Arc<Mutex<Vec<ModelEvent>>>);

impl TokenEvents {
    fn register<const KEY: u8>(&self, app: &Application) {
        use ModelEvent::*;
        for event in [
            Saving, Creating, Created, Saved, Retrieved, Updating, Updated, Deleting, Deleted,
        ] {
            CustomToken::<KEY>::registerModelEvent(app, event, self.clone()).unwrap();
        }
    }

    fn take(&self) -> Vec<ModelEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl<const KEY: u8> ModelEventListener<CustomToken<KEY>> for TokenEvents {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        token: &'a mut CustomToken<KEY>,
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

impl ModelEventListener<DefaultPersonalAccessToken> for TokenEvents {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        _: &'a mut DefaultPersonalAccessToken,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(event);
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

impl<const SERVICE: bool, const TEXT: u8> Model for User<SERVICE, TEXT> {
    fn metadata() -> &'static ModelMetadata {
        static USERS: ModelMetadata = ModelMetadata::new("sanctum_users")
            .timestamps(None, None)
            .incrementing(false);
        static SERVICES: ModelMetadata =
            ModelMetadata::new("sanctum_services").timestamps(None, None);
        if SERVICE { &SERVICES } else { &USERS }
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

impl<const SERVICE: bool, const TEXT: u8> Authenticatable for User<SERVICE, TEXT> {
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

impl<const SERVICE: bool, const KEY: u8> HasApiTokens for User<SERVICE, KEY> {}

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
            router
                .get("/scopes", whoami)?
                .middleware_name("scopes:read,write");
            router
                .get("/scope", whoami)?
                .middleware_name("scope:write,read");
            router.post("/user", whoami)?;
            Ok(())
        },
    )
}

fn web<const KEY: u8>() -> Result<(), RoutingError> {
    Route::post("/login", login::<KEY>)
}

fn login<const KEY: u8>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut guard = Auth::session_guard(request, Some("web"))?;
        let id = if KEY != 0 {
            AuthIdentifier::from("2")
        } else {
            AuthIdentifier::from(2_u64)
        };
        guard.login_using_id(&id, false).await?;
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
            Some(AccessToken::Test(_)) => json!("test"),
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
    kernel.handle_transport(request).await
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

async fn verify<const KEY: u8>(database: Value, stateful: bool, custom_csrf: bool) {
    let root = tempfile::tempdir().unwrap();
    if custom_csrf {
        std::fs::write(root.path().join("custom-csrf"), "").unwrap();
    }
    let mut app = Application::configure(root.path().to_path_buf())
        .with_config(|root, _| Ok([
            ("app".into(), json!({"key":"a".repeat(32)})),
            ("session".into(), json!({"driver":"array","cookie":"sanctum_test","lottery":[0,100],"same_site":"strict","http_only":false})),
            ("sanctum".into(), json!({"stateful":format!("spa.example:8443{}", Sanctum::currentRequestHost()),"expiration":5,"token_prefix":"fixture_",
                "middleware": if root.join("custom-csrf").is_file() {
                    json!({"validate_csrf_token":"custom-csrf"})
                } else { json!({}) }})),
            ("auth".into(), json!({
                "guards":{"web":{"driver":"session","provider":"users"},"sanctum":{"provider":null},"restricted":{"driver":"sanctum","provider":"users"}},
                "providers":{"users":{"driver":"eloquent","model":"SanctumUser"}}
            })),
        ].into()))
        .with_middleware(move |middleware| {
            if stateful { middleware.stateful_api(); }
            middleware.alias("custom-csrf", Arc::new(CustomCsrf));
        })
        .with_routing(Routing::default().api(api).web(web::<KEY>))
        .create().unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config.set("database.default", json!("fixture")).unwrap();
    config
        .set("database.connections.fixture", database.clone())
        .unwrap();
    let auth = app.container().make::<AuthManager>().unwrap();
    auth.register_sanctum_model::<User<false, KEY>>("SanctumUser");
    auth.register_sanctum_model::<User<true>>("ServiceAccount");
    auth.register_model::<User<false, 9>>("WithoutTokens");
    MorphMap::morphMap(
        &app,
        [
            MorphMapEntry::model::<User<false, KEY>>("users_alias"),
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
    assert_eq!(
        sanctum.personalAccessTokenModel(),
        std::any::type_name::<DefaultPersonalAccessToken>()
    );
    assert_eq!(sanctum.currentApplicationUrlWithPort(), "");
    for (url, suffix) in [
        ("https://Example.test:443/api?x=1", ",Example.test:443"),
        ("http://[::1]:8123/app", ",[::1]:8123"),
        ("https://example.test/path", ",example.test"),
    ] {
        config.set("app.url", json!(url)).unwrap();
        assert_eq!(sanctum.currentApplicationUrlWithPort(), suffix);
    }
    if KEY == 2 {
        config
            .set("app.url", json!("https://spa.example:8443/frontend"))
            .unwrap();
        config.set("sanctum", json!({
            "expiration":5,"token_prefix":"fixture_","guard":["web","restricted"],"middleware":{"validate_csrf_token":"custom-csrf"}
        })).unwrap();
    }
    let token_table = if custom_csrf {
        model_events.register::<KEY>(&app);
        sanctum.usePersonalAccessTokenModel::<CustomToken<KEY>>();
        assert_eq!(
            sanctum.personalAccessTokenModel(),
            std::any::type_name::<CustomToken<KEY>>()
        );
        "custom_sanctum_tokens"
    } else {
        use ModelEvent::*;
        for event in [
            Saving, Creating, Created, Saved, Retrieved, Updating, Updated, Deleting, Deleted,
        ] {
            DefaultPersonalAccessToken::registerModelEvent(&app, event, model_events.clone())
                .unwrap();
        }
        "personal_access_tokens"
    };
    db.statement("DROP TABLE IF EXISTS sanctum_users", &[])
        .await
        .unwrap();
    let user_key_type = if KEY != 0 { "VARCHAR(36)" } else { "BIGINT" };
    db.statement(
        &format!("CREATE TABLE sanctum_users (id {user_key_type} PRIMARY KEY, password VARCHAR(255) NOT NULL)"),
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
    let token_key = if KEY != 0 { "token_id" } else { "id" };
    let service_owner = if KEY != 0 {
        AuthIdentifier::from("d9428888-122b-4c26-9713-89ee9a1623d6")
    } else {
        AuthIdentifier::from(2_u64)
    };
    db.statement("DROP TABLE IF EXISTS sanctum_services", &[])
        .await
        .unwrap();
    let service_key_type = if KEY != 0 { "VARCHAR(36)" } else { "BIGINT" };
    db.statement(
        &format!("CREATE TABLE sanctum_services (id {service_key_type} PRIMARY KEY, password VARCHAR(255) NOT NULL)"),
        &[],
    ).await.unwrap();
    db.statement(
        "INSERT INTO sanctum_services VALUES (?, 'service-hash')",
        &[service_owner.clone().into()],
    )
    .await
    .unwrap();
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists(token_table).await.unwrap();
    schema
        .create(token_table, |table| {
            if KEY != 0 {
                if KEY == 2 {
                    table.ulid(token_key);
                } else {
                    table.uuid(token_key);
                }
                table.primary(&[token_key], None);
            } else {
                table.id();
            }
            table.string("tokenable_type", 255);
            if KEY != 0 {
                table.string("tokenable_id", 36);
            } else {
                table.unsignedBigInteger("tokenable_id");
            }
            table.text("name");
            table.string("token", 64).unique();
            table.text("abilities");
            table.timestamp("last_used_at", 0).nullable();
            table.timestamp("expires_at", 0).nullable();
            table.timestamps();
        })
        .await
        .unwrap();
    let owner = |id: u64| {
        if KEY != 0 {
            AuthIdentifier::from(id.to_string())
        } else {
            AuthIdentifier::from(id)
        }
    };
    let tokens = sanctum.personalAccessTokens(&database, "").unwrap();
    let read = tokens
        .create_token("users_alias", owner(1), "read", &["read"], None)
        .await
        .unwrap();
    assert!(
        read.plain_text_token
            .split_once('|')
            .unwrap()
            .1
            .starts_with("fixture_")
    );
    assert_eq!(
        model_events.take(),
        [
            ModelEvent::Saving,
            ModelEvent::Creating,
            ModelEvent::Created,
            ModelEvent::Saved
        ]
    );
    assert!(read.access_token.updated_at.is_some());
    if custom_csrf {
        assert_eq!(read.access_token.name, "read");
        assert!(read.access_token.can("write"));
        let stored_name = db
            .table(token_table)
            .r#where(token_key, "=", SqlValue::from(read.access_token.id.clone()))
            .value("name")
            .await
            .unwrap();
        assert_eq!(stored_name, "stored:read".into());
        assert!(
            tokens
                .create_token("users_alias", owner(1), "cancel", &["read"], None)
                .await
                .is_err()
        );
        assert_eq!(
            model_events.take(),
            [ModelEvent::Saving, ModelEvent::Creating]
        );
        assert_eq!(db.table(token_table).count("*").await.unwrap(), 1);
    }
    if KEY != 0 {
        let (key, secret) = read.plain_text_token.split_once('|').unwrap();
        if KEY == 2 {
            assert!(ulid::Ulid::from_string(key).is_ok());
        } else {
            assert!(uuid::Uuid::parse_str(key).is_ok());
        }
        assert_eq!(read.access_token.id, AuthIdentifier::String(key.into()));
        assert_eq!(
            tokens.findToken(secret).await.unwrap().unwrap().id,
            read.access_token.id
        );
        for invalid in ["not-a-uuid", "1"] {
            assert!(
                tokens
                    .findToken(&format!("{invalid}|{secret}"))
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        model_events.take();
    }
    let full = tokens
        .create_token("SanctumUser", owner(1), "full", &["*"], None)
        .await
        .unwrap();
    let expired = tokens
        .create_token(
            "SanctumUser",
            owner(1),
            "expired",
            &["*"],
            Some(Timestamp::now() - SignedDuration::from_secs(10)),
        )
        .await
        .unwrap();
    let aged = tokens
        .create_token("SanctumUser", owner(1), "aged", &["*"], None)
        .await
        .unwrap();
    db.statement(
        &format!(
            "UPDATE {token_table} SET created_at = '2020-01-01 00:00:00' WHERE {token_key} = ?"
        ),
        &[SqlValue::from(aged.access_token.id.clone())],
    )
    .await
    .unwrap();
    let wrong_type = tokens
        .create_token("AnotherUser", owner(1), "wrong", &["*"], None)
        .await
        .unwrap();
    let service = tokens
        .create_token(
            "services_alias",
            service_owner.clone(),
            "service",
            &["*"],
            None,
        )
        .await
        .unwrap();
    assert_eq!(service.access_token.tokenable_id, service_owner);
    let missing = tokens
        .create_token("SanctumUser", owner(3), "missing", &["*"], None)
        .await
        .unwrap();

    let mut fresh_user = User::<false, KEY>::newInstanceOn(&db, Attributes::new()).unwrap();
    fresh_user.setAttribute("id", SqlValue::from(owner(4)));
    fresh_user.setAttribute("password", "new-user-password");
    assert!(fresh_user.save().await.unwrap());
    assert!(fresh_user.state().wasRecentlyCreated());
    let mut test_app = TestApp::create(|builder| builder).unwrap();
    let fresh_user = Sanctum::actingAs(&mut test_app, fresh_user, &[], None);
    assert!(!fresh_user.state().wasRecentlyCreated());
    assert!(fresh_user.state().exists());

    let relation = fresh_user.tokens(&sanctum, &database).unwrap();
    let issued = fresh_user
        .create_token(&sanctum, &database, "related", &["read"], None)
        .await
        .unwrap();
    let payload: Value = serde_json::from_str(&issued.to_json().unwrap()).unwrap();
    assert_eq!(payload, issued.to_array().unwrap());
    assert_eq!(payload["plainTextToken"], issued.plain_text_token.as_str());
    assert_eq!(
        payload["accessToken"][token_key],
        json!(issued.access_token.id)
    );
    if custom_csrf {
        assert_eq!(payload["accessToken"]["display_name"], "Token related");
        assert!(payload["accessToken"].get("updated_at").is_none());
    } else {
        assert!(payload["accessToken"]["updated_at"].is_string());
    }
    assert!(payload["accessToken"].get("token").is_none());
    assert_eq!(issued.access_token.tokenable_type, "users_alias");
    assert_eq!(issued.access_token.tokenable_id, owner(4));
    let morphs = app.container().make::<MorphMap>().unwrap();
    let related_owner = issued
        .access_token
        .tokenable(&auth, &morphs)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        json!(related_owner.get_auth_identifier()),
        json!(fresh_user.get_auth_identifier())
    );
    assert_eq!(relation.get().await.unwrap().len(), 1);
    let case_variant = tokens
        .create_token("USERS_ALIAS", owner(4), "related", &["read"], None)
        .await
        .unwrap();
    async fn verify_typed<T: PersonalAccessTokenModel, const KEY: u8>(
        user: &User<false, KEY>,
        db: &Connection,
        database: &DatabaseManager,
        sanctum: &Sanctum,
    ) {
        let mut chain = user.tokensQuery::<T>(sanctum, database).unwrap();
        chain.r#where("name", "=", "nonexistent").orWhere(
            "tokenable_id",
            "=",
            SqlValue::Unsigned(1),
        );
        assert!(chain.get().await.unwrap().is_empty());
        let mut chain = user.tokensQuery::<T>(sanctum, database).unwrap();
        chain.r#where("name", "like", "%related");
        assert_eq!(chain.get().await.unwrap().len(), 1);
        let relation = user.tokensRelation::<T>(sanctum).unwrap();
        let mut owners = User::<false, KEY>::queryOn(db).with("tokens", relation);
        owners.whereKey(SqlValue::from(user.get_auth_identifier()));
        let owner = owners.first().await.unwrap().unwrap();
        let tokens = owner
            .getRelation::<openrust::database::eloquent::RelatedCollection<T>>("tokens")
            .unwrap();
        assert_eq!(tokens.len(), 1);
    }
    app.call(
        openrust::container::Callback::new(&[], |_| async {
            if custom_csrf {
                verify_typed::<CustomToken<KEY>, KEY>(&fresh_user, &db, &database, &sanctum).await;
            } else {
                verify_typed::<DefaultPersonalAccessToken, KEY>(
                    &fresh_user,
                    &db,
                    &database,
                    &sanctum,
                )
                .await;
            }
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();

    assert!(
        tokens
            .revokeToken("USERS_ALIAS", owner(4), case_variant.access_token.id)
            .await
            .unwrap()
    );

    assert_eq!(
        relation
            .delete_token(read.access_token.id.clone())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        relation.delete_token(issued.access_token.id).await.unwrap(),
        1
    );
    relation
        .create_token("related-again", &[], None)
        .await
        .unwrap();
    model_events.take();
    assert_eq!(relation.delete().await.unwrap(), 1);
    assert!(model_events.take().is_empty());
    assert!(relation.get().await.unwrap().is_empty());
    assert!(
        tokens
            .findToken(read.plain_text_token.as_str())
            .await
            .unwrap()
            .is_some()
    );

    let unsupported = tokens
        .create_token("WithoutTokens", owner(1), "unsupported", &["*"], None)
        .await
        .unwrap();
    let events = app
        .container()
        .make::<Dispatcher>()
        .unwrap()
        .fake(&[TokenAuthenticated::NAME]);
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
    assert_eq!(
        get("/api/user")
            .bearer_auth(unsupported.plain_text_token.as_str())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    {
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
            assert_eq!(
                response.json::<Value>().await.unwrap()["user"],
                json!(owner(1))
            );
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
            .revokeToken("SanctumUser", owner(1), full.access_token.id.clone())
            .await
            .unwrap()
    );
    assert_eq!(
        model_events.take(),
        [
            ModelEvent::Retrieved,
            ModelEvent::Deleting,
            ModelEvent::Deleted
        ]
    );
    if custom_csrf {
        use ModelEvent::*;
        assert!(
            !tokens
                .revokeToken("SanctumUser", owner(1), full.access_token.id.clone())
                .await
                .unwrap()
        );
        assert!(model_events.take().is_empty());
        assert!(
            !tokens
                .revokeToken("users_alias", owner(1), read.access_token.id.clone())
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

    events.assertDispatchedTimes(TokenAuthenticated::NAME, 6);
    let first = events.dispatched(TokenAuthenticated::NAME);
    let first = first[0].downcast_ref::<TokenAuthenticated>().unwrap();
    assert_eq!(first.token.id, read.access_token.id.clone());
    assert!(first.token.last_used_at.is_none());

    if custom_csrf {
        if KEY == 0 {
            let alternate = format!("lookup:{}", read.plain_text_token.as_str());
            assert_eq!(
                get("/api/user")
                    .bearer_auth(alternate)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                401
            );
        }
        sanctum.getAccessTokenFromRequestUsing(Some(Arc::new(|request| {
            request
                .headers()
                .get("x-lookup-token")?
                .to_str()
                .ok()
                .map(str::to_owned)
        })));
        for (credential, expected) in [(&read, 200), (&service, 401)] {
            let alternate = format!("lookup:{}", credential.plain_text_token.as_str());
            assert_eq!(
                tokens.findToken(&alternate).await.unwrap().is_some(),
                expected == 200
            );
            assert_eq!(
                get("/api/user")
                    .header("x-lookup-token", alternate)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
    }

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
                if path == "/api/restricted" {
                    json!(owner(1))
                } else {
                    json!(service_owner)
                }
            );
        }
    }

    if KEY != 0 {
        let (id, _) = read.plain_text_token.split_once('|').unwrap();
        for credential in [format!("{id}|wrong"), "invalid|secret".to_owned()] {
            assert_eq!(
                get("/api/user")
                    .bearer_auth(credential)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                401
            );
        }
    }
    if KEY != 0 {
        assert!(
            !tokens
                .revokeToken("services_alias", 0, service.access_token.id.clone())
                .await
                .unwrap()
        );
        assert!(
            !tokens
                .revokeToken(
                    "users_alias",
                    service_owner.clone(),
                    service.access_token.id.clone()
                )
                .await
                .unwrap()
        );
        assert!(
            tokens
                .revokeToken(
                    "services_alias",
                    service_owner,
                    service.access_token.id.clone()
                )
                .await
                .unwrap()
        );
        assert_eq!(
            get("/api/user")
                .bearer_auth(service.plain_text_token.as_str())
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
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
            json!({"user":owner(2),"token":"session"})
        );
        assert_eq!(
            get("/api/user")
                .header(header::ORIGIN, &base)
                .header(header::COOKIE, &session)
                .send()
                .await
                .unwrap()
                .status(),
            if KEY == 2 { 401 } else { 200 }
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
            json!(owner(1))
        );
    } else {
        assert_eq!(
            response.json::<Value>().await.unwrap()["user"],
            json!(owner(1))
        );
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
            .create_token(
                "SanctumUser",
                owner(1),
                "hidden",
                &["*"],
                Some(Timestamp::now() - SignedDuration::from_secs(120)),
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
        prune_events.register::<KEY>(&console);
        let config = console.container().get::<dyn Repository>("config").unwrap();
        config.set("database.default", json!("unused")).unwrap();
        config.set("sanctum.expiration", json!(5)).unwrap();
        config.set("app.timezone", json!("Asia/Shanghai")).unwrap();
        config
            .set("database.connections.token_fixture", fixture_config)
            .unwrap();
        console
            .container()
            .make::<Sanctum>()
            .unwrap()
            .usePersonalAccessTokenModel::<CustomToken<KEY>>();
        assert_eq!(
            console
                .handle_command(&["sanctum:prune-expired".into(), "--hours=0".into()])
                .await
                .unwrap(),
            0
        );
        assert!(
            tokens
                .findToken(aged.plain_text_token.as_str())
                .await
                .unwrap()
                .is_none()
        );
        assert!(prune_events.take().is_empty());
        assert!(
            db.table(token_table)
                .r#where(
                    token_key,
                    "=",
                    SqlValue::from(hidden.access_token.id.clone())
                )
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
        verify::<0>(
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
        verify::<0>(json!({"url":url}), true, custom_csrf).await;
    }
    verify::<1>(json!({"url":url}), true, true).await;
    verify::<2>(json!({"url":url}), true, true).await;
}

#[path = "../src/foundation/console/stubs/api-routes.stub"]
mod installed_api_routes;

#[tokio::test]
async fn sanctum_checks_configured_request_guard_without_a_session() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().canonicalize().unwrap())
        .with_config(|_, _| {
            Ok([
                (
                    "auth".into(),
                    json!({
                        "guards":{"frontend":{"driver":"trusted-header"}},
                        "providers":{"users":{"driver":"eloquent","model":
                            openrust::auth::model!(User<true>, "other-users", sanctum)
                        }}
                    }),
                ),
                ("sanctum".into(), json!({"guard":["sanctum","frontend"]})),
            ]
            .into())
        })
        .with_routing(Routing::default().api(installed_api_routes::register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let auth = app.container().make::<AuthManager>().unwrap();
    auth.via_request("trusted-header", |request| {
        let trusted = request
            .headers()
            .get("x-fixture")
            .is_some_and(|header| header == "yes");
        assert!(!request.has_session());
        Box::pin(async move {
            if !trusted {
                return Ok(None);
            }
            let mut user = User::<true>(ModelState::default());
            user.setAttribute("id", SqlValue::Unsigned(42));
            user.setAttribute("name", "Custom account");
            user.setAttribute("password", "hidden-secret");
            user.setAttribute("remember_token", "hidden-remember-token");
            Ok(Some(Box::new(user) as Box<dyn Authenticatable>))
        })
    });
    let (base, server) = start(app).await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/user");
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    let response = client
        .get(&url)
        .header("x-fixture", "yes")
        .bearer_auth("unrecognized")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"id":42,"name":"Custom account"})
    );
    server.abort();
}

#[tokio::test]
async fn acting_as_isolated_to_test_app_and_selected_guard() {
    fn token_ability(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request)
                .await?
                .ok_or(HttpError::Unauthenticated)?;
            let user = (user.as_ref() as &dyn std::any::Any)
                .downcast_ref::<User>()
                .ok_or(HttpError::Unauthenticated)?;
            let mut other = User::<false, 0>(ModelState::default());
            other.setAttribute("id", SqlValue::Unsigned(43));
            let abilities = json!({
                "read": user.tokenCan(request, "read"),
                "write_denied": user.tokenCant(request, "write"),
                "other_denied": other.tokenCant(request, "read"),
            });
            let token = AccessToken::Test(Arc::from(["write".to_owned()]));
            assert!(other.withAccessToken(request, token.clone()).is_err());
            user.withAccessToken(request, token)?;
            assert!(user.tokenCan(request, "write"));
            assert!(user.tokenCant(request, "read"));
            Response::json(StatusCode::OK, abilities)
        })
    }

    let configure = |builder: openrust::foundation::configuration::ApplicationBuilder| {
        builder
            .with_config(|_, _| {
                Ok([(
                    "auth".into(),
                    json!({
                        "guards":{"alternate":{"driver":"sanctum"}}
                    }),
                )]
                .into())
            })
            .with_routing(Routing::default().api(|| {
                api()?;
                Route::get("/default", whoami)?;
                Route::get("/token-ability", token_ability)?;
                Route::getWith("/alternate", whoami, |route| {
                    route.middleware_name("auth:alternate");
                    Ok(())
                })
            }))
    };
    let mut app = TestApp::create(configure).unwrap();
    let mut other = TestApp::create(configure).unwrap();
    let user = || {
        let mut user = User::<false, 0>(ModelState::default());
        user.setAttribute("id", SqlValue::Unsigned(42));
        user
    };
    let selected = Sanctum::actingAs(&mut app, user(), &["read"], None);
    assert_eq!(selected.get_auth_identifier(), AuthIdentifier::from(42_u64));
    for path in ["/api/user", "/api/any", "/api/default"] {
        let response = app.get(path).await.unwrap();
        response.assert_ok();
        assert_eq!(
            response.json::<Value>().unwrap(),
            json!({"user":42,"token":"test"})
        );
    }
    let abilities = app.get("/api/token-ability").await.unwrap();
    abilities.assert_ok();
    assert_eq!(
        abilities.json::<Value>().unwrap(),
        json!({
            "read":true,"write_denied":true,"other_denied":true
        })
    );
    app.get("/api/all")
        .await
        .unwrap()
        .assert_status(StatusCode::FORBIDDEN);
    app.get("/api/scope").await.unwrap().assert_ok();
    for (path, message) in [
        ("/api/all", "Invalid ability provided."),
        ("/api/scopes", "Invalid scope(s) provided."),
    ] {
        let response = app
            .call(
                axum::http::Request::get(path)
                    .header("accept", "application/json")
                    .body(openrust::http::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(response.json::<Value>().unwrap()["message"], message);
    }
    let missing = openrust::sanctum::MissingAbilityException::new(["read"]);
    assert_eq!(missing.abilities(), ["read"]);
    let missing = openrust::sanctum::MissingScopeException::new(["write", "read"]);
    assert_eq!(missing.scopes(), ["write", "read"]);
    other
        .get("/api/user")
        .await
        .unwrap()
        .assert_status(StatusCode::UNAUTHORIZED);
    Sanctum::actingAs(&mut app, user(), &[], None);
    app.get("/api/any")
        .await
        .unwrap()
        .assert_status(StatusCode::FORBIDDEN);
    Sanctum::actingAs(&mut app, user(), &["*"], None);
    app.get("/api/all").await.unwrap().assert_ok();
    Sanctum::actingAs(&mut app, user(), &["*"], Some("alternate"));
    app.get("/api/alternate").await.unwrap().assert_ok();
    app.get("/api/default").await.unwrap().assert_ok();
    app.get("/api/user")
        .await
        .unwrap()
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn publishes_sanctum_resources_by_tag_without_overwriting_customizations() {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|_, _| {
            Ok([(
                "database".into(),
                json!({
                    "migrations": {"update_date_on_publish": true}
                }),
            )]
            .into())
        })
    })
    .unwrap();
    let config = app.root().join("config/sanctum.rs");
    let migrations = app.root().join("database/migrations");
    assert_eq!(
        app.artisan(["vendor:publish", "--tag=sanctum-config"])
            .await
            .status,
        0
    );
    assert!(config.is_file());
    assert!(!migrations.exists());
    std::fs::write(&config, "// customized").unwrap();
    assert_eq!(
        app.artisan([
            "vendor:publish",
            "--provider=OpenRust\\Sanctum\\SanctumServiceProvider"
        ])
        .await
        .status,
        0
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "// customized");
    let published = std::fs::read_dir(&migrations)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(published.len(), 1);
    let name = published[0].file_name().into_string().unwrap();
    assert!(name.ends_with("_create_personal_access_tokens_table.rs"));
    let bundled = "0001_01_01_000012_create_personal_access_tokens_table.rs";
    assert_ne!(name, bundled);
    assert_eq!(
        app.artisan(["vendor:publish", "--tag=sanctum-migrations"])
            .await
            .status,
        0
    );
    assert_eq!(std::fs::read_dir(&migrations).unwrap().count(), 1);
    std::fs::rename(migrations.join(name), migrations.join(bundled)).unwrap();
    let result = app
        .artisan(["vendor:publish", "--tag=sanctum-migrations", "--force"])
        .await;
    result.assert_successful();
    assert!(migrations.join(bundled).is_file());
    assert_eq!(std::fs::read_dir(&migrations).unwrap().count(), 1);

    let mut without_retiming = TestApp::create(|builder| builder).unwrap();
    without_retiming
        .artisan(["vendor:publish", "--tag=sanctum-migrations"])
        .await
        .assert_successful();
    assert!(
        without_retiming
            .root()
            .join("database/migrations")
            .join(bundled)
            .is_file()
    );
    assert_eq!(
        app.artisan(["vendor:publish", "--force", "--tag=sanctum-config"])
            .await
            .status,
        0
    );
    assert!(
        std::fs::read_to_string(config)
            .unwrap()
            .contains("SANCTUM_TOKEN_PREFIX")
    );
}

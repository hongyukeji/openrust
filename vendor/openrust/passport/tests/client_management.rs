#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{
    config::{ConfigError, Environment, Value as Json},
    contracts::{console::Kernel as _, container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Model, ModelState,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::Relation,
        },
        migrations::{MigrationEntry, Migrator, RunOptions},
    },
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_passport::{
    AuthCode, Client, ClientModel, ClientRepository, Passport, PassportServiceProvider,
    RefreshToken, Token, migrations,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

static MIGRATIONS: &[MigrationEntry] = &[
    MigrationEntry::new(
        "database/migrations/2016_06_01_000004_create_oauth_clients_table.rs",
        &migrations::oauth_clients::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2016_06_01_000001_create_oauth_auth_codes_table.rs",
        &migrations::oauth_auth_codes::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2016_06_01_000002_create_oauth_access_tokens_table.rs",
        &migrations::oauth_access_tokens::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2016_06_01_000003_create_oauth_refresh_tokens_table.rs",
        &migrations::oauth_refresh_tokens::Definition,
    ),
];
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "passport-management-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("storage")).unwrap();
        Self(path)
    }
    async fn app(&self) -> Application {
        let mut app = Application::configure(self.0.clone())
            .with_config(configuration)
            .with_providers(vec![Box::new(PassportServiceProvider)])
            .with_migrations(MIGRATIONS)
            .with_routing(Routing::new(routes))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        app
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
async fn in_app<R: Send>(app: &Application, future: impl Future<Output = R> + Send) -> R {
    app.call(
        openrust::container::Callback::new(&[], |_| async { Ok(future.await) }),
        Default::default(),
    )
    .await
    .unwrap()
}
fn configuration(_: &Path, _: &Environment) -> Result<BTreeMap<String, Json>, ConfigError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let prefix = format!(
        "management_{}_{}_",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL")
        .unwrap_or_else(|_| "mysql://unused@127.0.0.1:9/unused".into());
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing","name":"Default client","url":"https://app.test/base","host":"127.0.0.1","port":0}),
        ),
        (
            "database".into(),
            json!({"default":"main","connections":{"main":{"url":url,"prefix":format!("{prefix}main_")},"oauth":{"url":url,"prefix":format!("{prefix}oauth_")}},"migrations":{"table":"history"}}),
        ),
        ("passport".into(), json!({"connection":"oauth"})),
        (
            "auth".into(),
            json!({"guards":{"api":{"driver":"passport","provider":"users"}},"providers":{"users":{"driver":"eloquent"}}}),
        ),
        (
            "hashing".into(),
            json!({"driver":"bcrypt","bcrypt":{"rounds":4}}),
        ),
    ]))
}
fn required() -> bool {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_some() {
        return true;
    }
    assert!(
        std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
        "MySQL required"
    );
    eprintln!("SKIP management MySQL: URL absent");
    false
}
async fn database(app: &Application) -> Connection {
    app.make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("oauth"))
        .unwrap()
}
async fn migrate(app: &Application) {
    let rows = in_app(app, async {
        app.make::<Migrator>()
            .await
            .unwrap()
            .run(None, &[], RunOptions::default())
            .await
            .unwrap()
    })
    .await;
    assert_eq!(rows.len(), 4);
}
async fn cleanup(manager: &DatabaseManager) {
    let db = manager.connection(Some("oauth")).unwrap();
    for table in [
        "oauth_refresh_tokens",
        "oauth_access_tokens",
        "oauth_auth_codes",
        "oauth_clients",
    ] {
        db.getSchemaBuilder().dropIfExists(table).await.unwrap();
    }
    manager
        .connection(Some("main"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("history")
        .await
        .unwrap();
}
fn routes(_: &Application) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.get("/exercise", exercise)?;
    router.post("/manage", manage)?;
    router.get("/manage/{client}", manage)?;
    router.patch("/manage/{client}", manage)?;
    router.delete("/manage/{client}", manage)?;
    router.post("/manage/{client}/secret", manage)?;
    Ok(router)
}
fn exercise(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        exercise_models()
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        Response::json(StatusCode::OK, json!({"passed":true}))
    })
}
async fn exercise_models() -> DatabaseResult<()> {
    use openrust::support::facades::Config;
    let repo = openrust::container::Container::get_instance()
        .make::<ClientRepository>()
        .unwrap();
    let first = repo.createClientCredentialsGrantClient("first").await?;
    let second = repo.createClientCredentialsGrantClient("second").await?;
    let db = Token::default().getConnection()?;
    let mut token = Token::newInstanceOn(
        &db,
        Attributes::from([
            ("id".into(), "token-a".into()),
            ("client_id".into(), first.getKey().await.unwrap()),
            ("scopes".into(), Value::Json(json!(["orders"]))),
            ("revoked".into(), false.into()),
        ]),
    )?;
    token.save().await?;
    assert!(token.can("orders")?);
    assert!(token.cant("orders:read")?);
    let passport = openrust::container::Container::get_instance()
        .make::<Passport>()
        .unwrap();
    passport.withInheritedScopes.store(true, Ordering::Relaxed);
    assert!(token.can("orders:read")?);
    assert!(!token.can("other")?);
    let mut refresh = RefreshToken::newInstanceOn(
        &db,
        Attributes::from([
            ("id".into(), "refresh-a".into()),
            ("access_token_id".into(), "token-a".into()),
            ("revoked".into(), false.into()),
        ]),
    )?;
    refresh.save().await?;
    let loaded_client = token.client().getResults(&token).await?.unwrap();
    assert_eq!(loaded_client.getKey(), first.getKey().await.as_ref());
    assert_eq!(
        token
            .refreshToken()
            .getResults(&token)
            .await?
            .unwrap()
            .getKey(),
        refresh.getKey()
    );
    assert_eq!(
        refresh
            .accessToken()
            .getResults(&refresh)
            .await?
            .unwrap()
            .getKey(),
        token.getKey()
    );
    assert!(refresh.state().getAttribute("created_at").is_none());
    let mut orphan = Token::newInstanceOn(
        &db,
        Attributes::from([
            ("id".into(), "token-z".into()),
            ("client_id".into(), second.getKey().await.unwrap()),
            ("revoked".into(), false.into()),
        ]),
    )?;
    orphan.save().await?;
    let mut no_refresh = Token::newInstanceOn(
        &db,
        Attributes::from([
            ("id".into(), "token-b".into()),
            ("client_id".into(), first.getKey().await.unwrap()),
            ("revoked".into(), false.into()),
        ]),
    )?;
    no_refresh.save().await?;
    let mut code = AuthCode::newInstanceOn(
        &db,
        Attributes::from([
            ("id".into(), "code-a".into()),
            ("client_id".into(), first.getKey().await.unwrap()),
            ("user_id".into(), 1_i64.into()),
            ("revoked".into(), false.into()),
            ("expires_at".into(), "2027-01-01 00:00:00".into()),
        ]),
    )?;
    code.save().await?;
    assert_eq!(
        code.client().getResults(&code).await?.unwrap().getKey(),
        loaded_client.getKey()
    );
    assert_eq!(
        loaded_client
            .tokens()
            .getResults(&loaded_client)
            .await?
            .len(),
        2
    );
    assert_eq!(
        loaded_client
            .authCodes()
            .getResults(&loaded_client)
            .await?
            .len(),
        1
    );
    let failure = Config::get("probe.fail")
        .unwrap()
        .is_some_and(|v| v == json!(true));
    let outcome = repo.delete(&first).await;
    if failure {
        assert!(outcome.is_err());
        assert_eq!(
            Token::queryOn(&db)
                .find("token-a")
                .await?
                .unwrap()
                .getAttributeValue("revoked")?,
            Some(false.into())
        );
        assert_eq!(
            Client::queryOn(&db)
                .find(first.getKey().await.unwrap())
                .await?
                .unwrap()
                .getAttributeValue("revoked")?,
            Some(false.into())
        );
        return Ok(());
    }
    outcome?;
    assert_eq!(
        Token::queryOn(&db)
            .find("token-a")
            .await?
            .unwrap()
            .getAttributeValue("revoked")?,
        Some(true.into())
    );
    assert_eq!(
        Token::queryOn(&db)
            .find("token-b")
            .await?
            .unwrap()
            .getAttributeValue("revoked")?,
        Some(true.into())
    );
    assert_eq!(
        RefreshToken::queryOn(&db)
            .find("refresh-a")
            .await?
            .unwrap()
            .getAttributeValue("revoked")?,
        Some(true.into())
    );
    assert_eq!(
        Token::queryOn(&db)
            .find("token-z")
            .await?
            .unwrap()
            .getAttributeValue("revoked")?,
        Some(false.into())
    );
    assert!(
        repo.findActive(first.getKey().await.unwrap())
            .await?
            .is_none()
    );
    assert!(
        repo.findActive(second.getKey().await.unwrap())
            .await?
            .is_some()
    );
    assert_eq!(
        AuthCode::queryOn(&db)
            .find("code-a")
            .await?
            .unwrap()
            .getAttributeValue("revoked")?,
        Some(false.into())
    );
    Ok(())
}
struct Audit {
    label: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    fail: bool,
}
impl<M: Model> ModelEventListener<M> for Audit {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut M,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if model.getAttributeValue("revoked")? == Some(true.into()) {
                self.events.lock().unwrap().push(self.label.into());
                if self.fail {
                    return Err(openrust::database::DatabaseError::InvalidQuery(
                        "probe refresh failure",
                    ));
                }
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}
async fn serve(app: Application) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handle_transport(request));
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }),
    )
}
async fn revocation(fail: bool) {
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    app.config()
        .unwrap()
        .set("probe.fail", json!(fail))
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    RefreshToken::registerModelEvent(
        &app,
        ModelEvent::Saving,
        Audit {
            label: "refresh",
            events: events.clone(),
            fail,
        },
    )
    .unwrap();
    Token::registerModelEvent(
        &app,
        ModelEvent::Saving,
        Audit {
            label: "token",
            events: events.clone(),
            fail: false,
        },
    )
    .unwrap();
    Client::registerModelEvent(
        &app,
        ModelEvent::Saving,
        Audit {
            label: "client",
            events: events.clone(),
            fail: false,
        },
    )
    .unwrap();
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let (address, server) = serve(app).await;
    let response = reqwest::get(format!("http://{address}/exercise"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert_eq!(
        *events.lock().unwrap(),
        if fail {
            vec!["refresh"]
        } else {
            vec!["refresh", "token", "token", "client"]
        }
    );
    server.abort();
    cleanup(&manager).await;
}
#[tokio::test]
async fn mysql_revocation_uses_model_events_relations_and_order() {
    if required() {
        revocation(false).await;
    }
}
#[tokio::test]
async fn mysql_revocation_propagates_refresh_failure_before_token_or_client() {
    if required() {
        revocation(true).await;
    }
}

#[tokio::test]
async fn mysql_artisan_client_creates_each_grant_and_preserves_output_rules() {
    if !required() {
        return;
    }
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    let db = database(&app).await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let mut kernel =
        openrust::foundation::console::Kernel::new(app, openrust::console::Application::new());
    for (flags, grant, public, details) in [
        (
            vec!["--personal", "--password", "--public"],
            "personal_access",
            false,
            false,
        ),
        (vec!["--password", "--public"], "password", true, true),
        (
            vec!["--client", "--public"],
            "client_credentials",
            false,
            true,
        ),
        (vec!["--implicit"], "implicit", true, true),
        (
            vec!["--device", "--public"],
            "urn:ietf:params:oauth:grant-type:device_code",
            true,
            true,
        ),
        (vec![], "authorization_code", false, true),
    ] {
        let mut args: Vec<std::ffi::OsString> =
            vec!["passport:client".into(), "--no-interaction".into()];
        args.extend(flags.iter().map(Into::into));
        let mut output = openrust::console::Output::default();
        assert_eq!(
            kernel.handle(&args, &mut output).await,
            0,
            "{} {}",
            output.stdout,
            output.stderr
        );
        assert!(output.stdout.contains("New client created successfully."));
        assert_eq!(output.stdout.contains("Client ID"), details);
        assert_eq!(output.stdout.contains("Client Secret"), details && !public);
        let models = Client::queryOn(&db).get().await.unwrap();
        let model = models.all().last().unwrap();
        // UUID ordering is unrelated to creation order: select by the unique grant.
        let model = models
            .all()
            .iter()
            .find(|m| m.hasGrantType(grant).unwrap())
            .unwrap_or(model);
        assert_eq!(model.confidential(), !public);
        assert!(model.hasGrantType(grant).unwrap());
        assert_eq!(
            model.getAttributeValue("name").unwrap(),
            Some("Default client".into())
        );
        if matches!(grant, "authorization_code" | "implicit") {
            assert_eq!(
                model.redirectUris().unwrap().as_value(),
                &json!(["https://app.test/base/auth/callback"])
            );
        }
        db.table("oauth_clients").delete().await.unwrap();
    }
    let mut output = openrust::console::Output::default();
    let args = [
        "passport:client",
        "--no-interaction",
        "--public",
        "--name=Explicit",
        "--redirect_uri=https://a.test/cb,https://b.test/cb",
    ];
    assert_eq!(kernel.handle(&args.map(Into::into), &mut output).await, 0);
    let model = Client::queryOn(&db).first().await.unwrap().unwrap();
    assert_eq!(
        model.redirectUris().unwrap().as_value(),
        &json!(["https://a.test/cb", "https://b.test/cb"])
    );
    assert!(!model.confidential());
    cleanup(&manager).await;
}

#[test]
fn token_scope_empty_wildcard_and_metadata_defaults() {
    let token = Token::default();
    assert!(!token.can("anything").unwrap());
    assert!(token.cant("anything").unwrap());
    let mut state = ModelState::default();
    state.setAttribute("scopes", Value::Json(json!(["*"])));
    assert!(Token::from_state(state).can("anything").unwrap());
    for empty in ["null", "false", "0", "\"0\"", "\"\""] {
        let mut state = ModelState::default();
        state.setAttribute("scopes", empty);
        assert!(!Token::from_state(state).can("anything").unwrap());
    }
    assert!(!Token::getIncrementing());
    assert!(!RefreshToken::getIncrementing());
    assert!(!AuthCode::getIncrementing());
}

#[tokio::test]
async fn mysql_client_uuid_switch_preserves_constructed_instances_and_numeric_lookup() {
    if !required() {
        return;
    }
    let root = Root::new();
    let app = root.app().await;
    let db = database(&app).await;
    db.getSchemaBuilder()
        .create("oauth_clients", |table| {
            table.id();
            table.nullableMorphs("owner", None, None);
            table.string("name", None);
            table.string("secret", Some(100)).nullable();
            table.string("provider", None).nullable();
            table.text("redirect_uris");
            table.text("grant_types");
            table.boolean("revoked");
            table.timestamps();
        })
        .await
        .unwrap();
    in_app(&app, async {
        let passport = app.make::<Passport>().await.unwrap();
        let mut uuid = Client::default();
        passport.clientUuids.store(false, Ordering::Relaxed);
        let mut numeric = Client::newInstanceOn(
            &db,
            Attributes::from([
                ("name".into(), "numeric".into()),
                ("redirect_uris".into(), Value::Json(json!([]))),
                (
                    "grant_types".into(),
                    Value::Json(json!(["client_credentials"])),
                ),
                ("revoked".into(), false.into()),
            ]),
        )
        .unwrap();
        assert_eq!(
            numeric.getKeyType(),
            openrust::database::eloquent::ModelKeyType::Integer
        );
        passport.clientUuids.store(true, Ordering::Relaxed);
        // The saved instance keeps its initialization strategy despite later app changes.
        numeric.save().await.unwrap();
        assert!(matches!(numeric.getKey(), Some(Value::Integer(1))));
        uuid.setUniqueIds().await.unwrap();
        assert!(matches!(uuid.getKey(), Some(Value::Text(id)) if Client::isValidUniqueId(id)));
        passport.clientUuids.store(false, Ordering::Relaxed);
        uuid.setUniqueIds().await.unwrap();
        assert!(!uuid.getIncrementingForInstance());
        assert!(numeric.getIncrementingForInstance());
        let repo = app.make::<ClientRepository>().await.unwrap();
        let client = repo
            .createClientCredentialsGrantClient("created")
            .await
            .unwrap();
        assert_eq!(client.getKey().await, Some(2_i64.into()));
        assert!(repo.find(1_i64).await.unwrap().is_some());
        assert_eq!(Client::routeBindingValue("id", "2"), Some(2_i64.into()));
        let made = Client::factory()
            .unwrap()
            .asPublic()
            .createOne(Attributes::new())
            .await
            .unwrap();
        assert_eq!(made.getKey(), Some(&3_i64.into()));
    })
    .await;
    cleanup(&app.make::<DatabaseManager>().await.unwrap()).await;
}

macro_rules! custom_model {
    ($name:ident, $table:literal, $trait:ident) => {
        struct $name(ModelState);
        impl Model for $name {
            fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
                static META: openrust::database::eloquent::ModelMetadata =
                    openrust::database::eloquent::ModelMetadata::new($table)
                        .connection("main")
                        .incrementing(false)
                        .guarded(&[])
                        .timestamps(None, None);
                &META
            }
            fn casts() -> &'static [(
                &'static str,
                openrust::database::eloquent::casts::AttributeCast,
            )] {
                &[(
                    "revoked",
                    openrust::database::eloquent::casts::AttributeCast::Boolean,
                )]
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
        impl openrust_passport::$trait for $name {}
    };
}
custom_model!(CustomToken, "custom_tokens", TokenModel);
custom_model!(CustomRefresh, "custom_refreshes", RefreshTokenModel);

#[tokio::test]
async fn mysql_custom_token_registration_retains_models_events_and_separate_connection() {
    if !required() {
        return;
    }
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let main = manager.connection(Some("main")).unwrap();
    for (table, foreign) in [
        ("custom_tokens", "client_id"),
        ("custom_refreshes", "access_token_id"),
    ] {
        main.getSchemaBuilder()
            .create(table, |table| {
                table.string("id", Some(100)).primary();
                table.string(foreign, Some(100));
                table.boolean("revoked");
            })
            .await
            .unwrap();
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    CustomToken::registerModelEvent(
        &app,
        ModelEvent::Saving,
        Audit {
            label: "custom token",
            events: events.clone(),
            fail: false,
        },
    )
    .unwrap();
    CustomRefresh::registerModelEvent(
        &app,
        ModelEvent::Saving,
        Audit {
            label: "custom refresh",
            events: events.clone(),
            fail: false,
        },
    )
    .unwrap();
    let (client, bound_token) = app
        .container()
        .scope(async {
            // Resolve the repository before changing the model registrations.
            let repo = app.make::<ClientRepository>().await.unwrap();
            let passport = app.make::<Passport>().await.unwrap();
            passport.useTokenModel::<CustomToken>();
            passport.useRefreshTokenModel::<CustomRefresh>();
            assert_eq!(passport.tokenModel(), std::any::type_name::<CustomToken>());
            assert_eq!(
                passport.refreshTokenModel(),
                std::any::type_name::<CustomRefresh>()
            );
            let client = repo
                .createClientCredentialsGrantClient("custom")
                .await
                .unwrap();
            let mut token = CustomToken::newInstanceOn(
                &main,
                Attributes::from([
                    ("id".into(), "custom-t".into()),
                    ("client_id".into(), client.getKey().await.unwrap()),
                    ("revoked".into(), false.into()),
                ]),
            )
            .unwrap();
            token.save().await.unwrap();
            let mut refresh = CustomRefresh::newInstanceOn(
                &main,
                Attributes::from([
                    ("id".into(), "custom-r".into()),
                    ("access_token_id".into(), "custom-t".into()),
                    ("revoked".into(), false.into()),
                ]),
            )
            .unwrap();
            refresh.save().await.unwrap();
            let model = client.model::<Client>().unwrap();
            let guard = model.lock().await;
            assert_eq!(
                guard
                    .tokensFor::<CustomToken>()
                    .getResults(&guard)
                    .await
                    .unwrap()
                    .len(),
                1
            );
            drop(guard);
            repo.delete(&client).await.unwrap();
            assert_eq!(
                *events.lock().unwrap(),
                vec!["custom refresh", "custom token"]
            );
            assert_eq!(
                CustomToken::queryOn(&main)
                    .find("custom-t")
                    .await
                    .unwrap()
                    .unwrap()
                    .getAttributeValue("revoked")
                    .unwrap(),
                Some(true.into())
            );
            assert_eq!(
                Token::queryOn(&database(&app).await)
                    .get()
                    .await
                    .unwrap()
                    .len(),
                0
            );
            // A default token relation must use passport.connection, even when its parent is on main.
            let bound_token = Token::newInstanceOn(
                &main,
                Attributes::from([("client_id".into(), client.getKey().await.unwrap())]),
            )
            .unwrap();
            (client, bound_token)
        })
        .await;
    let other_root = Root::new();
    let other_app = other_root.app().await;
    other_app
        .config()
        .unwrap()
        .set("passport.connection", json!("missing"))
        .unwrap();
    other_app
        .container()
        .scope(async {
            // Bound models continue resolving through their owning app, not the current app.
            let related = bound_token
                .client()
                .getResults(&bound_token)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(related.getKey(), client.getKey().await.as_ref());
        })
        .await;
    for table in ["custom_refreshes", "custom_tokens"] {
        main.getSchemaBuilder().dropIfExists(table).await.unwrap();
    }
    cleanup(&manager).await;
}

struct ManagementUser(ModelState);
impl Model for ManagementUser {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("management_users")
                .connection("main")
                .guarded(&[])
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
impl openrust::contracts::auth::Authenticatable for ManagementUser {
    fn get_auth_identifier_name(&self) -> &str {
        "subject"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        self.getAttribute("subject").unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        match self.getAttribute("password") {
            Some(Value::Text(password)) => Some(password),
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

#[tokio::test]
async fn mysql_owner_and_user_relations_keep_primary_and_authentication_keys_distinct() {
    if !required() {
        return;
    }
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let main = manager.connection(Some("main")).unwrap();
    main.getSchemaBuilder()
        .create("management_users", |table| {
            table.id();
            table.bigInteger("subject");
        })
        .await
        .unwrap();
    let mut user = ManagementUser::newInstanceOn(
        &main,
        Attributes::from([("subject".into(), 991_i64.into())]),
    )
    .unwrap();
    user.save().await.unwrap();
    let user = ManagementUser::queryOn(&main)
        .find(user.getKey().unwrap().clone())
        .await
        .unwrap()
        .unwrap();
    in_app(&app, async {
        let mut client = Client::newInstanceOn(
            &database(&app).await,
            Attributes::from([
                ("owner_id".into(), user.getKey().unwrap().clone()),
                ("owner_type".into(), "user".into()),
                ("user_id".into(), user.getKey().unwrap().clone()),
            ]),
        )
        .unwrap();
        let owner = client
            .owner()
            .target::<ManagementUser>("user", "id")
            .getResults(&client)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            owner.downcast_ref::<ManagementUser>().unwrap().getKey(),
            user.getKey()
        );
        assert_eq!(
            client
                .user::<ManagementUser>()
                .getResults(&client)
                .await
                .unwrap()
                .unwrap()
                .getKey(),
            user.getKey()
        );
        let token = Token::newInstanceOn(
            &database(&app).await,
            Attributes::from([("user_id".into(), 991_i64.into())]),
        )
        .unwrap();
        assert!(token.user::<ManagementUser>("id").is_err());
        let resolved = token
            .user::<ManagementUser>("subject")
            .unwrap()
            .getResults(&token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved.getKey(), user.getKey());
        // Relationship keys are primary keys even if URL binding uses another column.
        client.state_mut().setAttribute("user_id", Value::Null);
        assert!(
            client
                .user::<ManagementUser>()
                .getResults(&client)
                .await
                .unwrap()
                .is_none()
        );
    })
    .await;
    main.getSchemaBuilder()
        .dropIfExists("management_users")
        .await
        .unwrap();
    cleanup(&manager).await;
}

struct CancelRefresh;
impl ModelEventListener<RefreshToken> for CancelRefresh {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut RefreshToken,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            Ok(
                if model.getAttributeValue("revoked")? == Some(true.into()) {
                    ModelEventResponse::Cancel
                } else {
                    ModelEventResponse::Continue
                },
            )
        })
    }
}
#[tokio::test]
async fn mysql_revocation_ignores_cancelled_save_but_propagates_token_errors() {
    if !required() {
        return;
    }
    for token_error in [false, true] {
        let root = Root::new();
        let app = root.app().await;
        migrate(&app).await;
        let manager = app.make::<DatabaseManager>().await.unwrap();
        let db = database(&app).await;
        RefreshToken::registerModelEvent(&app, ModelEvent::Saving, CancelRefresh).unwrap();
        Token::registerModelEvent(
            &app,
            ModelEvent::Saving,
            Audit {
                label: "token",
                events: Arc::default(),
                fail: token_error,
            },
        )
        .unwrap();
        in_app(&app, async {
            let repo = app.make::<ClientRepository>().await.unwrap();
            let client = repo
                .createClientCredentialsGrantClient("cancel")
                .await
                .unwrap();
            let mut token = Token::newInstanceOn(
                &db,
                Attributes::from([
                    ("id".into(), "cancel-t".into()),
                    ("client_id".into(), client.getKey().await.unwrap()),
                    ("revoked".into(), false.into()),
                ]),
            )
            .unwrap();
            token.save().await.unwrap();
            let mut refresh = RefreshToken::newInstanceOn(
                &db,
                Attributes::from([
                    ("id".into(), "cancel-r".into()),
                    ("access_token_id".into(), "cancel-t".into()),
                    ("revoked".into(), false.into()),
                ]),
            )
            .unwrap();
            refresh.save().await.unwrap();
            assert_eq!(repo.delete(&client).await.is_err(), token_error);
            assert_eq!(
                RefreshToken::queryOn(&db)
                    .find("cancel-r")
                    .await
                    .unwrap()
                    .unwrap()
                    .getAttributeValue("revoked")
                    .unwrap(),
                Some(false.into())
            );
            assert_eq!(
                Token::queryOn(&db)
                    .find("cancel-t")
                    .await
                    .unwrap()
                    .unwrap()
                    .getAttributeValue("revoked")
                    .unwrap(),
                Some((!token_error).into())
            );
            assert_eq!(
                Client::queryOn(&db)
                    .find(client.getKey().await.unwrap())
                    .await
                    .unwrap()
                    .unwrap()
                    .getAttributeValue("revoked")
                    .unwrap(),
                Some((!token_error).into())
            );
        })
        .await;
        cleanup(&manager).await;
    }
}

#[tokio::test]
async fn mysql_artisan_provider_without_api_default_stays_null_and_string_foreign_keys_use_uuid_columns()
 {
    if !required() {
        return;
    }
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    app.config().unwrap().set("auth.guards", json!({})).unwrap();
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let db = database(&app).await;
    db.getSchemaBuilder()
        .create("key_metadata", |table| {
            table.foreignIdFor::<Token>(Some("token_id"));
            table.foreignIdFor::<RefreshToken>(Some("refresh_id"));
            table.foreignIdFor::<AuthCode>(Some("code_id"));
        })
        .await
        .unwrap();
    for column in db
        .getSchemaBuilder()
        .getColumns("key_metadata")
        .await
        .unwrap()
    {
        assert_eq!(column.full_type, "char(36)");
    }
    db.getSchemaBuilder()
        .dropIfExists("key_metadata")
        .await
        .unwrap();
    let mut kernel =
        openrust::foundation::console::Kernel::new(app, openrust::console::Application::new());
    let mut output = openrust::console::Output::default();
    assert_eq!(
        kernel
            .handle(
                &["passport:client", "--personal", "--no-interaction"].map(Into::into),
                &mut output
            )
            .await,
        0,
        "{}",
        output.stderr
    );
    let model = Client::queryOn(&db).first().await.unwrap().unwrap();
    assert_eq!(
        model.getAttributeValue("provider").unwrap(),
        Some(Value::Null)
    );
    cleanup(&manager).await;
}

#[tokio::test]
async fn mysql_revocation_crosses_chunk_boundary_without_skipping_tokens() {
    if !required() {
        return;
    }
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let db = database(&app).await;
    let count = Arc::new(Mutex::new(Vec::new()));
    Token::registerModelEvent(
        &app,
        ModelEvent::Saving,
        Audit {
            label: "token",
            events: count.clone(),
            fail: false,
        },
    )
    .unwrap();
    in_app(&app, async {
        let repo = app.make::<ClientRepository>().await.unwrap();
        let client = repo
            .createClientCredentialsGrantClient("chunked")
            .await
            .unwrap();
        let client_id = client.getKey().await.unwrap();
        let rows = (0..1001)
            .map(|n| {
                Attributes::from([
                    ("id".into(), format!("chunk-{n:04}").into()),
                    ("client_id".into(), client_id.clone()),
                    ("revoked".into(), false.into()),
                ])
            })
            .collect::<Vec<_>>();
        db.table("oauth_access_tokens").insert(&rows).await.unwrap();
        repo.delete(&client).await.unwrap();
        assert_eq!(count.lock().unwrap().len(), 1001);
        let query = db
            .table("oauth_access_tokens")
            .r#where("revoked", "=", false);
        assert_eq!(query.count("*").await.unwrap(), 0);
    })
    .await;
    cleanup(&manager).await;
}

// Application-defined fixture routes exercise the public repository; these are not package routes.
fn manage(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::support::facades::Auth;
        let internal = |source| HttpError::Internal {
            source: Box::new(source),
        };
        Auth::once_basic(request, "email", &[]).await?;
        let user = Auth::user(request).await?.unwrap();
        let user = (user.as_ref() as &dyn std::any::Any)
            .downcast_ref::<ManagementUser>()
            .unwrap();
        let owner = openrust_passport::ClientOwner::for_user(user).map_err(internal)?;
        let repo = openrust::container::Container::get_instance()
            .make::<ClientRepository>()
            .unwrap();
        let Some(id) = request.route("client") else {
            let client = repo
                .createAuthorizationCodeGrantClient(
                    "HTTP client",
                    &["https://http.test/cb".into()],
                    true,
                    Some(&owner),
                    false,
                )
                .await
                .map_err(internal)?;
            return Response::json(
                StatusCode::CREATED,
                json!({"client":client.to_array().await.map_err(internal)?,"secret":client.getAttributeValue("plain_secret").await.map_err(internal)?.and_then(|v| if let Value::Text(s)=v {Some(s)} else {None})}),
            );
        };
        let Some(client) = repo.findForUser(id, &owner).await.map_err(internal)? else {
            return Response::json(StatusCode::NOT_FOUND, json!({"missing":true}));
        };
        match *request.method() {
            openrust::http::Method::PATCH => {
                assert!(
                    repo.update(&client, "HTTP renamed", &["https://http.test/new".into()])
                        .await
                        .map_err(internal)?
                );
            }
            openrust::http::Method::DELETE => {
                repo.delete(&client).await.map_err(internal)?;
            }
            openrust::http::Method::POST => {
                assert!(repo.regenerateSecret(&client).await.map_err(internal)?);
                return Response::json(
                    StatusCode::OK,
                    json!({"secret":client.getAttributeValue("plain_secret").await.map_err(internal)?.and_then(|v| if let Value::Text(s)=v {Some(s)} else {None})}),
                );
            }
            _ => {}
        }
        Response::json(
            StatusCode::OK,
            json!({"client":client.to_array().await.map_err(internal)?}),
        )
    })
}
async fn managed_app() -> (Root, Application, Arc<DatabaseManager>) {
    use openrust::contracts::hashing::Hasher;
    let root = Root::new();
    let app = root.app().await;
    migrate(&app).await;
    app.config()
        .unwrap()
        .set("auth.defaults.guard", json!("web"))
        .unwrap();
    app.config()
        .unwrap()
        .set(
            "auth.guards.web",
            json!({"driver":"session","provider":"users"}),
        )
        .unwrap();
    app.config()
        .unwrap()
        .set(
            "auth.providers.users",
            json!({"driver":"eloquent","model":"ManagementUser"}),
        )
        .unwrap();
    app.config()
        .unwrap()
        .set("session.driver", json!("array"))
        .unwrap();
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let main = manager.connection(Some("main")).unwrap();
    main.getSchemaBuilder()
        .create("management_users", |table| {
            table.id();
            table.bigInteger("subject");
            table.string("email", None);
            table.string("password", None);
        })
        .await
        .unwrap();
    let hasher = app.make::<dyn Hasher>().await.unwrap();
    let password = hasher
        .make(b"good password", &openrust::hashing::HashOptions::new())
        .await
        .unwrap();
    for (email, subject) in [("one@test", 991_i64), ("two@test", 992_i64)] {
        let mut user = ManagementUser::newInstanceOn(
            &main,
            Attributes::from([
                ("subject".into(), subject.into()),
                ("email".into(), email.into()),
                ("password".into(), password.clone().into()),
            ]),
        )
        .unwrap();
        user.save().await.unwrap();
    }
    app.make::<openrust::auth::AuthManager>()
        .await
        .unwrap()
        .register_model::<ManagementUser>("ManagementUser");
    (root, app, manager)
}
#[tokio::test]
async fn mysql_http_management_authenticates_owners_rejects_other_users_and_apps_and_reloads_revocation()
 {
    if !required() {
        return;
    }
    use openrust::contracts::hashing::Hasher;
    let (_root, app, manager) = managed_app().await;
    let (_other_root, other, other_manager) = managed_app().await;
    let hasher = app.make::<dyn Hasher>().await.unwrap();
    let (address, server) = serve(app).await;
    let (other_address, other_server) = serve(other).await;
    let http = reqwest::Client::new();
    let create = http
        .post(format!("http://{address}/manage"))
        .basic_auth("one@test", Some("good password"))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 201, "{}", create.text().await.unwrap());
    let body: Json = serde_json::from_str(&create.text().await.unwrap()).unwrap();
    assert!(body["client"].get("secret").is_none());
    let id = body["client"]["id"].as_str().unwrap();
    let secret = body["secret"].as_str().unwrap();
    let db = manager.connection(Some("oauth")).unwrap();
    let model = Client::queryOn(&db).find(id).await.unwrap().unwrap();
    let Some(Value::Text(hash)) = model.getAttribute("secret") else {
        panic!("missing hash")
    };
    assert!(
        hasher
            .check(
                secret.as_bytes(),
                hash,
                &openrust::hashing::HashOptions::new()
            )
            .await
            .unwrap()
    );
    assert!(
        !hasher
            .check(
                b"wrong secret",
                hash,
                &openrust::hashing::HashOptions::new()
            )
            .await
            .unwrap()
    );
    for host in [address, other_address] {
        for method in [
            reqwest::Method::GET,
            reqwest::Method::PATCH,
            reqwest::Method::DELETE,
        ] {
            let result = http
                .request(method, format!("http://{host}/manage/{id}"))
                .basic_auth("two@test", Some("good password"))
                .send()
                .await
                .unwrap();
            assert_eq!(result.status(), 404, "{}", result.text().await.unwrap());
        }
    }
    for method in [
        reqwest::Method::GET,
        reqwest::Method::PATCH,
        reqwest::Method::DELETE,
    ] {
        let response = http
            .request(method, format!("http://{other_address}/manage/{id}"))
            .basic_auth("one@test", Some("good password"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
    for (host, user) in [(address, "two@test"), (other_address, "one@test")] {
        let response = http
            .post(format!("http://{host}/manage/{id}/secret"))
            .basic_auth(user, Some("good password"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
    assert_eq!(
        http.post(format!("http://{address}/manage"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.get(format!("http://{address}/manage/not-a-uuid"))
            .basic_auth("one@test", Some("good password"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let bad = http
        .patch(format!("http://{address}/manage/{id}"))
        .basic_auth("one@test", Some("wrong password"))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 401);
    let updated = http
        .patch(format!("http://{address}/manage/{id}"))
        .basic_auth("one@test", Some("good password"))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200, "{}", updated.text().await.unwrap());
    let updated = Client::queryOn(&db).find(id).await.unwrap().unwrap();
    assert_eq!(
        updated.getAttribute("name"),
        Some(&Value::Text("HTTP renamed".into()))
    );
    assert_eq!(
        updated.redirectUris().unwrap().as_value(),
        &json!(["https://http.test/new"])
    );
    let rotated = http
        .post(format!("http://{address}/manage/{id}/secret"))
        .basic_auth("one@test", Some("good password"))
        .send()
        .await
        .unwrap();
    assert_eq!(rotated.status(), 200, "{}", rotated.text().await.unwrap());
    let rotated: Json = serde_json::from_str(&rotated.text().await.unwrap()).unwrap();
    let model = Client::queryOn(&db).find(id).await.unwrap().unwrap();
    let Some(Value::Text(hash)) = model.getAttribute("secret") else {
        panic!("missing hash")
    };
    assert!(
        hasher
            .check(
                rotated["secret"].as_str().unwrap().as_bytes(),
                hash,
                &openrust::hashing::HashOptions::new()
            )
            .await
            .unwrap()
    );
    assert!(
        !hasher
            .check(
                secret.as_bytes(),
                hash,
                &openrust::hashing::HashOptions::new()
            )
            .await
            .unwrap()
    );
    let mut token = Token::newInstanceOn(
        &db,
        Attributes::from([
            ("id".into(), "http-t".into()),
            ("client_id".into(), id.into()),
            ("revoked".into(), false.into()),
        ]),
    )
    .unwrap();
    token.save().await.unwrap();
    let deleted = http
        .delete(format!("http://{address}/manage/{id}"))
        .basic_auth("one@test", Some("good password"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200, "{}", deleted.text().await.unwrap());
    assert_eq!(
        Token::queryOn(&db)
            .find("http-t")
            .await
            .unwrap()
            .unwrap()
            .getAttributeValue("revoked")
            .unwrap(),
        Some(true.into())
    );
    let missing = http
        .get(format!("http://{address}/manage/{id}"))
        .basic_auth("one@test", Some("good password"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    server.abort();
    other_server.abort();
    for manager in [manager, other_manager] {
        manager
            .connection(Some("main"))
            .unwrap()
            .getSchemaBuilder()
            .dropIfExists("management_users")
            .await
            .unwrap();
        cleanup(&manager).await;
    }
}

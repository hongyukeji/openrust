#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{
    config::{ConfigError, Environment, Value as Json},
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseManager, DatabaseResult, Value,
        eloquent::{Attributes, Builder, Model, ModelMetadata, ModelState, Scope, UniqueId},
        migrations::{MigrationEntry, Migrator, RunOptions},
    },
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_passport::{
    Client, ClientModel, ClientRepository, Passport, PassportServiceProvider, migrations,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
const ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const MISSING: &str = "550e8400-e29b-41d4-a716-446655440001";
static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "database/migrations/2016_06_01_000004_create_oauth_clients_table.rs",
    &migrations::oauth_clients::Definition,
)];
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "passport-lookup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("storage")).unwrap();
        Self(root)
    }
    async fn application(&self) -> Application {
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
fn configuration(_: &Path, _: &Environment) -> Result<BTreeMap<String, Json>, ConfigError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let prefix = format!(
        "lookup_{}_{}_",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL")
        .unwrap_or_else(|_| "mysql://unused@127.0.0.1:9/unused".into());
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing","host":"127.0.0.1","port":0}),
        ),
        (
            "database".into(),
            json!({"default":"main","connections":{"main":{"url":url,"prefix":format!("{prefix}main_")},"oauth":{"url":url,"prefix":format!("{prefix}oauth_")}},"migrations":{"table":"history"}}),
        ),
        ("passport".into(), json!({"connection":"oauth"})),
        (
            "hashing".into(),
            json!({"driver":"bcrypt","bcrypt":{"rounds":4}}),
        ),
    ]))
}
fn mysql_required() -> bool {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_some() {
        return true;
    }
    assert!(
        std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
        "required MySQL URL missing"
    );
    eprintln!("SKIP MySQL client lookup: URL missing");
    false
}
async fn migrate(app: &Application) {
    let rows = app
        .call(
            openrust::container::Callback::new(&[], |_| async {
                let rows = app
                    .make::<Migrator>()
                    .await?
                    .run(None, &[], RunOptions::default())
                    .await
                    .unwrap();
                Ok(rows)
            }),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}
async fn insert(db: &Connection, id: &str, name: &str, revoked: bool) {
    let rows = vec![Attributes::from([
        ("id".into(), id.into()),
        ("name".into(), name.into()),
        ("secret".into(), Value::Null),
        ("redirect_uris".into(), "[]".into()),
        (
            "grant_types".into(),
            "[\"authorization_code\",\"refresh_token\"]".into(),
        ),
        ("revoked".into(), revoked.into()),
    ])];
    db.table("oauth_clients").insert(&rows).await.unwrap();
}
async fn cleanup(app: &Application) {
    let manager = app.make::<DatabaseManager>().await.unwrap();
    manager
        .connection(Some("oauth"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("oauth_clients")
        .await
        .unwrap();
    manager
        .connection(Some("main"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("history")
        .await
        .unwrap();
}
#[tokio::test]
async fn invalid_identifiers_do_not_resolve_a_connection_or_query() {
    let root = Root::new();
    let app = root.application().await;
    let config = app.config().unwrap();
    config
        .set("passport.connection", json!(["invalid"]))
        .unwrap();
    let repo = app.make::<ClientRepository>().await.unwrap();
    for id in [
        Value::from(""),
        Value::from("x"),
        Value::from("1"),
        Value::from(1_i64),
        Value::from("550e8400-e29b-41d4-a716-44665544000x"),
    ] {
        assert!(repo.find(id).await.unwrap().is_none());
    }
    assert!(repo.find(ID).await.is_err());
}
#[tokio::test]
async fn mysql_lookup_retries_errors_shares_models_and_isolates_request_caches() {
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let db = manager.connection(Some("oauth")).unwrap();
    let version = db.select("SELECT VERSION() AS version", &[]).await.unwrap();
    assert!(
        matches!(version[0].get("version"),Some(Value::Text(version)) if version.starts_with("8.4."))
    );
    let root_repo = app.make::<ClientRepository>().await.unwrap();
    assert!(root_repo.find(ID).await.is_err());
    migrate(&app).await;
    insert(&db, ID, "actual model", false).await;
    assert!(
        root_repo.find(ID).await.unwrap().is_some(),
        "query failures must not become cached misses"
    );
    db.enableQueryLog();
    db.flushQueryLog();
    app.container()
        .scope(async {
            let first = app.make::<ClientRepository>().await.unwrap();
            let same = app.make::<ClientRepository>().await.unwrap();
            assert!(Arc::ptr_eq(&first, &same));
            assert!(!Arc::ptr_eq(&first, &root_repo));
            let (a, b) = tokio::join!(first.find(ID), same.find(ID));
            let a = a.unwrap().unwrap();
            let b = b.unwrap().unwrap();
            assert!(a.sameInstance(&b));
            assert_eq!(db.getQueryLog().len(), 1);
            let model = a.model::<Client>().unwrap();
            model
                .lock()
                .await
                .setAttributeValue("revoked", true)
                .unwrap();
            assert!(first.findActive(ID).await.unwrap().is_none());
            assert_eq!(db.getQueryLog().len(), 1);
            assert!(first.find(MISSING).await.unwrap().is_none());
            insert(&db, MISSING, "created after miss", false).await;
            let before = db.getQueryLog().len();
            assert!(same.find(MISSING).await.unwrap().is_none());
            assert_eq!(db.getQueryLog().len(), before);
        })
        .await;
    app.container()
        .scope(async {
            let fresh = app.make::<ClientRepository>().await.unwrap();
            assert!(
                fresh.findActive(ID).await.unwrap().is_some(),
                "unsaved mutation must stay in previous request"
            );
            assert!(
                fresh.find(MISSING).await.unwrap().is_some(),
                "next request must not reuse a cached miss"
            );
        })
        .await;
    cleanup(&app).await;
}
struct CustomClient(ModelState);
struct Visible;
impl Scope<CustomClient> for Visible {
    fn apply(&self, query: &mut Builder<'_, CustomClient>) -> DatabaseResult<()> {
        query.r#where("revoked", "=", false);
        Ok(())
    }
}
impl Model for CustomClient {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("ulid_clients")
            .connection("main")
            .uniqueId(UniqueId::Ulid)
            .guarded(&[]);
        &META
    }
    fn globalScopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("visible", &Visible)]
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        Client::casts()
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
impl ClientModel for CustomClient {}
struct Retrieved(Arc<AtomicUsize>);
impl openrust::database::eloquent::concerns::ModelEventListener<CustomClient> for Retrieved {
    fn handle<'a>(
        &'a self,
        event: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut CustomClient,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        Box::pin(async move {
            assert_eq!(
                event,
                openrust::database::eloquent::concerns::ModelEvent::Retrieved
            );
            self.0.fetch_add(1, Ordering::Relaxed);
            model
                .state_mut()
                .setAttribute("name", "retrieved through model event");
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}

#[tokio::test]
async fn mysql_custom_models_keep_unique_id_validation_scopes_and_explicit_connection() {
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let db = manager.connection(Some("main")).unwrap();
    db.getSchemaBuilder()
        .create("ulid_clients", |table| {
            table.char("id", 26).primary();
            table.string("name", None);
            table.boolean("revoked");
        })
        .await
        .unwrap();
    for (id, revoked) in [
        ("01ARZ3NDEKTSV4RRFFQ69G5FAV", false),
        ("01ARZ3NDEKTSV4RRFFQ69G5FAW", true),
    ] {
        db.table("ulid_clients")
            .insert(&[Attributes::from([
                ("id".into(), id.into()),
                ("name".into(), "custom".into()),
                ("revoked".into(), revoked.into()),
            ])])
            .await
            .unwrap();
    }
    let passport = app.make::<Passport>().await.unwrap();
    passport.useClientModel::<CustomClient>();
    let observed = Arc::new(AtomicUsize::new(0));
    CustomClient::registerModelEvent(
        &app,
        openrust::database::eloquent::concerns::ModelEvent::Retrieved,
        Retrieved(observed.clone()),
    )
    .unwrap();
    assert!(passport.clientModel().ends_with("CustomClient"));
    db.enableQueryLog();
    db.flushQueryLog();
    app.container()
        .scope(async {
            let repo = app.make::<ClientRepository>().await.unwrap();
            assert!(repo.find(ID).await.unwrap().is_none());
            assert!(db.getQueryLog().is_empty());
            let visible = repo
                .find("01ARZ3NDEKTSV4RRFFQ69G5FAV")
                .await
                .unwrap()
                .unwrap();
            assert!(visible.model::<CustomClient>().is_some());
            assert_eq!(
                visible.getAttributeValue("name").await.unwrap(),
                Some("retrieved through model event".into())
            );
            assert_eq!(observed.load(Ordering::Relaxed), 1);
            assert!(
                repo.find("01ARZ3NDEKTSV4RRFFQ69G5FAV")
                    .await
                    .unwrap()
                    .unwrap()
                    .sameInstance(&visible)
            );
            assert_eq!(observed.load(Ordering::Relaxed), 1);
            assert!(visible.model::<Client>().is_none());
            assert!(
                repo.find("01ARZ3NDEKTSV4RRFFQ69G5FAW")
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(db.getQueryLog().len(), 2);
        })
        .await;
    db.getSchemaBuilder()
        .dropIfExists("ulid_clients")
        .await
        .unwrap();
}
fn routes(_: &Application) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut routes = openrust::routing::Router::new();
    routes.get("/lookup/{id}", lookup)?;
    routes.get("/model-connections", model_connections)?;
    routes.get("/scope-oracle", scope_oracle)?;
    routes.get("/factory-states", factory_states)?;
    routes.get("/factory-create", factory_create)?;
    Ok(routes)
}
fn lookup(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let internal =
            |source: Box<dyn std::error::Error + Send + Sync>| HttpError::Internal { source };
        let repo = openrust::container::Container::get_instance()
            .make::<ClientRepository>()
            .map_err(|e| internal(Box::new(e)))?;
        let id = request.route("id").unwrap();
        let Some(a) = repo.find(id).await.map_err(|e| internal(Box::new(e)))? else {
            return Ok(Response::new(
                StatusCode::NOT_FOUND,
                openrust::http::Body::from("missing"),
            ));
        };
        let b = repo
            .find(id)
            .await
            .map_err(|e| internal(Box::new(e)))?
            .unwrap();
        Response::json(
            StatusCode::OK,
            json!({"client":a.to_array().await.map_err(|e|internal(Box::new(e)))?,"same":a.sameInstance(&b),"scope_allowed":a.hasScope("orders:read").await.map_err(|e|internal(Box::new(e)))?}),
        )
    })
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
#[tokio::test]
async fn mysql_http_lookup_keeps_two_apps_and_successive_requests_isolated() {
    if !mysql_required() {
        return;
    }
    let root1 = Root::new();
    let root2 = Root::new();
    let first = root1.application().await;
    let second = root2.application().await;
    migrate(&first).await;
    migrate(&second).await;
    let manager1 = first.make::<DatabaseManager>().await.unwrap();
    let manager2 = second.make::<DatabaseManager>().await.unwrap();
    let db1 = manager1.connection(Some("oauth")).unwrap();
    let db2 = manager2.connection(Some("oauth")).unwrap();
    insert(&db1, ID, "first app", false).await;
    insert(&db2, ID, "second app", false).await;
    db1.enableQueryLog();
    db2.enableQueryLog();
    db1.flushQueryLog();
    db2.flushQueryLog();
    let (a, b) = tokio::join!(serve(first), serve(second));
    let client = reqwest::Client::new();
    for _ in 0..2 {
        let (one, two) = tokio::join!(
            client.get(format!("http://{}/lookup/{ID}", a.0)).send(),
            client.get(format!("http://{}/lookup/{ID}", b.0)).send()
        );
        for (response, name) in [(one.unwrap(), "first app"), (two.unwrap(), "second app")] {
            assert_eq!(response.status(), 200);
            let body: Json = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            assert_eq!(body["client"]["name"], name);
            assert_eq!(body["same"], true);
            assert_eq!(body["scope_allowed"], true);
            assert!(body["client"].get("secret").is_none());
        }
    }
    assert_eq!(db1.getQueryLog().len(), 2);
    assert_eq!(db2.getQueryLog().len(), 2);
    let missing = client
        .get(format!("http://{}/lookup/{MISSING}", a.0))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    a.1.abort();
    b.1.abort();
    let _ = a.1.await;
    let _ = b.1.await;
    for manager in [manager1, manager2] {
        manager
            .connection(Some("oauth"))
            .unwrap()
            .getSchemaBuilder()
            .dropIfExists("oauth_clients")
            .await
            .unwrap();
        manager
            .connection(Some("main"))
            .unwrap()
            .getSchemaBuilder()
            .dropIfExists("history")
            .await
            .unwrap();
    }
}

struct OwnerUser(ModelState);
impl Model for OwnerUser {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("users").guarded(&[]);
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
impl openrust::contracts::auth::Authenticatable for OwnerUser {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        openrust::auth::AuthIdentifier::Integer(991)
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}

struct Creating(Arc<AtomicUsize>);
impl openrust::database::eloquent::concerns::ModelEventListener<Client> for Creating {
    fn handle<'a>(
        &'a self,
        event: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut Client,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        Box::pin(async move {
            assert_eq!(
                event,
                openrust::database::eloquent::concerns::ModelEvent::Creating
            );
            assert_eq!(model.plainSecret.is_some(), model.confidential());
            if let Some(plain) = &model.plainSecret {
                assert_eq!(plain.len(), 40);
                assert!(
                    matches!(model.getAttribute("secret"),Some(Value::Text(hash)) if hash.starts_with("$2y$04$"))
                );
            }
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}
#[tokio::test]
async fn mysql_creation_update_and_secret_rotation_keep_actual_eloquent_models() {
    if !mysql_required() {
        return;
    }
    use openrust::contracts::hashing::Hasher;
    let root = Root::new();
    let app = root.application().await;
    migrate(&app).await;
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("oauth"))
        .unwrap();
    let hasher = app.make::<dyn Hasher>().await.unwrap();
    let creations = Arc::new(AtomicUsize::new(0));
    Client::registerModelEvent(
        &app,
        openrust::database::eloquent::concerns::ModelEvent::Creating,
        Creating(creations.clone()),
    )
    .unwrap();
    let user =
        OwnerUser::newInstanceOn(&db, Attributes::from([("id".into(), 7_i64.into())])).unwrap();
    let owner = openrust_passport::ClientOwner::for_user(&user).unwrap();
    app.container()
        .scope(async {
            let repo = app.make::<ClientRepository>().await.unwrap();
            let redirects = vec![
                "https://app.test/callback".to_owned(),
                "https://other.test/callback".to_owned(),
            ];
            let created = repo
                .createAuthorizationCodeGrantClient(
                    "owned app",
                    &redirects,
                    true,
                    Some(&owner),
                    true,
                )
                .await
                .unwrap();
            let model = created.model::<Client>().unwrap();
            let secret = model.lock().await.plainSecret.clone().unwrap();
            assert_eq!(secret.len(), 40);
            assert!(created.confidential().await);
            assert!(!created.firstParty().await.unwrap());
            assert!(
                created
                    .hasGrantType("urn:ietf:params:oauth:grant-type:device_code")
                    .await
                    .unwrap()
            );
            let id = created.getKey().await.unwrap();
            assert!(matches!(&id,Value::Text(id) if Client::isValidUniqueId(id)));
            let json = created.to_array().await.unwrap();
            assert_eq!(json["owner_id"], 7);
            assert_eq!(json["owner_type"], user.getMorphClass().unwrap());
            assert!(json.get("secret").is_none());
            let loaded = repo.find(id.clone()).await.unwrap().unwrap();
            assert!(!created.sameInstance(&loaded));
            assert!(
                loaded
                    .model::<Client>()
                    .unwrap()
                    .lock()
                    .await
                    .plainSecret
                    .is_none()
            );
            let Some(Value::Text(hash)) = loaded.getAttributeValue("secret").await.unwrap() else {
                panic!("missing persisted hash")
            };
            assert!(
                hasher
                    .check(secret.as_bytes(), &hash, &Default::default())
                    .await
                    .unwrap()
            );
            assert!(
                repo.update(&loaded, "renamed app", &["https://new.test/cb".into()])
                    .await
                    .unwrap()
            );
            assert_eq!(
                loaded.getAttributeValue("name").await.unwrap(),
                Some("renamed app".into())
            );
            assert_eq!(
                loaded.redirectUris().await.unwrap().as_value(),
                &json!(["https://new.test/cb"])
            );
            assert!(repo.regenerateSecret(&loaded).await.unwrap());
            let rotated = loaded
                .model::<Client>()
                .unwrap()
                .lock()
                .await
                .plainSecret
                .clone()
                .unwrap();
            assert!(rotated.as_str() != secret.as_str());
            let fresh = Client::queryOn(&db)
                .find(id.clone())
                .await
                .unwrap()
                .unwrap();
            assert!(fresh.plainSecret.is_none());
            let Some(Value::Text(new_hash)) = fresh.getAttribute("secret") else {
                panic!("missing rotated hash")
            };
            assert!(
                hasher
                    .check(rotated.as_bytes(), new_hash, &Default::default())
                    .await
                    .unwrap()
            );
            assert!(
                !hasher
                    .check(secret.as_bytes(), new_hash, &Default::default())
                    .await
                    .unwrap()
            );
            let same = repo.find(id).await.unwrap().unwrap();
            assert!(same.sameInstance(&loaded));
            let public = repo
                .createAuthorizationCodeGrantClient("public app", &redirects, false, None, false)
                .await
                .unwrap();
            assert!(!public.confidential().await);
            assert!(
                !public
                    .hasGrantType("urn:ietf:params:oauth:grant-type:device_code")
                    .await
                    .unwrap()
            );
            let device = repo
                .createDeviceAuthorizationGrantClient("device", false, None)
                .await
                .unwrap();
            assert!(!device.confidential().await);
            assert!(device.hasGrantType("refresh_token").await.unwrap());
            let implicit = repo
                .createImplicitGrantClient("implicit", &redirects)
                .await
                .unwrap();
            assert!(!implicit.confidential().await);
            assert!(implicit.hasGrantType("implicit").await.unwrap());
            assert!(!implicit.hasGrantType("refresh_token").await.unwrap());
            let password = repo
                .createPasswordGrantClient("password", Some("users"), false)
                .await
                .unwrap();
            assert!(!password.confidential().await);
            assert!(password.hasGrantType("password").await.unwrap());
            assert_eq!(
                password.getAttributeValue("provider").await.unwrap(),
                Some("users".into())
            );
            let personal = repo
                .createPersonalAccessGrantClient("personal", Some("users"))
                .await
                .unwrap();
            assert!(personal.confidential().await);
            assert!(personal.hasGrantType("personal_access").await.unwrap());
            let machine = repo
                .createClientCredentialsGrantClient("machine")
                .await
                .unwrap();
            assert!(machine.confidential().await);
            assert!(machine.hasGrantType("client_credentials").await.unwrap());
            assert!(Client::queryOn(&db).get().await.unwrap().len() == 7);
            assert_eq!(creations.load(Ordering::Relaxed), 7);
            let own_id = loaded.getKey().await.unwrap();
            assert!(
                repo.findForUser(own_id.clone(), &owner)
                    .await
                    .unwrap()
                    .is_some()
            );
            let other_user =
                OwnerUser::newInstanceOn(&db, Attributes::from([("id".into(), 8_i64.into())]))
                    .unwrap();
            let other_owner = openrust_passport::ClientOwner::for_user(&other_user).unwrap();
            assert!(
                repo.findForUser(own_id.clone(), &other_owner)
                    .await
                    .unwrap()
                    .is_none()
            );
            let owned = repo.for_user(&owner).await.unwrap();
            assert_eq!(owned.count(), 1);
            assert_eq!(owned.first().unwrap().getKey().await, Some(own_id.clone()));
            db.table("oauth_clients")
                .r#where("id", "=", own_id)
                .update(Attributes::from([("revoked".into(), true.into())]))
                .await
                .unwrap();
            assert!(repo.for_user(&owner).await.unwrap().is_empty());
            app.config()
                .unwrap()
                .set("auth.guards.api.provider", json!("users"))
                .unwrap();
            let legacy_pat = repo
                .createPersonalAccessGrantClient("default provider", None)
                .await
                .unwrap();
            let other_pat = repo
                .createPersonalAccessGrantClient("other provider", Some("admins"))
                .await
                .unwrap();
            for (record, created) in [
                (&personal, "2020-01-01 00:00:00"),
                (&legacy_pat, "2021-01-01 00:00:00"),
                (&other_pat, "2022-01-01 00:00:00"),
                (&password, "2023-01-01 00:00:00"),
            ] {
                db.table("oauth_clients")
                    .r#where("id", "=", record.getKey().await.unwrap())
                    .update(Attributes::from([("created_at".into(), created.into())]))
                    .await
                    .unwrap();
            }
            let selected = repo.personalAccessClient("users").await.unwrap();
            assert_eq!(selected.getKey().await, legacy_pat.getKey().await);
            assert_eq!(
                repo.personalAccessClient("admins")
                    .await
                    .unwrap()
                    .getKey()
                    .await,
                other_pat.getKey().await
            );
            let missing = repo.personalAccessClient("missing").await.unwrap_err();
            assert_eq!(
                missing.to_string(),
                "Personal access client not found for 'missing' user provider. Please create one."
            );
        })
        .await;
    cleanup(&app).await;
}

struct DatedClient(ModelState);
impl Model for DatedClient {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("dated_clients")
            .connection("main")
            .uniqueId(UniqueId::Uuid)
            .timestamps(Some("born_at"), None)
            .guarded(&[]);
        &META
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        Client::casts()
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
impl ClientModel for DatedClient {}
#[tokio::test]
async fn mysql_personal_client_selection_uses_the_selected_models_timestamp_column() {
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("main"))
        .unwrap();
    db.getSchemaBuilder()
        .create("dated_clients", |table| {
            table.char("id", 36).primary();
            table.string("provider", None).nullable();
            table.boolean("revoked");
            table.text("grant_types");
            table.dateTime("born_at", None);
        })
        .await
        .unwrap();
    for (id, date) in [
        (ID, "2021-01-01 00:00:00"),
        (MISSING, "2022-01-01 00:00:00"),
    ] {
        db.table("dated_clients")
            .insert(&[Attributes::from([
                ("id".into(), id.into()),
                ("provider".into(), "users".into()),
                ("revoked".into(), false.into()),
                ("grant_types".into(), "[\"personal_access\"]".into()),
                ("born_at".into(), date.into()),
            ])])
            .await
            .unwrap();
    }
    app.make::<Passport>()
        .await
        .unwrap()
        .useClientModel::<DatedClient>();
    app.container()
        .scope(async {
            let repo = app.make::<ClientRepository>().await.unwrap();
            let client = repo.personalAccessClient("users").await.unwrap();
            assert!(client.model::<DatedClient>().is_some());
            assert_eq!(client.getKey().await, Some(MISSING.into()));
        })
        .await;
    db.getSchemaBuilder()
        .dropIfExists("dated_clients")
        .await
        .unwrap();
}

#[path = "support/client_example.rs"]
mod documented;

#[tokio::test]
async fn mysql_documented_client_helper_runs_inside_its_application_lifecycle() {
    let snippet = include_str!("support/client_example.rs").trim();
    for page in [
        include_str!("../../../../docs/en/packages/passport.md"),
        include_str!("../../../../docs/zh-Hans/packages/passport.md"),
        include_str!("../../../../docs/zh-Hant/packages/passport.md"),
    ] {
        assert!(page.contains(&format!("```rust\n{snippet}\n```")));
    }
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    migrate(&app).await;
    app.container()
        .scope(async {
            let record = documented::create_client(&app, None).await.unwrap();
            assert!(record.confidential().await);
            assert!(record.firstParty().await.unwrap());
            assert_eq!(
                record.redirectUris().await.unwrap().as_value(),
                &json!(["https://example.com/callback"])
            );
            let repo = app.make::<ClientRepository>().await.unwrap();
            let loaded = repo
                .findActive(record.getKey().await.unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                loaded.getAttributeValue("name").await.unwrap(),
                Some("My application".into())
            );
            assert!(
                loaded
                    .model::<Client>()
                    .unwrap()
                    .lock()
                    .await
                    .plainSecret
                    .is_none()
            );
        })
        .await;
    cleanup(&app).await;
}

#[tokio::test]
async fn mysql_legacy_client_columns_keep_owners_redirects_and_secret_mutators() {
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("oauth"))
        .unwrap();
    db.getSchemaBuilder()
        .create("oauth_clients", |table| {
            table.uuid("id").primary();
            table.bigInteger("user_id").nullable();
            table.string("name", None);
            table.string("secret", None).nullable();
            table.string("provider", None).nullable();
            table.text("redirect");
            table.boolean("personal_access_client");
            table.boolean("password_client");
            table.boolean("revoked");
            table.timestamps();
        })
        .await
        .unwrap();
    let user =
        OwnerUser::newInstanceOn(&db, Attributes::from([("id".into(), 7_i64.into())])).unwrap();
    let owner = openrust_passport::ClientOwner::for_user(&user).unwrap();
    app.container()
        .scope(async {
            let repo = app.make::<ClientRepository>().await.unwrap();
            let created = repo
                .createAuthorizationCodeGrantClient(
                    "legacy owner",
                    &["https://old.test/a".into(), "https://old.test/b".into()],
                    true,
                    Some(&owner),
                    false,
                )
                .await
                .unwrap();
            assert_eq!(
                created.getAttributeValue("user_id").await.unwrap(),
                Some(7_i64.into())
            );
            assert!(!created.firstParty().await.unwrap());
            assert!(created.hasGrantType("authorization_code").await.unwrap());
            assert_eq!(
                created.redirectUris().await.unwrap().as_value(),
                &json!(["https://old.test/a", "https://old.test/b"])
            );
            assert!(
                repo.update(&created, "legacy renamed", &["https://new.test/cb".into()])
                    .await
                    .unwrap()
            );
            let loaded = repo
                .find(created.getKey().await.unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                loaded.redirectUris().await.unwrap().as_value(),
                &json!(["https://new.test/cb"])
            );
            assert!(
                loaded
                    .model::<Client>()
                    .unwrap()
                    .lock()
                    .await
                    .plainSecret
                    .is_none()
            );
            assert_eq!(repo.for_user(&owner).await.unwrap().count(), 1);
            let personal = repo
                .createPersonalAccessGrantClient("legacy personal", None)
                .await
                .unwrap();
            assert!(personal.hasGrantType("personal_access").await.unwrap());
            let password = repo
                .createPasswordGrantClient("legacy password", Some("users"), true)
                .await
                .unwrap();
            assert!(password.hasGrantType("password").await.unwrap());
            assert!(!password.hasGrantType("personal_access").await.unwrap());
        })
        .await;
    cleanup(&app).await;
}

fn model_connections(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let result = verify_model_connections().await.map_err(|source| {
            eprintln!("direct client connection probe failed: {source}");
            HttpError::Internal {
                source: Box::new(source),
            }
        })?;
        Response::json(StatusCode::OK, result)
    })
}

async fn verify_model_connections() -> DatabaseResult<Json> {
    use openrust::support::facades::Config;
    let configured = Client::default().getConnection()?;
    assert_eq!(configured.get_name(), "oauth");
    assert_eq!(
        CustomClient::from_state(Default::default())
            .getConnection()?
            .get_name(),
        "main"
    );
    let mut client = Client::default();
    client.forceFill(Attributes::from([
        ("name".into(), "direct model".into()),
        ("redirect_uris".into(), Value::Json(json!([]))),
        (
            "grant_types".into(),
            Value::Json(json!(["authorization_code"])),
        ),
        ("revoked".into(), false.into()),
    ]))?;
    assert!(client.save().await?);
    let key = client.getKey().unwrap().clone();
    let mut loaded = Client::find(key.clone()).await?.unwrap();
    assert_eq!(
        loaded.getAttributeValue("name")?,
        Some("direct model".into())
    );

    let manager = openrust::container::Container::get_instance()
        .make::<DatabaseManager>()
        .map_err(openrust::database::DatabaseError::Unavailable)?;
    let main = manager.connection(Some("main"))?;
    let mut wrong = main.beginTransaction().await?;
    let mut unbound = Client::default();
    assert!(matches!(
        unbound.saveInTransaction(&mut wrong).await,
        Err(openrust::database::DatabaseError::InvalidQuery(
            "model transaction connection"
        ))
    ));
    wrong.rollBack(None).await?;
    let mut correct = configured.beginTransaction().await?;
    unbound.forceFill(Attributes::from([
        ("name".into(), "rolled back".into()),
        ("redirect_uris".into(), Value::Json(json!([]))),
        (
            "grant_types".into(),
            Value::Json(json!(["authorization_code"])),
        ),
        ("revoked".into(), false.into()),
    ]))?;
    assert!(unbound.saveInTransaction(&mut correct).await?);
    correct.rollBack(None).await?;

    Config::set("passport.connection", json!("main")).unwrap();
    loaded.setAttributeValue("name", "retained original connection")?;
    assert!(loaded.save().await?);
    assert_eq!(loaded.getConnection()?.get_name(), "oauth");
    assert_eq!(Client::default().getConnection()?.get_name(), "main");
    assert_eq!(
        Client::queryOn(&configured)
            .find(key.clone())
            .await?
            .unwrap()
            .getAttributeValue("name")?,
        Some("retained original connection".into())
    );
    Config::set("passport.connection", Json::Null).unwrap();
    assert_eq!(Client::default().getConnection()?.get_name(), "main");
    Config::set("passport.connection", json!(["invalid"])).unwrap();
    assert!(matches!(
        Client::default().getConnection(),
        Err(openrust::database::DatabaseError::InvalidConfiguration(
            "passport.connection"
        ))
    ));
    assert!(Client::query().is_err());
    assert_eq!(loaded.getConnection()?.get_name(), "oauth");
    Config::set("passport.connection", json!("oauth")).unwrap();
    assert!(loaded.delete().await?);
    assert!(Client::find(key).await?.is_none());
    Ok(
        json!({"configured":"oauth", "explicit":"main", "bound":"oauth",
        "transaction_mismatch_rejected":true, "default":"main"}),
    )
}

#[tokio::test]
async fn mysql_direct_client_models_resolve_configuration_and_preserve_bound_transactions_over_http()
 {
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    migrate(&app).await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let (address, task) = serve(app).await;
    let response = reqwest::get(format!("http://{address}/model-connections"))
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    task.abort();
    let _ = task.await;
    manager
        .connection(Some("oauth"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("oauth_clients")
        .await
        .unwrap();
    manager
        .connection(Some("main"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("history")
        .await
        .unwrap();
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Json>(&body).unwrap()["transaction_mismatch_rejected"],
        true
    );
}

fn scope_oracle(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let passport = openrust::container::Container::get_instance()
            .make::<Passport>()
            .unwrap();
        let inherited = passport.withInheritedScopes.load(Ordering::Relaxed);
        let fixture: Json =
            serde_json::from_str(include_str!("fixtures/inherited_scopes.json")).unwrap();
        let mut results = Vec::new();
        for case in fixture["cases"].as_array().unwrap() {
            if case["inherited"] != inherited {
                continue;
            }
            let mut state = ModelState::default();
            state.setAttribute("scopes", Value::Json(case["scopes"].clone()));
            let client = Client::from_state(state);
            results.push(client.hasScope(case["scope"].as_str().unwrap()).unwrap());
        }
        let mut invalid = Client::default();
        invalid.state_mut().setAttribute("scopes", "null");
        assert!(invalid.hasScope("orders").is_err());
        invalid.state_mut().setAttribute("scopes", "12");
        assert!(invalid.hasScope("orders").is_err());
        Response::json(StatusCode::OK, json!(results))
    })
}

#[tokio::test]
async fn inherited_scope_php_oracle_runs_over_http_and_remains_application_local() {
    let first_root = Root::new();
    let second_root = Root::new();
    let first = first_root.application().await;
    let second = second_root.application().await;
    let first_passport = first.make::<Passport>().await.unwrap();
    let second_passport = second.make::<Passport>().await.unwrap();
    assert!(!first_passport.withInheritedScopes.load(Ordering::Relaxed));
    second_passport
        .withInheritedScopes
        .store(true, Ordering::Relaxed);
    let a = serve(first).await;
    let b = serve(second).await;
    let fixture: Json =
        serde_json::from_str(include_str!("fixtures/inherited_scopes.json")).unwrap();
    assert_eq!(
        fixture["passport"],
        "f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7"
    );
    for _ in 0..2 {
        let (one, two) = tokio::join!(
            reqwest::get(format!("http://{}/scope-oracle", a.0)),
            reqwest::get(format!("http://{}/scope-oracle", b.0))
        );
        for (response, inherited) in [(one.unwrap(), false), (two.unwrap(), true)] {
            assert_eq!(response.status(), 200);
            let values: Vec<bool> = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            let expected = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|case| case["inherited"] == inherited)
                .map(|case| case["allowed"].as_bool().unwrap())
                .collect::<Vec<_>>();
            assert!(!expected.is_empty());
            assert_eq!(values, expected);
        }
    }
    for task in [a.1, b.1] {
        task.abort();
        let _ = task.await;
    }
    assert!(!first_passport.withInheritedScopes.load(Ordering::Relaxed));
    assert!(second_passport.withInheritedScopes.load(Ordering::Relaxed));
}

fn factory_states(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust_passport::database::factories::ClientFactory;
        let factory = Client::factory().unwrap();
        assert_eq!(factory.getConnectionName(), Some("oauth"));
        assert_eq!(factory.modelName(), std::any::type_name::<Client>());
        let raw = factory.raw().await.unwrap();
        assert_eq!(raw["owner_id"], Value::Null);
        assert_eq!(raw["owner_type"], Value::Null);
        assert_eq!(raw["revoked"], false.into());
        let Value::Text(secret) = &raw["secret"] else {
            panic!("secret")
        };
        assert_eq!(secret.len(), 40);
        let Value::Text(name) = &raw["name"] else {
            panic!("name")
        };
        assert!(!name.is_empty());
        for (factory, grants, empty, public) in [
            (
                Client::factory().unwrap(),
                json!(["authorization_code", "refresh_token"]),
                false,
                false,
            ),
            (
                Client::factory().unwrap().asPublic(),
                json!(["authorization_code", "refresh_token"]),
                false,
                true,
            ),
            (
                Client::factory().unwrap().asPasswordClient(),
                json!(["password", "refresh_token"]),
                true,
                false,
            ),
            (
                Client::factory().unwrap().asPersonalAccessTokenClient(),
                json!(["personal_access"]),
                true,
                false,
            ),
            (
                Client::factory().unwrap().asImplicitClient(),
                json!(["implicit"]),
                false,
                true,
            ),
            (
                Client::factory().unwrap().asClientCredentials(),
                json!(["client_credentials"]),
                true,
                false,
            ),
            (
                Client::factory().unwrap().asDeviceCodeClient(),
                json!([
                    "urn:ietf:params:oauth:grant-type:device_code",
                    "refresh_token"
                ]),
                true,
                false,
            ),
        ] {
            let model = factory.make().await.unwrap();
            assert_eq!(model.grantTypes().unwrap().as_value(), &grants);
            assert_eq!(model.redirectUris().unwrap().is_empty(), empty);
            assert_eq!(model.confidential(), !public);
            assert_eq!(model.plainSecret.is_none(), public);
            assert!(!model.state().exists());
            if !public {
                let plain = model.plainSecret.as_ref().unwrap();
                assert_eq!(plain.len(), 40);
                assert_ne!(
                    model.state().getAttribute("secret"),
                    Some(&Value::Text(plain.to_string()))
                );
            }
        }
        let model = Client::factory()
            .unwrap()
            .asImplicitClient()
            .asPasswordClient()
            .state(|_| Attributes::from([("name".into(), "overridden".into())]))
            .make()
            .await
            .unwrap();
        assert_eq!(
            model.getAttributeValue("name").unwrap(),
            Some("overridden".into())
        );
        assert_eq!(
            model.grantTypes().unwrap().as_value(),
            &json!(["password", "refresh_token"])
        );
        assert!(!model.confidential());
        let custom = ClientFactory::<CustomClient>::new().unwrap();
        assert_eq!(custom.modelName(), std::any::type_name::<CustomClient>());
        assert_eq!(custom.getConnectionName(), Some("main"));
        openrust::support::facades::Config::set("passport.connection", json!([])).unwrap();
        assert!(Client::factory().is_err());
        Response::json(StatusCode::OK, json!({"passed":true}))
    })
}
fn factory_create(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let model = Client::factory()
            .unwrap()
            .asClientCredentials()
            .create()
            .await
            .unwrap();
        let plain = model.plainSecret.as_ref().unwrap();
        let stored = Client::query()
            .unwrap()
            .find(model.getKey().unwrap().clone())
            .await
            .unwrap()
            .unwrap();
        assert!(stored.plainSecret.is_none());
        let Some(Value::Text(hash)) = stored.getAttributeValue("secret").unwrap() else {
            panic!("hash")
        };
        let hasher = openrust::container::Container::get_instance()
            .make::<dyn openrust::contracts::hashing::Hasher>()
            .unwrap();
        assert!(
            hasher
                .check(plain.as_bytes(), &hash, &Default::default())
                .await
                .unwrap()
        );
        assert_eq!(
            stored.grantTypes().unwrap().as_value(),
            &json!(["client_credentials"])
        );
        Response::json(StatusCode::OK, json!({"passed":true}))
    })
}
#[tokio::test]
async fn client_factory_states_keep_model_assignment_and_connection() {
    let root = Root::new();
    let app = root.application().await;
    let (address, server) = serve(app).await;
    let response = reqwest::get(format!("http://{address}/factory-states"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        json!({"passed":true})
    );
    server.abort();
}
#[tokio::test]
async fn mysql_client_factory_create_reloads_hash_without_plaintext() {
    if !mysql_required() {
        return;
    }
    let root = Root::new();
    let app = root.application().await;
    migrate(&app).await;
    let manager = app.make::<DatabaseManager>().await.unwrap();
    let (address, server) = serve(app).await;
    let response = reqwest::get(format!("http://{address}/factory-create"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        json!({"passed":true})
    );
    server.abort();
    manager
        .connection(Some("oauth"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("oauth_clients")
        .await
        .unwrap();
    manager
        .connection(Some("main"))
        .unwrap()
        .getSchemaBuilder()
        .dropIfExists("history")
        .await
        .unwrap();
}

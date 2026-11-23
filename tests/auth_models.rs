#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use openrust::{
    auth::{AuthError, AuthIdentifier, AuthManager},
    contracts::{container::Container as _, foundation::Application as _},
    database::{DatabaseManager, eloquent::Model},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use serde_json::{Value, json};

#[path = "support/auth_catalog_user.rs"]
mod custom_user;
#[path = "../app/models/mod.rs"]
pub(crate) mod models;
#[path = "../app/providers/mod.rs"]
mod providers;

mod app {
    pub(crate) use crate::models;
}
#[path = "../config/mod.rs"]
mod config;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "openrust-auth-models-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("storage/framework")).unwrap();
        Self(root)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn configuration(database: Value, model: &str) -> BTreeMap<String, Value> {
    [
        (
            "database".into(),
            json!({"default":"test","connections":{"test":database}}),
        ),
        (
            "auth".into(),
            json!({"providers":{"users":{"driver":"eloquent","model":model}}}),
        ),
        ("hashing".into(), json!({"driver":"bcrypt"})),
    ]
    .into()
}

struct EarlyAuth;
impl ServiceProvider for EarlyAuth {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let auth = app.container().make::<AuthManager>()?;
            let user = models::User::from_state(Default::default());
            assert_eq!(
                auth.userClass(&user),
                Some(std::any::type_name::<models::User>())
            );
            // Constructing a provider must not query an unmigrated users table.
            auth.createUserProvider("users")?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn stock_models_are_ready_during_registration_even_with_cached_configuration() {
    let root = Directory::new();
    // Compile the real auth configuration, but do not execute it. Class references
    // must survive when LoadConfiguration takes the cached path instead.
    let _loader = config::load;
    let values = configuration(
        json!({"driver":"sqlite","database":":memory:"}),
        "app::models::User",
    );
    fs::write(
        root.0.join("storage/framework/config.json"),
        serde_json::to_vec(&values).unwrap(),
    )
    .unwrap();
    let mut app = Application::configure(root.0.clone())
        .with_config(|_, _| panic!("cached configuration must bypass the loader"))
        .with_providers(vec![
            Box::new(EarlyAuth),
            Box::new(providers::AppServiceProvider),
        ])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert!(app.configuration_is_cached());
}

async fn custom_models(database: Value) {
    let root = Directory::new();
    fs::write(
        root.0.join("database.json"),
        serde_json::to_vec(&database).unwrap(),
    )
    .unwrap();
    let mut app = Application::configure(root.0.clone())
        .with_config(|base, _| {
            let database =
                serde_json::from_slice(&fs::read(base.join("database.json")).unwrap()).unwrap();
            Ok(configuration(
                database,
                openrust::auth::model!(custom_user::User, "custom::User"),
            ))
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let auth = app.container().make::<AuthManager>().unwrap();
    let user = custom_user::User::from_state(Default::default());
    assert_eq!(
        auth.userClass(&user),
        Some(std::any::type_name::<custom_user::User>())
    );
    assert_eq!(
        auth.userClass(&models::User::from_state(Default::default())),
        Some(std::any::type_name::<models::User>())
    );
    let provider = auth.createUserProvider("users").unwrap();
    let config = app
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    config
        .set(
            "auth.providers.concrete",
            json!({
                "driver":"eloquent", "model":std::any::type_name::<custom_user::User>()
            }),
        )
        .unwrap();
    assert!(auth.createUserProvider("concrete").is_ok());
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    connection.statement("CREATE TABLE users (id INTEGER PRIMARY KEY, email VARCHAR(100), password VARCHAR(100), remember_token VARCHAR(100))", &[]).await.unwrap();
    connection
        .statement(
            "INSERT INTO users (id, email, password) VALUES (7, 'user@example.test', 'hash')",
            &[],
        )
        .await
        .unwrap();
    let user = provider
        .retrieveById(&AuthIdentifier::Integer(7))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.type_id(), std::any::TypeId::of::<custom_user::User>());
    assert_eq!(
        auth.userAttributes(user.as_ref()).unwrap()["email"],
        "user@example.test"
    );

    // Compiled classes are available like autoloadable PHP classes, but runtime
    // registrations and configuration still belong to their application.
    auth.registerModel::<custom_user::User>("runtime::Only");
    let other_root = Directory::new();
    let mut other = Application::configure(other_root.0.clone())
        .with_config(|_, _| {
            Ok(configuration(
                json!({"driver":"sqlite","database":":memory:"}),
                "runtime::Only",
            ))
        })
        .create()
        .unwrap();
    other.boot().await.unwrap();
    let other_auth = other.container().make::<AuthManager>().unwrap();
    assert!(matches!(
        other_auth.createUserProvider("users"),
        Err(AuthError::Configuration("compiled Eloquent model"))
    ));
    // Explicit registration remains available for application extensions.
    other_auth.registerModel::<custom_user::User>("runtime::Only");
    assert!(other_auth.createUserProvider("users").is_ok());
    connection.statement("DROP TABLE users", &[]).await.unwrap();
}

#[tokio::test]
async fn custom_auth_models_on_sqlite() {
    custom_models(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn custom_auth_models_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_AUTH_MODELS_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_AUTH_MODELS_MYSQL").is_none());
    if let Some(url) = url {
        custom_models(json!({"url":url})).await;
    }
}

#[tokio::test]
async fn custom_auth_models_on_postgres() {
    let url = std::env::var("OPENRUST_TEST_POSTGRES_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_POSTGRES").is_none());
    if let Some(url) = url {
        custom_models(json!({"url":url})).await;
    }
}

#[tokio::test]
async fn configuration_class_references_resolve_without_application_wiring() {
    assert_eq!(
        openrust::auth::model!(crate::app::models::User),
        "app::models::User"
    );
    let root = Directory::new();
    let mut app = Application::configure(root.0.clone())
        .with_config(config::load)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let auth = app.container().make::<AuthManager>().unwrap();
    assert!(auth.createUserProvider("users").is_ok());
}

#[tokio::test]
async fn ambiguous_class_names_fail_instead_of_depending_on_link_order() {
    let root = Directory::new();
    let mut app = Application::configure(root.0.clone())
        .with_config(|_, _| {
            let first = openrust::auth::model!(models::User, "collision::User");
            let second = openrust::auth::model!(custom_user::User, "collision::User");
            assert_eq!(first, second);
            Ok(configuration(
                json!({"driver":"sqlite","database":":memory:"}),
                first,
            ))
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert!(matches!(
        app.container()
            .make::<AuthManager>()
            .unwrap()
            .createUserProvider("users"),
        Err(AuthError::Configuration("ambiguous Eloquent model class"))
    ));
}

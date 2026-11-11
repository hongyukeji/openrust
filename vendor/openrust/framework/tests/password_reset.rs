use std::time::Duration;

use openrust::{
    auth::{
        AuthFuture, AuthIdentifier, AuthResult,
        passwords::{CacheTokenRepository, DatabaseTokenRepository, TokenRepository},
    },
    cache::CacheManager,
    contracts::{
        auth::{Authenticatable, CanResetPassword},
        config::Repository,
        container::Container as _,
        foundation::Application as _,
    },
    database::{DatabaseManager, Value},
    foundation::Application,
    hashing::HashManager,
};
use serde_json::json;

struct ResetUser(String);

impl Authenticatable for ResetUser {
    fn get_auth_identifier_name(&self) -> &str {
        "email"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::String(self.0.clone())
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

#[allow(
    non_snake_case,
    reason = "Implements Laravel's password reset capability"
)]
impl CanResetPassword for ResetUser {
    fn getEmailForPasswordReset(&self) -> &str {
        &self.0
    }
    fn sendPasswordResetNotification<'a>(&'a self, _: &'a str) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

async fn database_reset_tokens(configured: serde_json::Value) {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config.set("database.default", json!("reset")).unwrap();
    config
        .set("database.connections.reset", configured)
        .unwrap();
    config.set("hashing.driver", json!("bcrypt")).unwrap();
    config.set("hashing.bcrypt.rounds", json!(4)).unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    db.getSchemaBuilder()
        .dropIfExists("password_reset_tokens")
        .await
        .unwrap();
    db.getSchemaBuilder()
        .create("password_reset_tokens", |table| {
            table.string("email", 255).primary();
            table.string("token", 255);
            table.timestamp("created_at", 0);
        })
        .await
        .unwrap();
    let hasher = app
        .container()
        .make::<HashManager>()
        .unwrap()
        .driver(None)
        .unwrap();
    let store = DatabaseTokenRepository::new(
        db.clone(),
        "password_reset_tokens",
        "base64:c2VjcmV0",
        hasher,
        Duration::from_secs(3600),
        Duration::from_secs(60),
    )
    .unwrap();
    let user = ResetUser("one@example.test".into());
    let other = ResetUser("two@example.test".into());
    assert!(!store.recentlyCreatedToken(&user).await.unwrap());
    let (first, competing) = tokio::join!(store.issue(&user), store.issue(&user));
    let first = match (first.unwrap(), competing.unwrap()) {
        (Some(token), None) | (None, Some(token)) => token,
        _ => panic!("exactly one concurrent issue must succeed"),
    };
    assert!(store.recentlyCreatedToken(&user).await.unwrap());
    let Value::Text(hash) = db
        .table("password_reset_tokens")
        .value("token")
        .await
        .unwrap()
    else {
        panic!("hash missing");
    };
    assert_ne!(hash, first);
    assert!(hash.starts_with("$2"));
    assert!(store.exists(&user, &first).await.unwrap());
    assert!(!store.exists(&other, &first).await.unwrap());
    let second = store.create(&user).await.unwrap();
    assert!(!store.exists(&user, &first).await.unwrap());
    assert!(store.exists(&user, &second).await.unwrap());
    let (claim, competing) = tokio::join!(store.claim(&user, &second), store.claim(&user, &second));
    let claim = match (claim.unwrap(), competing.unwrap()) {
        (Some(claim), None) | (None, Some(claim)) => claim,
        _ => panic!("exactly one callback may claim the token"),
    };
    assert!(!store.exists(&user, &second).await.unwrap());
    store
        .finish(user.getEmailForPasswordReset(), claim, false)
        .await
        .unwrap();
    assert!(store.exists(&user, &second).await.unwrap());
    let claim = store.claim(&user, &second).await.unwrap().unwrap();
    let third = store.create(&user).await.unwrap();
    store
        .finish(user.getEmailForPasswordReset(), claim, false)
        .await
        .unwrap();
    assert!(!store.exists(&user, &second).await.unwrap());
    assert!(store.exists(&user, &third).await.unwrap());
    db.table("password_reset_tokens")
        .update([("created_at".into(), "2000-01-01 00:00:00".into())].into())
        .await
        .unwrap();
    assert!(!store.exists(&user, &third).await.unwrap());
    assert!(!store.recentlyCreatedToken(&user).await.unwrap());
    store.deleteExpired().await.unwrap();
    assert_eq!(
        db.table("password_reset_tokens").count("*").await.unwrap(),
        0
    );
}

#[tokio::test]
async fn sqlite_reset_tokens_are_hashed_replaced_throttled_and_expired() {
    database_reset_tokens(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn mysql_reset_tokens_are_hashed_replaced_throttled_and_expired() {
    if let Ok(url) = std::env::var("OPENRUST_TEST_RESET_MYSQL_URL") {
        database_reset_tokens(json!({"url":url})).await;
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_RESET_MYSQL").is_none());
    }
}

#[tokio::test]
async fn cache_reset_tokens_obey_redis_ttl_and_user_scope() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none(),
            "Redis fixture is required"
        );
        return;
    };
    let port: u16 = port.parse().unwrap();
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config
        .set("database.redis.default.port", json!(port))
        .unwrap();
    config
        .set(
            "cache.prefix",
            json!(format!("reset-{:016x}:", fastrand::u64(..))),
        )
        .unwrap();
    config
        .set(
            "cache.stores.redis",
            json!({"driver":"redis","connection":"default"}),
        )
        .unwrap();
    config.set("hashing.driver", json!("bcrypt")).unwrap();
    config.set("hashing.bcrypt.rounds", json!(4)).unwrap();
    let cache = app
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(Some("redis"))
        .unwrap();
    let hasher = app
        .container()
        .make::<HashManager>()
        .unwrap()
        .driver(None)
        .unwrap();
    let store = CacheTokenRepository::new(
        cache,
        "secret",
        hasher,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let user = ResetUser("one@example.test".into());
    let other = ResetUser("two@example.test".into());
    let token = store.create(&user).await.unwrap();
    assert!(store.recentlyCreatedToken(&user).await.unwrap());
    assert!(!store.exists(&other, &token).await.unwrap());
    assert!(store.exists(&user, &token).await.unwrap());
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.exists(&user, &token).await.unwrap() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(!store.recentlyCreatedToken(&user).await.unwrap());
    let next = store.create(&user).await.unwrap();
    let (claim, competing) = tokio::join!(store.claim(&user, &next), store.claim(&user, &next));
    let claim = match (claim.unwrap(), competing.unwrap()) {
        (Some(claim), None) | (None, Some(claim)) => claim,
        _ => panic!("exactly one callback may claim the Redis token"),
    };
    store
        .finish(user.getEmailForPasswordReset(), claim, false)
        .await
        .unwrap();
    let claim = store.claim(&user, &next).await.unwrap().unwrap();
    store
        .finish(user.getEmailForPasswordReset(), claim, true)
        .await
        .unwrap();
    assert!(!store.exists(&user, &next).await.unwrap());
}

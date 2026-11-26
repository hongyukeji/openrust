#![cfg(feature = "sanctum")]
#[path = "../src/starter/challenges.rs"]
mod challenges;
use challenges::{ChallengeStore, Pending, TABLE};
use openrust::{
    auth::AuthIdentifier,
    database::{DatabaseManager, Value},
    foundation::Application,
    support::facades::{App, Facade as _},
};
use serde_json::json;

async fn verify(mut database: serde_json::Value) {
    let root = Root::new();
    if let Ok(url) = std::env::var("OPENRUST_USER_KIT_READ_URL") {
        database["read"] = json!({"url":url});
    }
    std::fs::write(
        root.path().join("database.json"),
        serde_json::to_vec(&database).unwrap(),
    )
    .unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .with_config(|root, _| Ok([
            ("database".into(), json!({"default":"fixture","connections":{"fixture":serde_json::from_slice::<serde_json::Value>(&std::fs::read(root.join("database.json")).unwrap()).unwrap()}})),
            ("app".into(),json!({"timezone":"Asia/Shanghai"})),
        ].into())).create().unwrap();
    app.boot().await.unwrap();
    let db = app.make::<DatabaseManager>().await.unwrap();
    let connection = db.connection(None).unwrap();
    let mut schema = connection.get_schema_builder();
    schema.drop_if_exists(TABLE).await.unwrap();
    schema
        .create(TABLE, |table| {
            table.string("id", 64).primary();
            table.string("purpose", 64);
            table.string("owner_hash", 64).index();
            table.text("payload");
            table.big_integer("created_at");
            table.big_integer("expires_at").index();
            table.big_integer("consumed_at").nullable();
        })
        .await
        .unwrap();
    App::set_facade_application(Some(&app), async {
        let clock = app.make::<openrust::support::DateFactory>().await.unwrap();
        let frozen = "2030-01-02T03:04:05Z".parse::<jiff::Timestamp>().unwrap();
        clock.set_test_now(Some(openrust::support::Date::from_utc(
            frozen,
            jiff::tz::TimeZone::UTC,
        )));
        let store = ChallengeStore::new(connection.clone());
        let pending = Pending {
            user: AuthIdentifier::UnsignedInteger(42),
            owner: "users".into(),
            purpose: "login".into(),
            credential: "ABC-sensitive-binding".into(),
            factor: "TOTP-state".into(),
            token: None,
            payload: json!({"device":"phone","abilities":["account:read"]}),
        };
        assert!(store.create(&pending, 0).await.is_err());
        let challenge = store.create(&pending, 300).await.unwrap();
        assert_eq!(challenge.expires_at, frozen.as_second() + 300);
        let row = connection
            .table(TABLE)
            .use_write_pdo()
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_ne!(row.get("id"), Some(&Value::Text(challenge.key.to_string())));
        assert!(matches!(row.get("payload"), Some(Value::Text(payload)) if !payload.contains(challenge.key.as_str())));
        assert!(
            store
                .resolve(&challenge.key, "reauth")
                .await
                .unwrap()
                .is_none()
        );
        assert!(store.resolve("malformed", "login").await.unwrap().is_none());
        let resolved = store
            .resolve(&challenge.key, "login")
            .await
            .unwrap()
            .unwrap();
        let mut forged = resolved.clone();
        forged.credential.make_ascii_lowercase();
        assert!(store.consume(&challenge.key, &forged).await.unwrap().is_none());
        let mut wrong_user = resolved.clone(); wrong_user.user = AuthIdentifier::String("42".into());
        assert!(store.consume(&challenge.key, &wrong_user).await.unwrap().is_none());
        let mut wrong_token = resolved.clone(); wrong_token.token = Some(AuthIdentifier::Integer(42));
        assert!(store.consume(&challenge.key, &wrong_token).await.unwrap().is_none());
        let a = ChallengeStore::new(connection.clone());
        let b = ChallengeStore::new(connection.clone());
        let (left, right) = tokio::join!(
            a.consume(&challenge.key, &resolved),
            b.consume(&challenge.key, &resolved)
        );
        assert_eq!(
            [left.unwrap(), right.unwrap()]
                .into_iter()
                .filter(Option::is_some)
                .count(),
            1
        );
        assert!(
            store
                .resolve(&challenge.key, "login")
                .await
                .unwrap()
                .is_none()
        );
        assert!(store.consume(&challenge.key, &resolved).await.unwrap().is_none());
        let expires = store.create(&pending, 300).await.unwrap();
        clock.set_test_now(Some(openrust::support::Date::from_utc(
            frozen
                .checked_add(jiff::SignedDuration::from_secs(300))
                .unwrap(),
            jiff::tz::TimeZone::UTC,
        )));
        assert!(
            store
                .resolve(&expires.key, "login")
                .await
                .unwrap()
                .is_none()
        );
        assert!(store.consume(&expires.key, &pending).await.unwrap().is_none());
        assert!(store.prune(-1).await.is_err());
        assert_eq!(challenges::prune_for::<ChallengeOwner>(0).await.unwrap(), 2);
        assert_eq!(store.prune(0).await.unwrap(), 0);
    })
    .await;
}
#[tokio::test]
async fn sqlite_atomic_challenges_are_bound_expiring_and_single_use() {
    let root = Root::new();
    let file = root.path().join("challenges.sqlite");
    std::fs::File::create(&file).unwrap();
    verify(json!({"driver":"sqlite","database":file})).await;
}
#[tokio::test]
#[ignore = "需要独立MySQL 8.4连接"]
async fn mysql_atomic_challenges_are_bound_expiring_and_single_use() {
    verify(json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").expect("必须提供独立MySQL连接")}))
        .await;
}
#[tokio::test]
#[ignore = "需要独立PostgreSQL连接"]
async fn postgres_atomic_challenges_are_bound_expiring_and_single_use() {
    verify(json!({"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("必须提供独立PostgreSQL连接")})).await;
}

struct Root(std::path::PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "user-challenges-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ChallengeOwner(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for ChallengeOwner {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("users");
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}

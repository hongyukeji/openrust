use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        hashing::Hasher,
    },
    database::{
        Connection, DatabaseError, DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState, casts::AttributeCast, relations::HasMany,
        },
    },
    foundation::Application,
    hashing::{HashError, HashOptions},
};
use serde_json::{Value as Json, json};

struct Account(ModelState);

impl Model for Account {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("hashed_accounts")
            .timestamps(None, None)
            .fillable(&["name", "password"])
            .hidden(&["password"]);
        &METADATA
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

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("password", AttributeCast::Hashed)]
    }
}

async fn application(config: &Json, driver: &str, rounds: u32) -> (Application, Connection) {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let container = app.container();
    let repository = container.get::<dyn Repository>("config").unwrap();
    repository.set("hashing.driver", json!(driver)).unwrap();
    repository
        .set("hashing.bcrypt.rounds", json!(rounds))
        .unwrap();
    repository.set("database.default", json!("test")).unwrap();
    repository
        .set("database.connections.test", config.clone())
        .unwrap();
    let db = container
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    (app, db)
}

fn password(value: &str) -> Attributes {
    Attributes::from([("password".into(), value.into())])
}

fn hash_of(model: &Account) -> &str {
    let Some(Value::Text(hash)) = model.get_attribute("password") else {
        panic!("expected a stored password hash");
    };
    hash
}

fn php_bcrypt(password: &str, cost: u32) -> String {
    bcrypt::hash_with_result(password, cost)
        .unwrap()
        .format_for_version(bcrypt::Version::TwoY)
}

async fn verify(config: Json, id: &str) {
    let (app, db) = application(&config, "bcrypt", 5).await;
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    db.statement("DROP TABLE IF EXISTS hashed_accounts", &[])
        .await
        .unwrap();
    db.statement(&format!("CREATE TABLE hashed_accounts (id {id}, parent_id BIGINT, name VARCHAR(40), password TEXT)"), &[]).await.unwrap();
    let query = || Account::query_on(&db);
    let account = query().create(password("plain password")).await.unwrap();
    let key = account.get_key().unwrap().clone();
    assert!(!account.to_array().unwrap().contains_key("password"));
    let mut account = query().find_or_fail(key.clone()).await.unwrap();
    let first = hash_of(&account).to_owned();
    assert!(first.starts_with("$2y$05$"));
    assert!(
        hasher
            .check(b"plain password", &first, &HashOptions::new())
            .await
            .unwrap()
    );
    let old = php_bcrypt("old", 4);
    account
        .set_attribute_value("password", old.clone())
        .unwrap();
    account.save().await.unwrap();
    let persisted = query().find_or_fail(key.clone()).await.unwrap();
    assert_eq!(hash_of(&persisted), old);
    assert!(
        account
            .set_attribute_value("password", "use async")
            .is_err()
    );
    assert!(
        account
            .fill_json_attribute("password->field", "cannot bypass hashing")
            .is_err()
    );
    let too_costly = php_bcrypt("reject", 6);
    let mut snapshot = account.state().get_attributes().clone();
    assert!(matches!(
        account
            .fill_async(Attributes::from([
                ("name".into(), "changed".into()),
                ("password".into(), too_costly.into())
            ]))
            .await,
        Err(DatabaseError::AttributeHashing(HashError::InvalidOptions))
    ));
    // 后续字段转换失败不撤销先前已完成的赋值。
    let reference: Json =
        serde_json::from_str(include_str!("reference/eloquent-strictness.json")).unwrap();
    snapshot.insert(
        "name".into(),
        reference["fill_plain_setter_failure"]["name"]
            .as_str()
            .unwrap()
            .into(),
    );
    assert_eq!(account.state().get_attributes(), &snapshot);
    account.update(password("rotated")).await.unwrap();
    let reloaded = query().find_or_fail(key).await.unwrap();
    assert!(
        hasher
            .check(b"rotated", hash_of(&reloaded), &HashOptions::new())
            .await
            .unwrap()
    );

    let children = HasMany::<Account, Account>::new("id", "parent_id");
    let child = children.create(&account, password("child")).await.unwrap();
    assert!(
        hasher
            .check(b"child", hash_of(&child), &HashOptions::new())
            .await
            .unwrap()
    );
    let one = query();
    let two = query();
    let (one, two) = tokio::join!(one.make_async(password("")), two.make_async(password("")));
    let one = one.unwrap();
    let two = two.unwrap();
    assert_ne!(hash_of(&one), hash_of(&two));
    assert!(
        hasher
            .check(b"", hash_of(&one), &HashOptions::new())
            .await
            .unwrap()
    );

    let (_other_app, other_db) = application(&config, "bcrypt", 4).await;
    let other = Account::query_on(&other_db)
        .make_async(password("plain password"))
        .await
        .unwrap();
    assert!(hash_of(&other).starts_with("$2y$04$"));
    let (_argon_app, argon_db) = application(&config, "argon2id", 4).await;
    let argon = Account::query_on(&argon_db);
    assert!(matches!(
        argon.make_async(password(&old)).await,
        Err(DatabaseError::AttributeHashing(HashError::InvalidOptions))
    ));
    let (_invalid_app, invalid_db) = application(&config, "unsupported", 4).await;
    let mut invalid = Account::query_on(&invalid_db)
        .make(Attributes::new())
        .unwrap();
    invalid
        .set_attribute_value_async("password", Value::Null)
        .await
        .unwrap();
    assert!(
        invalid
            .set_attribute_value_async("password", "no driver")
            .await
            .is_err()
    );
    assert_eq!(invalid.get_attribute("password"), Some(&Value::Null));
    db.statement("DROP TABLE hashed_accounts", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn hashed_model_casts_on_sqlite() {
    verify(
        json!({"driver": "sqlite", "database": ":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn hashed_model_casts_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url": url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}

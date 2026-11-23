use openrust::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{AsEncryptedArrayObject, AsEncryptedCollection, AttributeCast, EncryptedCast},
        },
    },
    encryption::{Cipher, Encrypter},
    foundation::Application,
    support::{ArrayKey, Collection},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

struct Record(ModelState);

#[derive(Deserialize, Serialize)]
struct Member {
    name: String,
}

fn restore_owner(mut items: Collection) -> DatabaseResult<Collection> {
    items.put("owner", json!("system"));
    Ok(items)
}

fn describe_member(value: &Json, key: &ArrayKey) -> DatabaseResult<Json> {
    let name = value
        .as_str()
        .ok_or(DatabaseError::InvalidQuery("test member name"))?;
    Ok(json!({"name": name, "key": key.to_string()}))
}

impl Model for Record {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("encrypted_records")
            .incrementing(false)
            .timestamps(None, None)
            .fillable(&[
                "id",
                "secret",
                "settings",
                "alias_json",
                "metadata",
                "items",
                "class_items",
                "class_array",
                "projected",
                "custom",
                "class_items->team->name",
                "plain->a",
                "settings->mode",
            ]);
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
        const CASTS: &[(&str, AttributeCast)] = &[
            ("secret", AttributeCast::Encrypted(EncryptedCast::String)),
            ("settings", AttributeCast::Encrypted(EncryptedCast::Array)),
            ("alias_json", AttributeCast::Encrypted(EncryptedCast::Json)),
            ("metadata", AttributeCast::Encrypted(EncryptedCast::Object)),
            ("items", AttributeCast::Encrypted(EncryptedCast::Collection)),
            ("class_items", AsEncryptedCollection::cast()),
            ("class_array", AsEncryptedArrayObject::cast()),
            ("projected", AsEncryptedCollection::of::<Member>()),
            (
                "custom",
                AsEncryptedCollection::using(restore_owner, Some(describe_member)),
            ),
            ("plain", AttributeCast::Json),
        ];
        CASTS
    }
}

async fn application(
    config: &Json,
    key: Option<&str>,
    previous: &[&str],
) -> (Application, Connection) {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let container = app.container();
    let repository = container.get::<dyn Repository>("config").unwrap();
    repository.set("app.key", json!(key)).unwrap();
    repository.set("app.cipher", json!("AES-256-CBC")).unwrap();
    repository
        .set("app.previous_keys", json!(previous))
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

fn ciphertext(value: Option<&Value>) -> &str {
    let Some(Value::Text(value)) = value else {
        panic!("expected ciphertext");
    };
    value
}

async fn verify(config: Json) {
    const OLD: &str = "oooooooooooooooooooooooooooooooo";
    const NEW: &str = "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn";
    let (_old_app, old) = application(&config, Some(OLD), &[]).await;
    old.statement("DROP TABLE IF EXISTS encrypted_records", &[])
        .await
        .unwrap();
    old.statement(
        "CREATE TABLE encrypted_records (id BIGINT PRIMARY KEY, secret TEXT, settings TEXT, alias_json TEXT, metadata TEXT, items TEXT, class_items TEXT, class_array TEXT, projected TEXT, custom TEXT, plain TEXT)",
        &[],
    ).await.unwrap();
    let record = Record::query_on(&old)
        .create(Attributes::from([
            ("id".into(), 1.into()),
            ("secret".into(), "private sentinel".into()),
            ("settings".into(), Value::Json(json!({"mode": "on"}))),
            ("alias_json".into(), Value::Json(json!([1, 2]))),
            ("metadata".into(), Value::Json(json!({"object": true}))),
            ("items".into(), Value::Json(json!({"team": "ops"}))),
            (
                "class_items".into(),
                Value::Json(json!({"team": {"name":"ops","rank":1}})),
            ),
            ("class_array".into(), Value::Json(json!(["east", "west"]))),
        ]))
        .await
        .unwrap();
    let old_ciphertext = ciphertext(record.get_raw_original("secret")).to_owned();
    assert!(Encrypter::appearsEncrypted(&old_ciphertext));
    assert!(!old_ciphertext.contains("private sentinel"));
    let mut loaded = Record::query_on(&old).find_or_fail(1).await.unwrap();
    for (column, expected) in [
        ("settings", json!({"mode": "on"})),
        ("alias_json", json!([1, 2])),
        ("metadata", json!({"object": true})),
        ("items", json!({"team": "ops"})),
    ] {
        assert!(Encrypter::appearsEncrypted(ciphertext(
            loaded.get_raw_original(column)
        )));
        assert_eq!(
            loaded.get_attribute_value(column).unwrap(),
            Some(Value::Json(expected))
        );
    }
    assert_eq!(
        serde_json::to_value(loaded.get_encrypted_collection("items").unwrap().unwrap()).unwrap(),
        json!({"team": "ops"})
    );
    loaded
        .set_attribute_value("secret", "private sentinel")
        .unwrap();
    loaded
        .set_attribute_value("metadata", Value::Json(json!({"object": true})))
        .unwrap();
    loaded
        .set_attribute_value("items", Value::Json(json!({"team": "ops"})))
        .unwrap();
    assert_ne!(ciphertext(loaded.get_attribute("secret")), old_ciphertext);
    assert!(!loaded.is_dirty().unwrap());
    loaded.save().await.unwrap();

    let (rotated_app, rotated) = application(&config, Some(NEW), &[OLD]).await;
    let (_wrong_app, wrong) = application(&config, Some(NEW), &[]).await;
    let (rotating, wrong_key) = tokio::join!(
        Record::query_on(&rotated).find_or_fail(1),
        Record::query_on(&wrong).find_or_fail(1),
    );
    let mut rotating = rotating.unwrap();
    let mut wrong_key = wrong_key.unwrap();
    assert!(!rotating.is_dirty().unwrap());
    assert_eq!(
        rotating.get_raw_original("secret"),
        Some(&old_ciphertext.clone().into())
    );
    assert_eq!(
        rotating.get_attribute_value("secret").unwrap(),
        Some("private sentinel".into())
    );
    assert!(matches!(
        wrong_key.get_attribute_value("secret"),
        Err(DatabaseError::AttributeDecryption(_))
    ));
    wrong_key.set_visible(&["id"]);
    assert_eq!(wrong_key.to_json().unwrap(), r#"{"id":1}"#);
    let mut before = wrong_key.state().get_attributes().clone();
    assert!(matches!(
        wrong_key.fill(Attributes::from([
            ("plain->a".into(), 1.into()),
            ("settings->mode".into(), "blocked".into()),
        ])),
        Err(DatabaseError::AttributeDecryption(_))
    ));
    // 后续加密字段失败不会撤销已经完成的JSON路径赋值。
    let reference: Json =
        serde_json::from_str(include_str!("reference/eloquent-strictness.json")).unwrap();
    before.insert(
        "plain".into(),
        Value::Json(
            serde_json::from_str(
                reference["fill_json_setter_failure"]["plain"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        ),
    );
    assert_eq!(wrong_key.state().get_attributes(), &before);
    verify_json_edits(&mut rotating);
    rotating
        .set_attribute_value("secret", "private sentinel")
        .unwrap();
    rotating
        .set_attribute_value("items", Value::Json(json!({"team": "ops"})))
        .unwrap();
    assert!(rotating.is_dirty().unwrap());
    rotating.save().await.unwrap();
    let new_key_only = Record::query_on(&wrong).find_or_fail(1).await.unwrap();
    assert_eq!(
        new_key_only.get_attribute_value("secret").unwrap(),
        Some("private sentinel".into())
    );
    assert_eq!(
        new_key_only.get_attribute_value("class_items").unwrap(),
        Some(Value::Json(
            json!({"team":{"name":"support","rank":1},"added":true})
        ))
    );
    assert_eq!(
        new_key_only.get_attribute_value("class_array").unwrap(),
        Some(Value::Json(json!(["east", "west", "north"])))
    );
    assert_eq!(
        new_key_only.get_attribute_value("plain").unwrap(),
        Some(Value::Json(json!({"list":[1,null],"a":1})))
    );
    assert!(new_key_only.get_attribute("plain->a").is_none());
    assert_eq!(
        new_key_only.get_attribute_value("settings").unwrap(),
        Some(Value::Json(json!({"mode":"on","nested":{"flag":true}})))
    );
    assert!(
        Record::query_on(&old)
            .find_or_fail(1)
            .await
            .unwrap()
            .get_attribute_value("secret")
            .is_err()
    );

    let (_missing_app, missing) = application(&config, None, &[]).await;
    let mut empty = Record::query_on(&missing).make(Attributes::new()).unwrap();
    empty.set_attribute_value("secret", Value::Null).unwrap();
    assert_eq!(
        empty.get_attribute_value("secret").unwrap(),
        Some(Value::Null)
    );
    assert!(matches!(
        empty.set_attribute_value("secret", "private sentinel"),
        Err(DatabaseError::AttributeEncryption(_))
    ));
    assert_eq!(empty.get_attribute("secret"), Some(&Value::Null));
    let mut malformed = Record::query_on(&wrong).find_or_fail(1).await.unwrap();
    malformed.set_attribute("secret", "malformed payload");
    assert!(matches!(
        malformed.get_attribute_value("secret"),
        Err(DatabaseError::AttributeDecryption(_))
    ));

    verify_projections(&old, &rotated, &wrong, NEW).await;

    // A model continues to belong to its original application.
    drop(rotated_app);
    assert!(matches!(
        rotating.get_attribute_value("secret"),
        Err(DatabaseError::AttributeDecryption(_))
    ));
    old.statement("DROP TABLE encrypted_records", &[])
        .await
        .unwrap();
    let fresh = Encrypter::new(NEW, Cipher::Aes256Cbc).unwrap();
    assert_eq!(
        fresh
            .decrypt_string(ciphertext(new_key_only.get_raw_original("secret")))
            .unwrap(),
        b"private sentinel"
    );
}

async fn verify_projections(
    old: &Connection,
    rotated: &Connection,
    current: &Connection,
    key: &str,
) {
    let projected = json!({"second":{"name":"West","internal":1},"first":{"name":"East"}});
    let custom = json!({"8":"Dana","team":"Rui"});
    Record::query_on(old)
        .create(Attributes::from([
            ("id".into(), 2.into()),
            ("projected".into(), Value::Json(projected.clone())),
            ("custom".into(), Value::Json(custom.clone())),
        ]))
        .await
        .unwrap();
    let mut record = Record::query_on(old).find_or_fail(2).await.unwrap();
    let original_ciphertext = ciphertext(record.get_raw_original("projected")).to_owned();
    assert!(Encrypter::appearsEncrypted(&original_ciphertext));
    let view = record.to_array().unwrap();
    assert_eq!(
        view["projected"],
        json!({"second":{"name":"West"},"first":{"name":"East"}})
    );
    assert_eq!(
        view["custom"],
        json!({"8":{"name":"Dana","key":"8"},"team":{"name":"Rui","key":"team"},"owner":{"name":"system","key":"owner"}})
    );
    let values = record
        .get_encrypted_collection("projected")
        .unwrap()
        .unwrap();
    let keys = values
        .all()
        .keys()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(keys, ["second", "first"]);
    assert!(!record.is_dirty().unwrap());
    record
        .set_attribute_value("projected", Value::Json(projected))
        .unwrap();
    assert!(!record.is_dirty().unwrap());
    record.save().await.unwrap();
    let mut record = Record::query_on(rotated).find_or_fail(2).await.unwrap();
    assert_eq!(
        ciphertext(record.get_raw_original("projected")),
        original_ciphertext
    );
    assert_eq!(record.to_array().unwrap()["custom"], view["custom"]);
    record
        .fill_json_attribute("projected->first->name", "North")
        .unwrap();
    record
        .set_attribute_value("custom", Value::Json(custom))
        .unwrap();
    record.save().await.unwrap();
    let mut record = Record::query_on(current).find_or_fail(2).await.unwrap();
    let encrypted = ciphertext(record.get_raw_original("projected"));
    let encrypter = Encrypter::new(key, Cipher::Aes256Cbc).unwrap();
    let plain: Json =
        serde_json::from_slice(&encrypter.decrypt_string(encrypted).unwrap()).unwrap();
    assert_eq!(plain["second"]["internal"], 1);
    assert_eq!(plain["first"]["name"], "North");
    assert_eq!(record.to_array().unwrap()["custom"], view["custom"]);

    record.set_attribute_value("custom", "scalar").unwrap();
    assert_eq!(
        record.to_array().unwrap()["custom"],
        json!({"0":{"name":"scalar","key":"0"},"owner":{"name":"system","key":"owner"}})
    );
    record.set_attribute_value("custom", Value::Null).unwrap();
    assert!(record.get_encrypted_collection("custom").unwrap().is_none());
    let malformed = Value::Json(json!([{"missing_name":true}]));
    record.set_attribute_value("projected", malformed).unwrap();
    let attributes = record.state().get_attributes().clone();
    assert!(record.get_encrypted_collection("projected").is_err());
    assert!(record.to_array().is_err());
    assert!(
        record
            .edit_encrypted_collection("projected", |_| Ok(()))
            .is_err()
    );
    assert_eq!(record.state().get_attributes(), &attributes);
}

fn verify_json_edits(record: &mut Record) {
    record.set_attribute_value("plain", "east").unwrap();
    record.fill_json_attribute("plain->-1", "y").unwrap();
    assert_eq!(
        record.get_attribute_value("plain").unwrap(),
        Some(Value::Json(json!("easy")))
    );
    record.set_attribute_value("plain", Value::Null).unwrap();

    let array = record.get_attribute("class_array").cloned().unwrap();
    record
        .set_attribute_value("class_array", "not an array")
        .unwrap();
    assert!(record.get_encrypted_collection("class_array").is_err());
    record.set_attribute("class_array", array);

    let before = record.state().get_attributes().clone();
    assert!(
        record
            .fill_json_attribute("class_items->team..name", "bad")
            .is_err()
    );
    assert_eq!(record.state().get_attributes(), &before);
    record
        .fill(Attributes::from([
            ("class_items->team->name".into(), "support".into()),
            ("plain->a".into(), 1.into()),
            ("class_items->guarded".into(), true.into()),
        ]))
        .unwrap();
    record
        .edit_encrypted_collection("class_items", |items| {
            items.put("added", json!(true));
            Ok(())
        })
        .unwrap();
    record
        .fill_json_attribute("class_array->2", "north")
        .unwrap();
    record.fill_json_attribute("plain->list->0", 1).unwrap();
    record
        .set_attribute_value("plain->list->1", Value::Null)
        .unwrap();
    record
        .set_attribute_value("settings->nested.flag", true)
        .unwrap();
    let raw = ciphertext(record.get_attribute("class_items"));
    assert!(Encrypter::appearsEncrypted(raw));
    assert!(!raw.contains("support"));
    assert!(record.get_attribute("class_items->team->name").is_none());
}

#[tokio::test]
async fn encrypted_attributes_rotate_on_sqlite() {
    let path =
        std::env::temp_dir().join(format!("openrust-encrypted-{}.sqlite", fastrand::u64(..)));
    std::fs::File::create_new(&path).unwrap();
    verify(json!({"driver": "sqlite", "database": path})).await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn encrypted_attributes_rotate_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url": url})).await;
    }
}

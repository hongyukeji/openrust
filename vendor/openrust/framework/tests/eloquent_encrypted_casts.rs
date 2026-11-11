use openrust::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{AsEncryptedArrayObject, AsEncryptedCollection, AttributeCast, EncryptedCast},
        },
    },
    encryption::{Cipher, Encrypter},
    foundation::Application,
};
use serde_json::{Value as Json, json};

struct Record(ModelState);

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
        "CREATE TABLE encrypted_records (id BIGINT PRIMARY KEY, secret TEXT, settings TEXT, alias_json TEXT, metadata TEXT, items TEXT, class_items TEXT, class_array TEXT, plain TEXT)",
        &[],
    ).await.unwrap();
    let record = Record::queryOn(&old)
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
    let old_ciphertext = ciphertext(record.getRawOriginal("secret")).to_owned();
    assert!(Encrypter::appearsEncrypted(&old_ciphertext));
    assert!(!old_ciphertext.contains("private sentinel"));
    let mut loaded = Record::queryOn(&old).findOrFail(1).await.unwrap();
    for (column, expected) in [
        ("settings", json!({"mode": "on"})),
        ("alias_json", json!([1, 2])),
        ("metadata", json!({"object": true})),
        ("items", json!({"team": "ops"})),
    ] {
        assert!(Encrypter::appearsEncrypted(ciphertext(
            loaded.getRawOriginal(column)
        )));
        assert_eq!(
            loaded.getAttributeValue(column).unwrap(),
            Some(Value::Json(expected))
        );
    }
    assert_eq!(
        serde_json::to_value(loaded.getEncryptedCollection("items").unwrap().unwrap()).unwrap(),
        json!({"team": "ops"})
    );
    loaded
        .setAttributeValue("secret", "private sentinel")
        .unwrap();
    loaded
        .setAttributeValue("metadata", Value::Json(json!({"object": true})))
        .unwrap();
    loaded
        .setAttributeValue("items", Value::Json(json!({"team": "ops"})))
        .unwrap();
    assert_ne!(ciphertext(loaded.getAttribute("secret")), old_ciphertext);
    assert!(!loaded.isDirty().unwrap());
    loaded.save().await.unwrap();

    let (rotated_app, rotated) = application(&config, Some(NEW), &[OLD]).await;
    let (_wrong_app, wrong) = application(&config, Some(NEW), &[]).await;
    let (rotating, wrong_key) = tokio::join!(
        Record::queryOn(&rotated).findOrFail(1),
        Record::queryOn(&wrong).findOrFail(1),
    );
    let mut rotating = rotating.unwrap();
    let mut wrong_key = wrong_key.unwrap();
    assert!(!rotating.isDirty().unwrap());
    assert_eq!(
        rotating.getRawOriginal("secret"),
        Some(&old_ciphertext.clone().into())
    );
    assert_eq!(
        rotating.getAttributeValue("secret").unwrap(),
        Some("private sentinel".into())
    );
    assert!(matches!(
        wrong_key.getAttributeValue("secret"),
        Err(DatabaseError::AttributeDecryption(_))
    ));
    wrong_key.setVisible(&["id"]);
    assert_eq!(wrong_key.toJson().unwrap(), r#"{"id":1}"#);
    let before = wrong_key.state().getAttributes().clone();
    assert!(matches!(
        wrong_key.fill(Attributes::from([
            ("plain->a".into(), 1.into()),
            ("settings->mode".into(), "blocked".into()),
        ])),
        Err(DatabaseError::AttributeDecryption(_))
    ));
    assert_eq!(wrong_key.state().getAttributes(), &before);
    verify_json_edits(&mut rotating);
    rotating
        .setAttributeValue("secret", "private sentinel")
        .unwrap();
    rotating
        .setAttributeValue("items", Value::Json(json!({"team": "ops"})))
        .unwrap();
    assert!(rotating.isDirty().unwrap());
    rotating.save().await.unwrap();
    let new_key_only = Record::queryOn(&wrong).findOrFail(1).await.unwrap();
    assert_eq!(
        new_key_only.getAttributeValue("secret").unwrap(),
        Some("private sentinel".into())
    );
    assert_eq!(
        new_key_only.getAttributeValue("class_items").unwrap(),
        Some(Value::Json(
            json!({"team":{"name":"support","rank":1},"added":true})
        ))
    );
    assert_eq!(
        new_key_only.getAttributeValue("class_array").unwrap(),
        Some(Value::Json(json!(["east", "west", "north"])))
    );
    assert_eq!(
        new_key_only.getAttributeValue("plain").unwrap(),
        Some(Value::Json(json!({"list":[1,null],"a":1})))
    );
    assert!(new_key_only.getAttribute("plain->a").is_none());
    assert_eq!(
        new_key_only.getAttributeValue("settings").unwrap(),
        Some(Value::Json(json!({"mode":"on","nested":{"flag":true}})))
    );
    assert!(
        Record::queryOn(&old)
            .findOrFail(1)
            .await
            .unwrap()
            .getAttributeValue("secret")
            .is_err()
    );

    let (_missing_app, missing) = application(&config, None, &[]).await;
    let mut empty = Record::queryOn(&missing).make(Attributes::new()).unwrap();
    empty.setAttributeValue("secret", Value::Null).unwrap();
    assert_eq!(
        empty.getAttributeValue("secret").unwrap(),
        Some(Value::Null)
    );
    assert!(matches!(
        empty.setAttributeValue("secret", "private sentinel"),
        Err(DatabaseError::AttributeEncryption(_))
    ));
    assert_eq!(empty.getAttribute("secret"), Some(&Value::Null));
    let mut malformed = Record::queryOn(&wrong).findOrFail(1).await.unwrap();
    malformed.setAttribute("secret", "malformed payload");
    assert!(matches!(
        malformed.getAttributeValue("secret"),
        Err(DatabaseError::AttributeDecryption(_))
    ));

    // A model continues to belong to its original application.
    drop(rotated_app);
    assert!(matches!(
        rotating.getAttributeValue("secret"),
        Err(DatabaseError::AttributeDecryption(_))
    ));
    old.statement("DROP TABLE encrypted_records", &[])
        .await
        .unwrap();
    let fresh = Encrypter::new(NEW, Cipher::Aes256Cbc).unwrap();
    assert_eq!(
        fresh
            .decryptString(ciphertext(new_key_only.getRawOriginal("secret")))
            .unwrap(),
        b"private sentinel"
    );
}

fn verify_json_edits(record: &mut Record) {
    record.setAttributeValue("plain", "east").unwrap();
    record.fillJsonAttribute("plain->-1", "y").unwrap();
    assert_eq!(
        record.getAttributeValue("plain").unwrap(),
        Some(Value::Json(json!("easy")))
    );
    record.setAttributeValue("plain", Value::Null).unwrap();

    let array = record.getAttribute("class_array").cloned().unwrap();
    record
        .setAttributeValue("class_array", "not an array")
        .unwrap();
    assert!(record.getEncryptedCollection("class_array").is_err());
    record.setAttribute("class_array", array);

    let before = record.state().getAttributes().clone();
    assert!(
        record
            .fillJsonAttribute("class_items->team..name", "bad")
            .is_err()
    );
    assert_eq!(record.state().getAttributes(), &before);
    record
        .fill(Attributes::from([
            ("class_items->team->name".into(), "support".into()),
            ("plain->a".into(), 1.into()),
            ("class_items->guarded".into(), true.into()),
        ]))
        .unwrap();
    record
        .editEncryptedCollection("class_items", |items| {
            items.put("added", json!(true));
            Ok(())
        })
        .unwrap();
    record.fillJsonAttribute("class_array->2", "north").unwrap();
    record.fillJsonAttribute("plain->list->0", 1).unwrap();
    record
        .setAttributeValue("plain->list->1", Value::Null)
        .unwrap();
    record
        .setAttributeValue("settings->nested.flag", true)
        .unwrap();
    let raw = ciphertext(record.getAttribute("class_items"));
    assert!(Encrypter::appearsEncrypted(raw));
    assert!(!raw.contains("support"));
    assert!(record.getAttribute("class_items->team->name").is_none());
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

use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{AsFluent, AttributeCast},
        },
    },
    support::Fluent,
};
use serde_json::{Value as Json, json};

struct Record(ModelState);

impl Model for Record {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("fluent_records")
            .timestamps(None, None)
            .fillable(&["options"]);
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
        static CASTS: &[(&str, AttributeCast)] = &[("options", AsFluent::cast())];
        CASTS
    }
}

async fn verify(config: Json, id: &str) {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"models","connections":{"models":config}}),
    )]));
    let manager = DatabaseManager::new(Arc::new(settings), ".");
    let db = manager.connection(None).unwrap();
    db.statement("DROP TABLE IF EXISTS fluent_records", &[])
        .await
        .unwrap();
    db.statement(
        &format!("CREATE TABLE fluent_records (id {id}, options TEXT)"),
        &[],
    )
    .await
    .unwrap();
    let query = || Record::queryOn(&db);
    let payload = json!({
        "z": {"enabled": null},
        "z.enabled": "literal",
        "items": [{"name":"first"},{"name":"last"}],
        "indexed": {"0": "item"},
        "empty": {}
    });
    let record = query()
        .create(Attributes::from([("options".into(), Value::Json(payload))]))
        .await
        .unwrap();
    let key = record.getKey().unwrap().clone();
    let mut record = query().findOrFail(key.clone()).await.unwrap();
    let options = record.getValueObject::<Fluent>("options").unwrap().unwrap();
    assert!(
        options
            .toJson()
            .unwrap()
            .starts_with(r#"{"z":{"enabled":null},"z.enabled":"literal","#)
    );
    assert_eq!(options.get("z.enabled"), Some(Json::Null));
    assert_eq!(options.value("z.enabled"), Some(&json!("literal")));
    assert!(options.has("z.enabled"));
    assert!(!options.has("z.missing"));
    assert_eq!(options.get("items.*.name"), Some(json!(["first", "last"])));
    assert_eq!(options.get("items.{last}.name"), Some(json!("last")));
    assert_eq!(options.scope("indexed").toArray(), json!(["item"]));
    assert_eq!(options.value("empty"), Some(&json!([])));
    assert!(!record.isDirty().unwrap());

    record
        .editValueObject::<Fluent>("options", |options| {
            options.set("items.*.enabled", json!(true)).unwrap();
            let before = options.toArray();
            assert!(options.set(&"path.".repeat(128), json!(true)).is_err());
            assert_eq!(options.toArray(), before);
            Ok(())
        })
        .unwrap();
    assert!(record.isDirty().unwrap());
    record
        .setAttributeValue("options->z->enabled", true)
        .unwrap();
    assert_eq!(
        record
            .getValueObject::<Fluent>("options")
            .unwrap()
            .unwrap()
            .get("z.enabled"),
        Some(json!(true))
    );
    assert!(record.save().await.unwrap());
    let mut record = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(
        record.toArray().unwrap()["options"]["items"][1]["enabled"],
        true
    );
    assert_eq!(record.toArray().unwrap()["options"]["z"]["enabled"], true);

    record.setAttributeValue("options", "scalar").unwrap();
    assert!(record.save().await.unwrap());
    let mut record = query().findOrFail(key.clone()).await.unwrap();
    assert!(record.getValueObject::<Fluent>("options").is_err());
    assert!(
        record
            .setAttributeValue("options", Value::Real(f64::NAN))
            .is_err()
    );
    assert!(record.getValueObject::<Fluent>("options").is_err());
    record
        .setValueObject("options", Some(Fluent::default()))
        .unwrap();
    assert!(record.save().await.unwrap());
    let mut record = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(record.toArray().unwrap()["options"], json!([]));
    record.setAttributeValue("options", Value::Null).unwrap();
    assert!(record.save().await.unwrap());
    let mut record = query().findOrFail(key).await.unwrap();
    assert!(
        record
            .getValueObject::<Fluent>("options")
            .unwrap()
            .is_none()
    );
    assert_eq!(record.getAttribute("options"), Some(&Value::Null));
    db.statement("DROP TABLE fluent_records", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn fluent_model_cast_on_sqlite() {
    verify(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn fluent_model_cast_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}

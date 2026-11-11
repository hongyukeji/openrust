use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{AsStringable, AttributeCast},
        },
    },
    support::Stringable,
};
use serde_json::{Value as Json, json};

struct Record(ModelState);

impl Model for Record {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("stringable_records")
            .timestamps(None, None)
            .fillable(&["label"]);
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
        static CASTS: &[(&str, AttributeCast)] = &[("label", AsStringable::cast())];
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
    db.statement("DROP TABLE IF EXISTS stringable_records", &[])
        .await
        .unwrap();
    db.statement(
        &format!("CREATE TABLE stringable_records (id {id}, label TEXT)"),
        &[],
    )
    .await
    .unwrap();
    let query = || Record::queryOn(&db);
    let label = "\u{feff} \u{200b}甲\u{200b}乙\u{a0}";
    let record = query()
        .create(Attributes::from([("label".into(), label.into())]))
        .await
        .unwrap();
    let key = record.getKey().unwrap().clone();
    let mut record = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(record.getAttribute("label"), Some(&label.into()));
    let text = record
        .getValueObject::<Stringable>("label")
        .unwrap()
        .unwrap();
    assert_eq!(text.toString(), label);
    let trimmed = text.into_owned().trim();
    assert_eq!(trimmed.value(), "甲\u{200b}乙");
    assert_eq!(
        serde_json::to_value(&trimmed).unwrap(),
        json!("甲\u{200b}乙")
    );
    assert!(!record.isDirty().unwrap());
    record.setValueObject("label", Some(trimmed)).unwrap();
    assert!(record.isDirty().unwrap());
    assert!(record.save().await.unwrap());
    let mut record = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(record.toArray().unwrap()["label"], json!("甲\u{200b}乙"));

    assert!(
        record
            .setAttributeValue("label", Value::Json(json!({"no":"text"})))
            .is_err()
    );
    assert_eq!(record.toArray().unwrap()["label"], json!("甲\u{200b}乙"));
    for (value, expected) in [(Value::Bool(false), ""), (Value::Integer(42), "42")] {
        record.setAttributeValue("label", value).unwrap();
        assert!(record.save().await.unwrap());
        record = query().findOrFail(key.clone()).await.unwrap();
        assert_eq!(record.toArray().unwrap()["label"], json!(expected));
    }
    record.setAttributeValue("label", Value::Null).unwrap();
    assert!(record.save().await.unwrap());
    let mut record = query().findOrFail(key).await.unwrap();
    assert!(
        record
            .getValueObject::<Stringable>("label")
            .unwrap()
            .is_none()
    );
    assert_eq!(record.getAttribute("label"), Some(&Value::Null));
    assert_eq!(record.toArray().unwrap()["label"], Json::Null);
    db.statement("DROP TABLE stringable_records", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn stringable_model_cast_on_sqlite() {
    verify(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn stringable_model_cast_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}

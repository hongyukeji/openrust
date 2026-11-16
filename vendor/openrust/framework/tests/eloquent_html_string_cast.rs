use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    contracts::support::Htmlable,
    database::{
        DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{AsHtmlString, AttributeCast},
        },
    },
    support::{HtmlString, Stringable},
};
use serde_json::{Value as Json, json};

struct Record(ModelState);

impl Model for Record {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("html_string_records")
            .timestamps(None, None)
            .fillable(&["body"]);
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
        static CASTS: &[(&str, AttributeCast)] = &[("body", AsHtmlString::cast())];
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
    db.statement("DROP TABLE IF EXISTS html_string_records", &[])
        .await
        .unwrap();
    db.statement(
        &format!("CREATE TABLE html_string_records (id {id}, body TEXT)"),
        &[],
    )
    .await
    .unwrap();

    let original = "<em>甲 & 乙</em>";
    let record = Record::queryOn(&db)
        .create(Attributes::from([("body".into(), original.into())]))
        .await
        .unwrap();
    let key = record.getKey().unwrap().clone();
    let reload = || Record::queryOn(&db).findOrFail(key.clone());
    let mut record = reload().await.unwrap();
    let html = record
        .getValueObject::<HtmlString>("body")
        .unwrap()
        .unwrap();
    assert_eq!(Htmlable::toHtml(html.as_ref()), original);
    assert_eq!(html.to_string(), original);
    assert!(html.isNotEmpty());
    assert_eq!(serde_json::to_value(&html).unwrap(), json!(original));
    assert!(HtmlString::default().isEmpty());
    assert!(!record.isDirty().unwrap());

    record
        .setValueObject("body", Some(HtmlString::new(original)))
        .unwrap();
    assert!(!record.isDirty().unwrap());
    let updated = Stringable::from("<strong>丙</strong>");
    record
        .setAttributeValue("body", updated.toString())
        .unwrap();
    assert!(record.isDirty().unwrap());
    assert!(record.save().await.unwrap());
    let mut record = reload().await.unwrap();
    assert_eq!(
        record.getAttribute("body"),
        Some(&updated.toString().into())
    );
    assert_eq!(record.toArray().unwrap()["body"], json!(updated.toString()));
    let serialized: Json = serde_json::from_str(&record.toJson().unwrap()).unwrap();
    assert_eq!(serialized["body"], json!(updated.toString()));
    assert!(
        record
            .setAttributeValue("body", Value::Json(json!([])))
            .is_err()
    );
    assert!(!record.isDirty().unwrap());

    for (value, expected) in [(Value::Bool(false), ""), (Value::Integer(17), "17")] {
        record.setAttributeValue("body", value).unwrap();
        assert_eq!(record.toArray().unwrap()["body"], json!(expected));
    }
    record.setValueObject::<HtmlString>("body", None).unwrap();
    assert!(record.save().await.unwrap());
    let mut record = reload().await.unwrap();
    assert!(
        record
            .getValueObject::<HtmlString>("body")
            .unwrap()
            .is_none()
    );
    assert_eq!(record.toArray().unwrap()["body"], Json::Null);
    db.statement("DROP TABLE html_string_records", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn html_string_model_cast_on_sqlite() {
    verify(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn html_string_model_cast_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}

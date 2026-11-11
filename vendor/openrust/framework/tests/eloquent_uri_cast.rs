use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{AsUri, AttributeCast},
        },
    },
    support::Uri,
};
use serde_json::{Value as Json, json};

struct Record(ModelState);

impl Model for Record {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("uri_records")
            .timestamps(None, None)
            .fillable(&["link"]);
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
        static CASTS: &[(&str, AttributeCast)] = &[("link", AsUri::cast())];
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
    db.statement("DROP TABLE IF EXISTS uri_records", &[])
        .await
        .unwrap();
    db.statement(
        &format!("CREATE TABLE uri_records (id {id}, link TEXT)"),
        &[],
    )
    .await
    .unwrap();
    let query = || Record::queryOn(&db);
    let raw = "HTTPS://Example.com:443/orders?";
    let mut record = query()
        .create(Attributes::from([("link".into(), raw.into())]))
        .await
        .unwrap();
    let key = record.getKey().unwrap().clone();
    assert_eq!(record.getAttribute("link"), Some(&raw.into()));
    let uri = record.getValueObject::<Uri>("link").unwrap().unwrap();
    assert_eq!(uri.host(), Some("example.com"));
    assert_eq!(uri.port(), None);
    assert_eq!(uri.queryString(), Some(""));
    assert_eq!(
        record.toArray().unwrap()["link"],
        json!("https://example.com/orders?")
    );

    record
        .setValueObject("link", Some(Uri::of("../new?#").unwrap()))
        .unwrap();
    assert!(record.save().await.unwrap());
    record = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(record.toArray().unwrap()["link"], json!("../new?#"));
    assert_eq!(
        record
            .getValueObject::<Uri>("link")
            .unwrap()
            .unwrap()
            .fragment(),
        Some("")
    );

    record.setAttributeValue("link", "http://").unwrap();
    assert_eq!(record.getAttribute("link"), Some(&"http://".into()));
    assert!(record.getValueObject::<Uri>("link").is_err());
    assert!(record.toArray().is_err());
    assert!(
        record
            .setAttributeValue("link", Value::Json(json!([])))
            .is_err()
    );
    assert_eq!(record.getAttribute("link"), Some(&"http://".into()));

    record.setAttributeValue("link", Value::Null).unwrap();
    assert!(record.save().await.unwrap());
    record = query().findOrFail(key).await.unwrap();
    assert!(record.getValueObject::<Uri>("link").unwrap().is_none());
    assert_eq!(record.toArray().unwrap()["link"], Json::Null);
    db.statement("DROP TABLE uri_records", &[]).await.unwrap();
}

#[tokio::test]
async fn uri_cast_preserves_reference_components_on_sqlite() {
    verify(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn uri_cast_preserves_reference_components_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}

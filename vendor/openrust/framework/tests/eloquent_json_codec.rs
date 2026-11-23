use std::sync::Arc;

use openrust::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{
                AsCollection, AsEncryptedCollection, AsEnumCollection, AsFluent, AttributeCast,
                CastableEnum, Json,
            },
        },
    },
    foundation::Application,
    support::{Collection, Fluent},
};
use serde_json::{Value as Document, json};

struct Record(ModelState);
struct Status;

impl CastableEnum for Status {
    fn from_value(value: &Value) -> DatabaseResult<Self> {
        if value == &Value::Text("active".into()) {
            Ok(Self)
        } else {
            Err(DatabaseError::InvalidQuery("status"))
        }
    }

    fn into_value(self) -> Value {
        Value::Text("active".into())
    }
}

impl Model for Record {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("json_codec_records")
            .incrementing(false)
            .timestamps(None, None)
            .fillable(&[
                "id",
                "payload",
                "native_payload",
                "items",
                "secret",
                "states",
                "options",
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
        static CASTS: &[(&str, AttributeCast)] = &[
            ("payload", AttributeCast::Json),
            ("native_payload", AttributeCast::Json),
            ("items", AsCollection::cast()),
            ("secret", AsEncryptedCollection::cast()),
            ("states", AsEnumCollection::of::<Status>()),
            ("options", AsFluent::cast()),
        ];
        CASTS
    }
}

async fn application(config: &Document, marker: Option<&'static str>) -> (Application, Connection) {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let container = app.container();
    let repository = container.get::<dyn Repository>("config").unwrap();
    repository
        .set("app.key", json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))
        .unwrap();
    repository.set("app.cipher", json!("AES-256-CBC")).unwrap();
    repository.set("database.default", json!("codec")).unwrap();
    repository
        .set("database.connections.codec", config.clone())
        .unwrap();
    if let Some(marker) = marker {
        let codec = Json::default()
            .encode_using(move |value| {
                if value == "reject" {
                    return Err(DatabaseError::InvalidQuery("codec rejected value"));
                }
                Json::default().encode(&json!({"format":marker, "document":value}))
            })
            .decode_using(move |text| {
                let mut value = Json::default().decode(text)?;
                if value["format"] != marker {
                    return Err(DatabaseError::InvalidQuery("codec format mismatch"));
                }
                value
                    .as_object_mut()
                    .and_then(|value| value.remove("document"))
                    .ok_or(DatabaseError::InvalidQuery("codec document missing"))
            });
        container.instance(Arc::new(codec)).unwrap();
    }
    let db = container
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    (app, db)
}

async fn verify(config: Document, native_type: &str) {
    let (_first, first) = application(&config, Some("first")).await;
    let (_second, second) = application(&config, Some("second")).await;
    let (_default, default) = application(&config, None).await;
    first
        .statement("DROP TABLE IF EXISTS json_codec_records", &[])
        .await
        .unwrap();
    first.statement(
        &format!("CREATE TABLE json_codec_records (id BIGINT PRIMARY KEY, payload TEXT, native_payload {native_type}, items TEXT, secret TEXT, states TEXT, options TEXT)"),
        &[],
    ).await.unwrap();

    let contents = |id: i64| {
        Attributes::from([
            ("id".into(), id.into()),
            (
                "payload".into(),
                Value::Json(json!({"name":"record","count":1})),
            ),
            ("native_payload".into(), Value::Json(json!({"native":true}))),
            ("items".into(), Value::Json(json!({"team":"ops"}))),
            ("secret".into(), Value::Json(json!(["redacted"]))),
            ("states".into(), Value::Json(json!({"ready":"active"}))),
            ("options".into(), Value::Json(json!({"mail":false}))),
        ])
    };
    let (one, two, three) = tokio::join!(
        Record::query_on(&first).create(contents(1)),
        Record::query_on(&second).create(contents(2)),
        Record::query_on(&default).create(contents(3)),
    );
    assert!(one.is_ok() && two.is_ok() && three.is_ok());
    let (one, two, three) = tokio::join!(
        Record::query_on(&first).find_or_fail(1),
        Record::query_on(&second).find_or_fail(2),
        Record::query_on(&default).find_or_fail(3),
    );
    let mut one = one.unwrap();
    let two = two.unwrap();
    let three = three.unwrap();
    let expected = json!({"name":"record","count":1});
    for (record, marker) in [(&one, "first"), (&two, "second")] {
        let raw = record.get_raw_original("payload").unwrap();
        let Value::Text(text) = raw else {
            panic!("expected stored text")
        };
        assert_eq!(Json::default().decode(text).unwrap()["format"], marker);
        let data = record.to_array().unwrap();
        assert_eq!(data["payload"], expected);
        assert_eq!(data["native_payload"], json!({"native":true}));
        assert_eq!(data["secret"], json!(["redacted"]));
    }
    assert_eq!(three.to_array().unwrap()["payload"], expected);
    let foreign = Record::query_on(&second).find_or_fail(1).await.unwrap();
    assert!(foreign.to_array().is_err());
    assert!(foreign.get_encrypted_collection("secret").is_err());
    assert!(
        one.get_enum_collection::<Status>("states")
            .unwrap()
            .unwrap()
            .has("ready")
    );
    assert_eq!(
        one.get_json_collection("items")
            .unwrap()
            .unwrap()
            .to_array()
            .unwrap(),
        json!({"team":"ops"})
    );
    assert!(one.get_value_object::<Fluent>("options").unwrap().is_some());

    let before = one.state().get_attributes().clone();
    assert!(one.set_attribute_value("payload", "reject").is_err());
    assert!(one.set_attribute_value("secret", "reject").is_err());
    assert_eq!(one.state().get_attributes(), &before);
    one.fill_json_attribute("payload->count", 2).unwrap();
    one.fill_json_attribute("secret->0", "updated").unwrap();
    one.set_json_collection("items", Collection::from_value(json!(["next"])))
        .unwrap();
    one.set_enum_collection("states", Collection::from_entries([("next", Status)]))
        .unwrap();
    one.set_value_object("options", Some(Fluent::make(json!({"mail":true})).unwrap()))
        .unwrap();
    assert!(one.save().await.unwrap());
    let one = Record::query_on(&first).find_or_fail(1).await.unwrap();
    let serialized: Document = serde_json::from_str(&one.to_json().unwrap()).unwrap();
    assert_eq!(serialized["payload"], json!({"name":"record","count":2}));
    assert_eq!(serialized["secret"], json!(["updated"]));
    assert_eq!(serialized["items"], json!(["next"]));
    assert_eq!(serialized["states"], json!({"next":"active"}));
    assert_eq!(serialized["options"], json!({"mail":true}));

    first
        .statement("DROP TABLE json_codec_records", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn json_cast_codec_on_sqlite() {
    let path =
        std::env::temp_dir().join(format!("openrust-json-codec-{}.sqlite", fastrand::u64(..)));
    std::fs::File::create_new(&path).unwrap();
    verify(json!({"driver":"sqlite","database":path}), "TEXT").await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn json_cast_codec_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), "JSON").await;
    }
}

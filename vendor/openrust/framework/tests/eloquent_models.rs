use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock, Weak},
};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Builder, Model, ModelMetadata, ModelState, Scope,
            casts::{
                AsArrayObject, AsBinary, AsCollection, AsEnumArrayObject, AsEnumCollection,
                AsVector, Attribute, AttributeCast, BinaryCodec, BinaryFormat, CastWrite, Castable,
                CastableEnum, CastableObject, CastsAttributes, CastsInboundAttributes, ClassCast,
                EnumCast, ValueObjectCast,
            },
            relations::Relation,
        },
        query::{Column, Expression},
    },
    support::{ArrayKey, Collection},
};
use serde_json::{Value as Json, json};

struct Article(ModelState);

struct SelfReference(Arc<OnceLock<Weak<Article>>>);

impl Relation for SelfReference {
    type Parent = Article;
    type Output = Arc<OnceLock<Weak<Article>>>;

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a Article],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(async move { Ok(parents.iter().map(|_| Arc::clone(&self.0)).collect()) })
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<Json>> {
        let model = output
            .get()
            .and_then(Weak::upgrade)
            .ok_or(DatabaseError::InvalidQuery("uninitialized self relation"))?;
        model.toArray().map(Json::Object).map(Some)
    }
}

#[tokio::test]
async fn model_json_stops_at_repeated_models_and_retries_projection_errors() {
    let reference = Arc::new(OnceLock::new());
    let relation = SelfReference(reference.clone());
    let mut model = Article::newInstance(attrs([("title", "Post".into())])).unwrap();
    model.load("selfReference", &relation).await.unwrap();
    assert!(model.toJson().is_err());
    let model = Arc::new(model);
    assert!(reference.set(Arc::downgrade(&model)).is_ok());
    let expected = json!({"title": "Post", "self_reference": {"title": "Post"}});
    for _ in 0..2 {
        assert_eq!(
            serde_json::from_str::<Json>(&model.toJson().unwrap()).unwrap(),
            expected
        );
    }
}

struct CastRecord(ModelState);

#[derive(Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct CollectionItem {
    name: String,
}

fn filter_collection(mut values: Collection) -> DatabaseResult<Collection> {
    values.forget("skip");
    Ok(values)
}

fn upper_collection_item(value: &Json, _: &ArrayKey) -> DatabaseResult<Json> {
    value
        .as_str()
        .map(|value| json!(value.to_ascii_uppercase()))
        .ok_or(DatabaseError::InvalidQuery("collection item string"))
}

struct Scaled(i64);

impl Castable for Scaled {
    type Arguments = i64;
    type Caster = Self;

    fn castUsing(factor: &i64) -> DatabaseResult<Self> {
        if *factor > 0 {
            Ok(Self(*factor))
        } else {
            Err(DatabaseError::InvalidQuery("invalid scale"))
        }
    }
}

impl CastsInboundAttributes for Scaled {
    fn set(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<CastWrite> {
        let Value::Integer(value) = value else {
            return Err(DatabaseError::InvalidQuery("scaled integer"));
        };
        value
            .checked_mul(self.0)
            .map(|value| CastWrite::Value(value.into()))
            .ok_or(DatabaseError::InvalidQuery("scaled integer"))
    }
}

impl CastsAttributes for Scaled {
    fn get(&self, _: &str, value: Option<&Value>, _: &Attributes) -> DatabaseResult<Value> {
        let value = match value {
            Some(Value::Integer(value)) => *value,
            Some(Value::Text(value)) => value
                .parse::<i64>()
                .map_err(|_| DatabaseError::InvalidQuery("scaled integer"))?,
            _ => return Ok(Value::Null),
        };
        Ok((value / self.0).into())
    }

    fn serialize(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<Value> {
        let Value::Integer(value) = value else {
            return Err(DatabaseError::InvalidQuery("scaled integer"));
        };
        Ok(Value::Json(json!({"decoded":value})))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Codes {
    left: String,
    right: String,
}

impl CastableObject for Codes {
    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        match (raw.get(name), raw.get("right_code")) {
            (Some(Value::Text(left)), Some(Value::Text(right))) => Ok(Some(Self {
                left: left.clone(),
                right: right.clone(),
            })),
            _ => Ok(None),
        }
    }

    fn toAttributes(value: Option<&Self>, name: &str, _: &Attributes) -> DatabaseResult<CastWrite> {
        let (left, right) = match value {
            Some(value) if !value.left.is_empty() => {
                (value.left.clone().into(), value.right.clone().into())
            }
            Some(_) => return Err(DatabaseError::InvalidQuery("empty left code")),
            None => (Value::Null, Value::Null),
        };
        Ok(CastWrite::Patch(attrs([
            (name, left),
            ("right_code", right),
        ])))
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        Ok(Value::Json(json!({"left": self.left, "right": self.right})))
    }
}

#[derive(Clone)]
struct FreshCodes(Codes);

impl CastableObject for FreshCodes {
    const WITHOUT_OBJECT_CACHING: bool = true;

    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        Codes::fromAttributes(name, raw).map(|value| value.map(Self))
    }

    fn toAttributes(
        value: Option<&Self>,
        name: &str,
        raw: &Attributes,
    ) -> DatabaseResult<CastWrite> {
        Codes::toAttributes(value.map(|value| &value.0), name, raw)
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        self.0.serialize()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Status {
    Draft,
    Published,
}

impl CastableEnum for Status {
    fn fromValue(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Text(name) if name == "Draft" => Ok(Self::Draft),
            Value::Text(name) if name == "Published" => Ok(Self::Published),
            _ => Err(DatabaseError::InvalidQuery("status case")),
        }
    }

    fn intoValue(self) -> Value {
        match self {
            Self::Draft => "Draft".into(),
            Self::Published => "Published".into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Priority {
    Normal,
    Urgent,
}

impl CastableEnum for Priority {
    fn fromValue(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Integer(1) => Ok(Self::Normal),
            Value::Integer(2) => Ok(Self::Urgent),
            _ => Err(DatabaseError::InvalidQuery("priority backing value")),
        }
    }

    fn intoValue(self) -> Value {
        match self {
            Self::Normal => 1.into(),
            Self::Urgent => 2.into(),
        }
    }
}

struct CodePair;

impl CastsInboundAttributes for CodePair {
    fn set(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<CastWrite> {
        let Value::Text(value) = value else {
            return Err(DatabaseError::InvalidQuery("code pair"));
        };
        let (left, right) = value
            .split_once('/')
            .ok_or(DatabaseError::InvalidQuery("code pair"))?;
        Ok(CastWrite::Patch(attrs([
            ("left_code", left.trim().into()),
            ("right_code", right.trim().into()),
        ])))
    }
}

impl CastsAttributes for CodePair {
    fn get(&self, _: &str, _: Option<&Value>, raw: &Attributes) -> DatabaseResult<Value> {
        match (raw.get("left_code"), raw.get("right_code")) {
            (Some(Value::Text(left)), Some(Value::Text(right))) => {
                Ok(format!("{left}/{right}").into())
            }
            _ => Ok(Value::Null),
        }
    }
}

struct CanonicalCode;

impl CastsInboundAttributes for CanonicalCode {
    fn set(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<CastWrite> {
        match value {
            Value::Text(text) => Ok(CastWrite::Value(text.trim().to_ascii_lowercase().into())),
            Value::Null => Ok(CastWrite::Value(Value::Null)),
            _ => Err(DatabaseError::InvalidQuery("canonical code")),
        }
    }
    fn compare(&self, _: &str, original: &Value, current: &Value) -> DatabaseResult<Option<bool>> {
        Ok(match (original, current) {
            (Value::Text(original), Value::Text(current)) => {
                Some(original.eq_ignore_ascii_case(current))
            }
            _ => None,
        })
    }
}

impl CastsAttributes for CanonicalCode {
    fn get(&self, _: &str, value: Option<&Value>, _: &Attributes) -> DatabaseResult<Value> {
        match value {
            Some(Value::Text(text)) => Ok(text.to_ascii_uppercase().into()),
            _ => Ok(Value::Null),
        }
    }

    fn serialize(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<Value> {
        let Value::Text(code) = value else {
            return Err(DatabaseError::InvalidQuery("serialized code"));
        };
        Ok(Value::Json(json!({"code": code})))
    }
}

impl Model for CastRecord {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("cast_records")
            .timestamps(None, None)
            .dateFormat("%Y-%m-%d %H:%M:%S%.6f")
            .hidden(&["label", "invalid_blob"])
            .appends(&["summary"])
            .fillable(&[
                "label",
                "number",
                "active",
                "options",
                "measure",
                "missing",
                "amount",
                "occurred_at",
                "day",
                "epoch",
                "status",
                "priority",
                "pair",
                "code",
                "inbound_code",
                "enum_list",
                "enum_array",
                "factory_value",
                "factory_inbound",
                "json_collection",
                "json_array",
                "json_nullable",
                "json_mapped",
                "json_callback",
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
            ("label", AttributeCast::String),
            ("number", AttributeCast::Integer),
            ("active", AttributeCast::Boolean),
            ("options", AttributeCast::Json),
            ("measure", AttributeCast::Float),
            ("amount", AttributeCast::Decimal(2)),
            ("occurred_at", AttributeCast::DateTime),
            ("day", AttributeCast::Date),
            ("epoch", AttributeCast::Timestamp),
            ("missing", AttributeCast::String),
            ("status", AttributeCast::Enum(EnumCast::of::<Status>())),
            ("priority", AttributeCast::Enum(EnumCast::of::<Priority>())),
            ("pair", AttributeCast::Custom(&CodePair)),
            ("code", AttributeCast::Custom(&CanonicalCode)),
            ("inbound_code", AttributeCast::Inbound(&CanonicalCode)),
            ("enum_list", AsEnumCollection::of::<Status>()),
            ("enum_array", AsEnumArrayObject::of::<Priority>()),
            ("left_code", ValueObjectCast::of::<Codes>()),
            ("factory_value", ClassCast::of::<Scaled>(&2)),
            ("factory_inbound", ClassCast::inbound::<Scaled>(&4)),
            ("json_collection", AsCollection::cast()),
            ("json_array", AsArrayObject::cast()),
            ("json_nullable", AsArrayObject::nullable()),
            ("json_mapped", AsCollection::of::<CollectionItem>()),
            (
                "json_callback",
                AsCollection::using(filter_collection, Some(upper_collection_item)),
            ),
        ];
        CASTS
    }

    fn serializeDate(&self, date: chrono::DateTime<chrono::Utc>) -> DatabaseResult<String> {
        Ok(date.format("%Y-%m-%d").to_string())
    }

    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "label" => Some(Attribute::make(
                Some(|value, _| match value {
                    Some(Value::Text(value)) => Ok(value.to_uppercase().into()),
                    _ => Ok(Value::Null),
                }),
                Some(|value, _| match value {
                    Value::Text(value) => {
                        Ok(attrs([("label", value.trim().to_lowercase().into())]))
                    }
                    _ => Err(DatabaseError::InvalidQuery("label attribute")),
                }),
            )),
            "summary" => Some(Attribute::get(|_, raw| {
                Ok(raw.get("label").cloned().unwrap_or(Value::Null))
            })),
            _ => None,
        }
    }
}

impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("articles")
            .primaryKey("article_id")
            .fillable(&["title", "account", "status"])
            .softDeletes("deleted_at");
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

    fn globalScopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("account", &CurrentAccount)]
    }
}

struct CurrentAccount;

impl Scope<Article> for CurrentAccount {
    fn apply(&self, builder: &mut Builder<'_, Article>) -> DatabaseResult<()> {
        builder.r#where("account", "=", 1);
        Ok(())
    }
}

struct Published;

impl Scope<Article> for Published {
    fn apply(&self, builder: &mut Builder<'_, Article>) -> DatabaseResult<()> {
        builder
            .r#where("status", "=", "published")
            .orWhere("status", "=", "review");
        Ok(())
    }
}

struct Label(ModelState);

impl Model for Label {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("labels")
            .primaryKey("slug")
            .incrementing(false)
            .timestamps(None, None);
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
}

fn connect(config: Json) -> Connection {
    connect_with_timezone(config, "America/New_York")
}

fn connect_with_timezone(config: Json, timezone: &str) -> Connection {
    let settings = Repository::new(BTreeMap::from([
        (
            "database".into(),
            json!({"default":"models","connections":{"models":config}}),
        ),
        ("app".into(), json!({"timezone":timezone})),
    ]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

fn attrs<const N: usize>(values: [(&str, Value); N]) -> Attributes {
    values
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}

async fn verify_casts(db: &Connection, mysql: bool) {
    db.statement("DROP TABLE IF EXISTS cast_records", &[])
        .await
        .unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    let json_type = if mysql { "JSON" } else { "TEXT" };
    let decimal_type = if mysql { "DECIMAL(38, 5)" } else { "TEXT" };
    db.statement(&format!(
        "CREATE TABLE cast_records (id {id}, label VARCHAR(80), number VARCHAR(80), active VARCHAR(20), options {json_type}, measure VARCHAR(80), missing VARCHAR(20), amount {decimal_type}, occurred_at DATETIME(6), day DATETIME(6), epoch VARCHAR(40), status VARCHAR(20), priority BIGINT, left_code VARCHAR(40), right_code VARCHAR(40), code VARCHAR(40), inbound_code VARCHAR(40), enum_list TEXT, enum_array TEXT, factory_value BIGINT, factory_inbound BIGINT, json_collection TEXT, json_array {json_type}, json_nullable {json_type}, json_mapped {json_type}, json_callback {json_type}, binary_uuid BLOB, binary_ulid BINARY(16), binary_label BLOB, embedding TEXT, embedding_blob BLOB)"
    ), &[]).await.unwrap();
    let query = || CastRecord::queryOn(db);
    let model = query()
        .create(attrs([
            ("label", "  Hello ".into()),
            ("number", "  42.8x".into()),
            ("active", "false".into()),
            ("options", Value::Json(json!({"mode":"demo"}))),
            ("measure", "1.25e2x".into()),
            ("amount", "9007199254740993.125".into()),
            ("occurred_at", "2025-06-01T12:30:00.123456Z".into()),
            ("day", "2025-06-01 15:00:00".into()),
            ("epoch", "-0.5".into()),
        ]))
        .await
        .unwrap();
    let mut loaded = query()
        .findOrFail(model.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        loaded.getRawOriginal("occurred_at"),
        Some(&"2025-06-01 08:30:00.123456".into())
    );
    assert_eq!(
        loaded.getAttributeValue("occurred_at").unwrap(),
        Some("2025-06-01T12:30:00.123456Z".into())
    );
    assert_eq!(
        loaded.getAttributeValue("day").unwrap(),
        Some("2025-06-01T04:00:00.000000Z".into())
    );
    assert_eq!(
        loaded.getAttributeValue("epoch").unwrap(),
        Some((-1).into())
    );
    loaded
        .setAttributeValue("occurred_at", "2025-06-01T14:30:00.123456+02:00")
        .unwrap();
    assert!(loaded.originalIsEquivalent("occurred_at").unwrap());
    let utc = connect_with_timezone(json!({"driver":"sqlite","database":":memory:"}), "UTC");
    let other = CastRecord::queryOn(&utc)
        .make(attrs([(
            "occurred_at",
            "2025-06-01T12:30:00.123456Z".into(),
        )]))
        .unwrap();
    assert_eq!(
        other.getAttribute("occurred_at"),
        Some(&"2025-06-01 12:30:00.123456".into())
    );
    assert_eq!(
        other.getAttributeValue("occurred_at").unwrap(),
        loaded.getAttributeValue("occurred_at").unwrap()
    );
    assert!(
        loaded
            .setAttributeValue("occurred_at", "2025-03-09 02:30:00")
            .is_err()
    );
    assert!(!loaded.isDirty().unwrap());
    let mut fallback = query().make(Attributes::new()).unwrap();
    fallback
        .setAttributeValue("occurred_at", "2025-11-02 01:30:00")
        .unwrap();
    assert_eq!(
        fallback.getAttributeValue("occurred_at").unwrap(),
        Some("2025-11-02T05:30:00.000000Z".into())
    );
    assert_eq!(loaded.getAttribute("label"), Some(&"hello".into()));
    assert_eq!(
        loaded.getAttributeValue("label").unwrap(),
        Some("HELLO".into())
    );
    assert_eq!(
        loaded.getAttributeValue("summary").unwrap(),
        Some("hello".into())
    );
    assert_eq!(loaded.getAttributeValue("number").unwrap(), Some(42.into()));
    assert_eq!(
        loaded.getAttributeValue("active").unwrap(),
        Some(true.into())
    );
    assert_eq!(
        loaded.getAttributeValue("measure").unwrap(),
        Some(Value::Real(125.0))
    );
    assert_eq!(
        loaded.getAttributeValue("missing").unwrap(),
        Some(Value::Null)
    );
    assert_eq!(loaded.getAttributeValue("absent").unwrap(), None);
    assert_eq!(
        loaded.getAttributeValue("options").unwrap(),
        Some(Value::Json(json!({"mode":"demo"})))
    );
    assert!(!loaded.state().isDirty());

    assert_eq!(
        loaded.getAttributeValue("amount").unwrap(),
        Some("9007199254740993.13".into())
    );
    let raw_amount = loaded.getRawOriginal("amount").cloned().unwrap();
    loaded
        .setAttributeValue("amount", "9007199254740993.1251")
        .unwrap();
    loaded.setAttributeValue("number", 42).unwrap();
    loaded.setAttributeValue("measure", "125").unwrap();
    loaded
        .setAttributeValue("options", Value::Json(json!({"mode":"demo"})))
        .unwrap();
    assert!(loaded.state().isDirty());
    assert!(!loaded.isDirty().unwrap());
    loaded.save().await.unwrap();
    let unchanged = query()
        .findOrFail(model.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(unchanged.getRawOriginal("amount"), Some(&raw_amount));

    let mut example = query().make(Attributes::new()).unwrap();
    example.setAttributeValue("amount", "-1.235").unwrap();
    assert_eq!(
        example.getAttributeValue("amount").unwrap(),
        Some("-1.24".into())
    );
    example.setAttributeValue("amount", "1/8").unwrap();
    assert_eq!(
        example.getAttributeValue("amount").unwrap(),
        Some("0.13".into())
    );
    example.setAttributeValue("amount", "1e100000").unwrap();
    assert!(example.getAttributeValue("amount").is_err());

    assert!(
        loaded
            .fill(attrs([("active", "0".into()), ("label", 1.into())]))
            .is_err()
    );
    assert!(!loaded.state().isDirty());
    loaded.setAttributeValue("label", " World ").unwrap();
    loaded.setAttributeValue("options", "text").unwrap();
    loaded
        .setAttributeValue("amount", "9007199254740993.135")
        .unwrap();
    assert_eq!(loaded.getRawOriginal("label"), Some(&"hello".into()));
    loaded.save().await.unwrap();
    let updated = query()
        .findOrFail(model.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        updated.getAttributeValue("amount").unwrap(),
        Some("9007199254740993.14".into())
    );
    assert_eq!(
        updated.getAttributeValue("label").unwrap(),
        Some("WORLD".into())
    );
    assert_eq!(
        updated.getAttributeValue("options").unwrap(),
        Some(Value::Json(json!("text")))
    );
    verify_model_projection(updated);
    verify_query_casts(db, model.getKey().unwrap().clone()).await;
    verify_enum_and_class_casts(db).await;
    verify_enum_collections_and_objects(db).await;
    verify_factory_casts(db).await;
    verify_json_collection_casts(db).await;
    verify_binary_and_vector_casts(db).await;
    db.statement("DROP TABLE cast_records", &[]).await.unwrap();
}

async fn verify_binary_and_vector_casts(db: &Connection) {
    const UUID: &str = "550e8400-e29b-41d4-a716-446655440000";
    const ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let label = BinaryCodec::new(
        |value| {
            value
                .strip_prefix("label:")
                .map(|value| value.as_bytes().to_vec())
                .ok_or(DatabaseError::InvalidQuery("binary label"))
        },
        |bytes| {
            std::str::from_utf8(bytes)
                .map(|label| format!("label:{label}"))
                .map_err(|_| DatabaseError::InvalidQuery("binary label"))
        },
    );
    let query = || {
        CastRecord::queryOn(db).withCasts([
            ("binary_uuid", AsBinary::uuid()),
            ("binary_ulid", AsBinary::ulid()),
            ("binary_label", AsBinary::of(BinaryFormat::Custom(label))),
            ("embedding", AsVector::cast()),
            ("embedding_blob", AsVector::cast()),
        ])
    };
    let mut model = query().make(Attributes::new()).unwrap();
    model
        .setAttributeValue("binary_uuid", UUID.to_ascii_uppercase())
        .unwrap();
    model
        .setAttributeValue("binary_ulid", ULID.to_ascii_lowercase())
        .unwrap();
    model
        .setAttributeValue("binary_label", "label:example")
        .unwrap();
    model
        .setAttributeValue("embedding", Value::Json(json!([0.5, -1.25, 3.75])))
        .unwrap();
    assert_eq!(
        model.getAttributeValue("binary_uuid").unwrap(),
        Some(UUID.into())
    );
    assert_eq!(
        model.getAttributeValue("binary_ulid").unwrap(),
        Some(ULID.into())
    );
    model.setAttribute("embedding_blob", Value::Bytes(vec![0x5b, 0, 0x80, 0x3f]));
    model.save().await.unwrap();
    let key = model.getKey().unwrap().clone();
    let mut loaded = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(
        loaded.getRawOriginal("binary_uuid"),
        Some(&Value::Bytes(vec![
            0x55, 0x0e, 0x84, 0, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0, 0
        ]))
    );
    assert_eq!(
        loaded.getRawOriginal("binary_ulid"),
        Some(&Value::Bytes(vec![
            0x01, 0x56, 0x3e, 0x3a, 0xb5, 0xd3, 0xd6, 0x76, 0x4c, 0x61, 0xef, 0xb9, 0x93, 0x02,
            0xbd, 0x5b
        ]))
    );
    assert_eq!(
        loaded.getRawOriginal("binary_label"),
        Some(&Value::Bytes(b"example".to_vec()))
    );
    assert_eq!(
        loaded.getAttributeValue("binary_label").unwrap(),
        Some("label:example".into())
    );
    assert_eq!(
        loaded.getRawOriginal("embedding"),
        Some(&"[0.5,-1.25,3.75]".into())
    );
    let output = loaded.toArray().unwrap();
    assert_eq!(output["embedding"], json!([0.5, -1.25, 3.75]));
    assert_eq!(output["embedding_blob"], json!([1.0000108480453491]));
    assert_eq!(output["binary_uuid"], json!(UUID));
    assert_eq!(output["binary_ulid"], json!(ULID));
    assert!(loaded.setAttributeValue("binary_uuid", "invalid").is_err());
    assert!(
        loaded
            .setAttributeValue("binary_ulid", "81ARZ3NDEKTSV4RRFFQ69G5FAV")
            .is_err()
    );
    assert!(
        loaded
            .setAttributeValue("embedding", "not an array")
            .is_err()
    );
    assert!(
        loaded
            .setAttributeValue("embedding", Value::Json(json!(["1e999"])))
            .is_err()
    );
    assert!(!loaded.isDirty().unwrap());
    loaded
        .setAttributeValue("binary_ulid", "01563e3a-b5d3-d676-4c61-efb99302bd5b")
        .unwrap();
    loaded
        .setAttributeValue("binary_ulid", "1AaLyDYFxmKZxXbNo18znE")
        .unwrap();
    assert!(!loaded.isDirty().unwrap());
    loaded.setAttributeValue("binary_uuid", " ").unwrap();
    loaded
        .setAttributeValue("embedding", Value::Json(json!([1, "2.25", false])))
        .unwrap();
    loaded.save().await.unwrap();
    let loaded = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(loaded.getRawOriginal("binary_uuid"), Some(&Value::Null));
    assert_eq!(
        loaded.toArray().unwrap()["embedding"],
        json!([1.0, 2.25, 0.0])
    );
    db.statement(
        "UPDATE cast_records SET binary_uuid = ?, embedding = ? WHERE id = ?",
        &[Value::Bytes(vec![0]), "invalid".into(), key.clone()],
    )
    .await
    .unwrap();
    let mut invalid = query().findOrFail(key.clone()).await.unwrap();
    assert!(invalid.getAttributeValue("binary_uuid").is_err());
    assert!(invalid.getAttributeValue("embedding").is_err());
    invalid.setAttributeValue("binary_uuid", UUID).unwrap();
    invalid.setAttributeValue("embedding", Value::Null).unwrap();
    invalid.save().await.unwrap();
    let repaired = query().findOrFail(key).await.unwrap();
    assert_eq!(
        repaired.getAttributeValue("binary_uuid").unwrap(),
        Some(UUID.into())
    );
    assert_eq!(
        repaired.getAttributeValue("embedding").unwrap(),
        Some(Value::Null)
    );
}

async fn verify_json_collection_casts(db: &Connection) {
    let query = || CastRecord::queryOn(db);
    let mut model = query()
        .create(attrs([
            ("json_array", Value::Null),
            ("json_nullable", Value::Null),
            (
                "json_mapped",
                Value::Json(json!([{"name":"first","ignored":true}])),
            ),
            (
                "json_callback",
                Value::Json(json!({"label":"value","skip":false})),
            ),
        ]))
        .await
        .unwrap();
    model
        .setJsonCollection(
            "json_collection",
            Collection::fromEntries([("second", json!(2)), ("first", json!(1))]),
        )
        .unwrap();
    model.save().await.unwrap();
    let key = model.getKey().unwrap().clone();
    let row = db.selectOne(
        "SELECT CASE WHEN json_array IS NULL THEN 1 ELSE 0 END AS array_null, CASE WHEN json_nullable IS NULL THEN 1 ELSE 0 END AS nullable_null FROM cast_records WHERE id = ?",
        std::slice::from_ref(&key),
    ).await.unwrap().unwrap();
    assert_eq!(row.get("array_null"), Some(&0.into()));
    assert_eq!(row.get("nullable_null"), Some(&1.into()));
    let mut loaded = query().findOrFail(key.clone()).await.unwrap();
    assert!(loaded.getJsonCollection("json_array").unwrap().is_none());
    assert!(loaded.getJsonCollection("json_nullable").unwrap().is_none());
    assert_eq!(
        loaded
            .getJsonCollection("json_collection")
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        vec![ArrayKey::from("second"), ArrayKey::from("first")]
    );
    let projected = loaded.toArray().unwrap();
    assert_eq!(projected["json_mapped"], json!([{"name":"first"}]));
    assert_eq!(projected["json_callback"], json!({"label":"VALUE"}));
    assert_eq!(
        loaded
            .getJsonCollectionAs::<CollectionItem>("json_mapped")
            .unwrap()
            .unwrap()
            .get(0),
        Some(&CollectionItem {
            name: "first".into()
        })
    );
    assert!(
        loaded
            .editJsonCollection("json_collection", |values| {
                values.put("third", json!(3));
                Err(DatabaseError::InvalidQuery("test edit failure"))
            })
            .is_err()
    );
    assert!(!loaded.isDirty().unwrap());
    loaded
        .editJsonCollection("json_collection", |values| {
            values.put("third", json!(3));
            Ok(())
        })
        .unwrap();
    loaded
        .fillJsonAttribute("json_array->0->name", "array")
        .unwrap();
    loaded
        .setAttributeValue("json_mapped", Value::Json(json!([{"name":1}])))
        .unwrap();
    loaded.save().await.unwrap();
    let mut invalid = query().findOrFail(key.clone()).await.unwrap();
    assert!(invalid.getJsonCollection("json_mapped").is_err());
    assert_eq!(
        invalid.getAttributeValue("json_array").unwrap(),
        Some(Value::Json(json!([{"name":"array"}])))
    );
    assert_eq!(
        invalid.getRawOriginal("json_collection"),
        Some(&r#"{"second":2,"first":1,"third":3}"#.into())
    );
    invalid
        .setJsonCollection(
            "json_mapped",
            Collection::make([CollectionItem {
                name: "repaired".into(),
            }]),
        )
        .unwrap();
    invalid.save().await.unwrap();
    assert_eq!(
        query().findOrFail(key).await.unwrap().toArray().unwrap()["json_mapped"],
        json!([{"name":"repaired"}])
    );
}

async fn verify_factory_casts(db: &Connection) {
    let query = || CastRecord::queryOn(db);
    let model = query()
        .create(attrs([
            ("factory_value", 9.into()),
            ("factory_inbound", 9.into()),
        ]))
        .await
        .unwrap();
    let key = model.getKey().unwrap().clone();
    let mut plain = query().findOrFail(key.clone()).await.unwrap();
    let mut overridden = query()
        .withCasts([("factory_value", ClassCast::of::<Scaled>(&3))])
        .findOrFail(key.clone())
        .await
        .unwrap();
    assert_eq!(plain.getRawOriginal("factory_value"), Some(&18.into()));
    assert_eq!(
        plain.getAttributeValue("factory_value").unwrap(),
        Some(9.into())
    );
    assert_eq!(
        plain.getAttributeValue("factory_inbound").unwrap(),
        Some(36.into())
    );
    assert_eq!(
        plain.toArray().unwrap()["factory_value"],
        json!({"decoded":9})
    );
    assert_eq!(
        overridden.getAttributeValue("factory_value").unwrap(),
        Some(6.into())
    );
    overridden.mergeCasts([("factory_value", ClassCast::of::<Scaled>(&0))]);
    let snapshot = overridden.state().getAttributes().clone();
    assert!(overridden.setAttributeValue("factory_value", 4).is_err());
    assert_eq!(overridden.state().getAttributes(), &snapshot);
    overridden.mergeCasts([("factory_value", ClassCast::of::<Scaled>(&3))]);
    overridden.setAttributeValue("factory_value", 4).unwrap();
    overridden.save().await.unwrap();
    assert_eq!(
        plain.getAttributeValue("factory_value").unwrap(),
        Some(9.into())
    );
    assert_eq!(
        query()
            .findOrFail(key)
            .await
            .unwrap()
            .getAttributeValue("factory_value")
            .unwrap(),
        Some(6.into())
    );

    plain.mergeCasts([("left_code", ValueObjectCast::of::<FreshCodes>())]);
    plain
        .setValueObject(
            "left_code",
            Some(FreshCodes(Codes {
                left: "x".into(),
                right: "y".into(),
            })),
        )
        .unwrap();
    assert!(matches!(
        plain.getValueObject::<FreshCodes>("left_code").unwrap(),
        Some(std::borrow::Cow::Owned(_))
    ));
    plain
        .editValueObject::<FreshCodes>("left_code", |value| {
            value.0.right = "z".into();
            Ok(())
        })
        .unwrap();
    assert_eq!(plain.getAttribute("right_code"), Some(&"z".into()));
    let mut collection = openrust::database::eloquent::Collection::new(vec![plain, overridden]);
    collection
        .setVisible(&["factory_value"])
        .makeVisible(&["factory_inbound"])
        .makeHidden(&["factory_value"]);
    assert_eq!(
        collection.toArray().unwrap(),
        vec![
            json!({"factory_inbound":36}).as_object().unwrap().clone(),
            json!({"factory_inbound":36}).as_object().unwrap().clone(),
        ]
    );
}

async fn verify_enum_collections_and_objects(db: &Connection) {
    let query = || CastRecord::queryOn(db);
    let mut model = query().make(Attributes::new()).unwrap();
    assert!(
        model
            .getEnumCollection::<Status>("enum_list")
            .unwrap()
            .is_none()
    );
    assert!(
        model
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .is_none()
    );
    model
        .setEnumCollection(
            "enum_list",
            Collection::fromEntries([("second", Status::Published), ("first", Status::Draft)]),
        )
        .unwrap();
    model
        .setEnumCollection(
            "enum_array",
            Collection::fromEntries([("second", Priority::Urgent), ("first", Priority::Normal)]),
        )
        .unwrap();
    model
        .setValueObject(
            "left_code",
            Some(Codes {
                left: "A".into(),
                right: "B".into(),
            }),
        )
        .unwrap();
    model.save().await.unwrap();

    let key = model.getKey().unwrap().clone();
    let mut model = query().findOrFail(key.clone()).await.unwrap();
    let list = model
        .getEnumCollection::<Status>("enum_list")
        .unwrap()
        .unwrap();
    assert_eq!(
        list.into_iter().map(|(key, _)| key).collect::<Vec<_>>(),
        vec!["second".into(), "first".into()]
    );
    assert_eq!(
        model.getRawOriginal("enum_list"),
        Some(&r#"{"second":"Published","first":"Draft"}"#.into())
    );
    assert_eq!(model.getRawOriginal("enum_array"), Some(&"[2,1]".into()));
    let output = model.toArray().unwrap();
    assert_eq!(output["enum_array"], json!([2, 1]));
    assert_eq!(
        output["enum_list"],
        json!({"second":"Published","first":"Draft"})
    );
    assert_eq!(output["left_code"], json!({"left":"A","right":"B"}));
    assert!(model.getEnumCollection::<Priority>("enum_list").is_err());

    let before = std::ptr::from_ref(
        model
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .as_ref(),
    );
    let after = std::ptr::from_ref(
        model
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .as_ref(),
    );
    assert_eq!(before, after);
    assert!(
        model
            .editValueObject::<Codes>("left_code", |value| {
                value.left.clear();
                Ok(())
            })
            .is_err()
    );
    assert!(!model.isDirty().unwrap());
    assert_eq!(
        model
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .left,
        "A"
    );
    model
        .editValueObject::<Codes>("left_code", |value| {
            value.right = "C".into();
            Ok(())
        })
        .unwrap();
    assert!(model.isDirty().unwrap());
    assert_eq!(model.getRawOriginal("right_code"), Some(&"B".into()));
    assert_eq!(model.getAttribute("right_code"), Some(&"C".into()));
    model
        .editEnumCollection::<Status>("enum_list", |cases| {
            cases.put("third", Status::Published);
            Ok(())
        })
        .unwrap();
    model.save().await.unwrap();
    let mut other = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(
        other
            .getEnumCollection::<Status>("enum_list")
            .unwrap()
            .unwrap()
            .get("third"),
        Some(&Status::Published)
    );
    assert_eq!(
        other
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .right,
        "C"
    );
    model.setValueObject::<Codes>("left_code", None).unwrap();
    assert!(model.setAttributeValue("enum_array", true).is_err());
    model.setAttributeValue("enum_list", true).unwrap();
    assert_eq!(model.getAttribute("enum_list"), Some(&"[true]".into()));
    model
        .setAttributeValue(
            "enum_list",
            Value::Json(json!(["Unknown", null, {"raw": true}])),
        )
        .unwrap();
    model.save().await.unwrap();
    assert_eq!(
        other
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .right,
        "C"
    );
    let mut invalid = query().findOrFail(key.clone()).await.unwrap();
    assert!(
        invalid
            .getValueObject::<Codes>("left_code")
            .unwrap()
            .is_none()
    );
    assert!(invalid.getEnumCollection::<Status>("enum_list").is_err());
    invalid
        .setEnumCollection("enum_list", Collection::make([Status::Draft]))
        .unwrap();
    invalid.save().await.unwrap();
    let repaired = query().findOrFail(key).await.unwrap();
    assert_eq!(repaired.toArray().unwrap()["enum_list"], json!(["Draft"]));
}

fn verify_model_projection(mut model: CastRecord) {
    assert_eq!(model.getHidden(), vec!["label", "invalid_blob"]);
    assert!(model.hasAppended("summary"));
    model.makeVisibleIf(false, &["label"]);
    assert!(!model.toArray().unwrap().contains_key("label"));
    let mut evaluations = 0;
    model.makeVisibleIf(
        |record: &CastRecord| {
            evaluations += 1;
            record.getAttribute("label") == Some(&"world".into())
        },
        &["label"],
    );
    assert_eq!(evaluations, 1);
    model.makeHiddenIf(false, &["label"]);
    assert_eq!(model.toArray().unwrap()["label"], json!("WORLD"));
    model.makeHiddenIf(|_: &CastRecord| true, &["label"]);
    model.setAttribute("invalid_blob", Value::Bytes(vec![0xff]));
    let array = model.attributesToArray().unwrap();
    assert!(!array.contains_key("label"));
    assert!(!array.contains_key("invalid_blob"));
    assert_eq!(array["summary"], json!("world"));
    assert_eq!(array["amount"], json!("9007199254740993.14"));
    assert_eq!(array["occurred_at"], json!("2025-06-01"));
    assert_eq!(array["epoch"], json!(-1));
    assert_eq!(array["options"], json!("text"));
    assert_eq!(
        model.getRawOriginal("occurred_at"),
        Some(&"2025-06-01 08:30:00.123456".into())
    );
    model.mergeCasts([
        (
            "occurred_at",
            AttributeCast::DateTimeWithFormat("%H:%M:%S%.6f %:z"),
        ),
        ("day", AttributeCast::DateWithFormat("%Y/%m/%d %H:%M %:z")),
        ("missing", AttributeCast::DateWithFormat("%Y/%m/%d")),
    ]);
    let formatted = model.toArray().unwrap();
    assert_eq!(formatted["occurred_at"], json!("08:30:00.123456 -04:00"));
    assert_eq!(formatted["day"], json!("2025/06/01 00:00 -04:00"));
    assert!(formatted["missing"].is_null());
    model
        .setAttributeValue("occurred_at", "2025-06-01T14:30:00.123456+02:00")
        .unwrap();
    assert!(model.originalIsEquivalent("occurred_at").unwrap());
    assert_eq!(
        model.getAttributeValue("occurred_at").unwrap(),
        Some("2025-06-01T12:30:00.123456Z".into())
    );
    model.makeVisible(&["invalid_blob"]);
    assert!(model.toJson().is_err());
    model.makeHidden(&["invalid_blob"]);
    assert!(model.toJson().is_ok());
    model.mergeCasts([("label", AttributeCast::Custom(&CanonicalCode))]);
    model.setVisible(&["summary"]).makeVisible(&["label"]);
    assert_eq!(
        serde_json::from_str::<Json>(&model.toJson().unwrap()).unwrap(),
        json!({
            "label": "WORLD",
            "summary": "world"
        })
    );
    model.withoutAppends();
    assert_eq!(
        model.toArray().unwrap(),
        json!({"label":"WORLD"}).as_object().unwrap().clone()
    );
    model
        .append(&["summary", "summary"])
        .makeHidden(&["summary"]);
    assert_eq!(model.getAppends(), vec!["summary"]);
    assert!(!model.toArray().unwrap().contains_key("summary"));
}

async fn verify_enum_and_class_casts(db: &Connection) {
    let query = || CastRecord::queryOn(db);
    let mut model = query().make(Attributes::new()).unwrap();
    assert_eq!(model.getEnum::<Status>("status").unwrap(), None);
    assert!(model.setEnum("status", Priority::Urgent).is_err());
    model.setEnum("status", Status::Draft).unwrap();
    assert_eq!(model.getAttribute("status"), Some(&"Draft".into()));
    assert_eq!(
        model.getAttributeValue("status").unwrap(),
        Some("Draft".into())
    );
    assert!(model.setAttributeValue("status", "Undefined").is_err());
    assert_eq!(model.getEnum("status").unwrap(), Some(Status::Draft));
    model
        .fill(attrs([
            ("priority", 2.into()),
            ("pair", " alpha / beta ".into()),
            ("code", " hello ".into()),
            ("inbound_code", " WORLD ".into()),
        ]))
        .unwrap();
    model.save().await.unwrap();
    let key = model.getKey().unwrap().clone();
    let mut loaded = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(loaded.getRawOriginal("priority"), Some(&2.into()));
    assert_eq!(loaded.getEnum("priority").unwrap(), Some(Priority::Urgent));
    assert_eq!(
        loaded.getAttributeValue("pair").unwrap(),
        Some("alpha/beta".into())
    );
    assert_eq!(loaded.getRawOriginal("left_code"), Some(&"alpha".into()));
    assert_eq!(loaded.getRawOriginal("right_code"), Some(&"beta".into()));
    assert_eq!(loaded.getRawOriginal("code"), Some(&"hello".into()));
    let json = loaded.toArray().unwrap();
    assert_eq!(json["code"], json!({"code": "HELLO"}));
    assert_eq!(json["priority"], json!(2));
    assert_eq!(json["status"], json!("Draft"));
    assert_eq!(
        loaded.getAttributeValue("code").unwrap(),
        Some("HELLO".into())
    );
    assert_eq!(
        loaded.getAttributeValue("inbound_code").unwrap(),
        Some("world".into())
    );

    let snapshot = loaded.state().getAttributes().clone();
    assert!(
        loaded
            .fill(attrs([
                ("code", "changed".into()),
                ("pair", "invalid".into())
            ]))
            .is_err()
    );
    assert_eq!(loaded.state().getAttributes(), &snapshot);
    loaded.setAttribute("code", "HELLO");
    assert!(!loaded.isDirty().unwrap());
    loaded.save().await.unwrap();
    loaded.setAttributeValue("pair", "second/third").unwrap();
    loaded.setAttributeValue("status", Value::Null).unwrap();
    loaded.save().await.unwrap();
    let loaded = query().findOrFail(key.clone()).await.unwrap();
    assert_eq!(loaded.getRawOriginal("code"), Some(&"hello".into()));
    assert_eq!(loaded.getEnum::<Status>("status").unwrap(), None);
    assert_eq!(
        loaded.getAttributeValue("pair").unwrap(),
        Some("second/third".into())
    );
    db.statement(
        "UPDATE cast_records SET priority = ? WHERE id = ?",
        &[8.into(), key.clone()],
    )
    .await
    .unwrap();
    let mut loaded = query().findOrFail(key.clone()).await.unwrap();
    assert!(loaded.getEnum::<Priority>("priority").is_err());
    loaded.setEnum("priority", Priority::Normal).unwrap();
    loaded.save().await.unwrap();
    let repaired = query().findOrFail(key).await.unwrap();
    assert_eq!(
        repaired.getEnum("priority").unwrap(),
        Some(Priority::Normal)
    );
}

async fn verify_query_casts(db: &Connection, key: Value) {
    let query = || CastRecord::queryOn(db);
    let mut projected = query()
        .select([
            Column::from("id"),
            Column::from("number"),
            Column::from("label"),
            Expression::new("occurred_at AS selected_at").into(),
            Expression::new("NULL AS missing_at").into(),
        ])
        .withCasts([
            ("number", AttributeCast::Float),
            ("selected_at", AttributeCast::DateTime),
            ("missing_at", AttributeCast::DateTime),
            ("label", AttributeCast::Integer),
        ])
        .withCasts([("number", AttributeCast::String)])
        .findOrFail(key.clone())
        .await
        .unwrap();
    let expected = Some("2025-06-01T12:30:00.123456Z".into());
    assert_eq!(
        projected.getAttributeValue("selected_at").unwrap(),
        expected
    );
    assert_eq!(
        projected.getRawOriginal("selected_at"),
        Some(&"2025-06-01 08:30:00.123456".into())
    );
    assert_eq!(
        projected.getAttributeValue("missing_at").unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        projected.getAttributeValue("label").unwrap(),
        Some("WORLD".into())
    );
    assert_eq!(
        projected.getAttributeValue("number").unwrap(),
        Some("  42.8x".into())
    );
    projected
        .setAttributeValue("selected_at", "2025-06-01T14:30:00.123456+02:00")
        .unwrap();
    assert!(!projected.isDirty().unwrap());
    projected.mergeCasts([(String::from("number"), AttributeCast::Float)]);
    assert_eq!(
        projected.getAttributeValue("number").unwrap(),
        Some(Value::Real(42.8))
    );

    let plain = query().findOrFail(key).await.unwrap();
    assert_eq!(plain.getAttributeValue("number").unwrap(), Some(42.into()));
    assert_eq!(plain.getAttributeValue("selected_at").unwrap(), None);
    let refreshed = openrust::database::eloquent::Collection::new(vec![projected, plain])
        .fresh()
        .await
        .unwrap();
    assert_eq!(refreshed.all().len(), 2);
    for model in refreshed.iter() {
        assert_eq!(
            model.getAttributeValue("number").unwrap(),
            Some(Value::Real(42.8))
        );
        assert_eq!(model.getAttributeValue("selected_at").unwrap(), None);
    }

    let builder = query().withCasts([("number", AttributeCast::DateTime)]);
    assert!(builder.make(attrs([("number", "invalid".into())])).is_err());
    let created = builder
        .create(attrs([("number", "2025-06-01T12:30:00.123456Z".into())]))
        .await
        .unwrap();
    assert_eq!(created.getAttributeValue("number").unwrap(), expected);
    let mut siblings = query()
        .withCasts([("number", AttributeCast::String)])
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap()
        .into_iter();
    let mut first = siblings.next().unwrap();
    let second = siblings.next().unwrap();
    first.mergeCasts([("number", AttributeCast::Integer)]);
    assert_eq!(first.getAttributeValue("number").unwrap(), Some(42.into()));
    assert_eq!(
        second.getAttributeValue("number").unwrap(),
        Some("2025-06-01 08:30:00.123456".into())
    );
}

async fn verify(db: Connection, mysql: bool) {
    db.statement("DROP TABLE IF EXISTS articles", &[])
        .await
        .unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    db.statement(
        &format!("CREATE TABLE articles (article_id {id}, title VARCHAR(80) NOT NULL UNIQUE, account BIGINT NOT NULL, status VARCHAR(20) NOT NULL, protected BIGINT DEFAULT 7, created_at DATETIME, updated_at DATETIME, deleted_at DATETIME)"),
        &[],
    ).await.unwrap();

    let query = || Article::queryOn(&db);
    let mut model = query()
        .create(attrs([
            ("title", "Initial".into()),
            ("account", 1.into()),
            ("status", "published".into()),
            ("protected", 99.into()),
        ]))
        .await
        .unwrap();
    assert!(model.state().exists() && model.state().wasRecentlyCreated());
    assert!(!model.state().isDirty());
    assert_eq!(model.getKey(), Some(&1.into()));
    assert_eq!(model.getConnectionName(), Some("models"));
    assert_eq!(
        model.getAttribute("created_at"),
        model.getAttribute("updated_at")
    );
    assert!(model.getAttribute("created_at").is_some());

    let mut hydrated = query().findOrFail(1).await.unwrap();
    assert!(!hydrated.state().wasRecentlyCreated());
    assert_eq!(hydrated.getAttribute("protected"), Some(&7.into()));
    hydrated
        .setAttribute("article_id", 11)
        .setAttribute("title", "Renamed");
    assert_eq!(hydrated.state().getDirty().len(), 2);
    assert!(hydrated.save().await.unwrap());
    assert_eq!(
        hydrated.state().getChanges().get("title"),
        Some(&"Renamed".into())
    );
    assert!(query().find(1).await.unwrap().is_none());
    assert_eq!(
        query().findOrFail(11).await.unwrap().getKey(),
        Some(&11.into())
    );

    model = query()
        .create(attrs([
            ("title", "Other account".into()),
            ("account", 2.into()),
            ("status", "review".into()),
        ]))
        .await
        .unwrap();
    let other = model.getKey().unwrap().clone();
    let mut draft = query()
        .create(attrs([
            ("title", "Draft".into()),
            ("account", 1.into()),
            ("status", "draft".into()),
        ]))
        .await
        .unwrap();
    let draft_key = draft.getKey().unwrap().clone();

    let mut scoped = query();
    scoped
        .r#where("title", "=", "Renamed")
        .orWhere("title", "=", "Other account");
    scoped.withScope(&Published).unwrap();
    assert_eq!(scoped.get().await.unwrap().len(), 1);
    assert!(matches!(
        query().findOrFail(other.clone()).await,
        Err(DatabaseError::ModelNotFound("articles"))
    ));
    assert!(
        query()
            .withoutGlobalScope("account")
            .find(other.clone())
            .await
            .unwrap()
            .is_some()
    );

    assert!(draft.delete().await.unwrap());
    assert!(draft.state().exists() && draft.trashed());
    assert!(query().find(draft_key.clone()).await.unwrap().is_none());
    assert_eq!(query().onlyTrashed().get().await.unwrap().len(), 1);
    assert!(draft.restore().await.unwrap());
    assert!(!draft.trashed());
    assert!(query().find(draft_key.clone()).await.unwrap().is_some());

    let mut bulk = query();
    bulk.whereKey(draft_key.clone());
    assert_eq!(bulk.delete().await.unwrap(), 1);
    let mut bulk = query().onlyTrashed();
    bulk.whereKey(draft_key.clone());
    assert_eq!(bulk.restore().await.unwrap(), 1);
    let mut bulk = query();
    bulk.r#where("account", "=", 1);
    assert_eq!(
        bulk.update(attrs([("status", "review".into())]))
            .await
            .unwrap(),
        2
    );

    // Failed writes retain the original snapshot so that a corrected retry is safe.
    hydrated.setAttribute("title", "Draft");
    assert!(hydrated.save().await.is_err());
    assert_eq!(
        hydrated.state().getOriginal().get("title"),
        Some(&"Renamed".into())
    );
    hydrated.setAttribute("title", "Retried");
    assert!(hydrated.save().await.unwrap());

    assert!(draft.forceDelete().await.unwrap());
    assert!(!draft.state().exists());
    assert!(
        query()
            .withTrashed()
            .find(draft_key)
            .await
            .unwrap()
            .is_none()
    );
    let mut bulk = query();
    bulk.whereKey(other);
    assert_eq!(bulk.forceDelete().await.unwrap(), 1);
    assert_eq!(query().get().await.unwrap().len(), 1);
    db.statement("DROP TABLE articles", &[]).await.unwrap();

    db.statement("DROP TABLE IF EXISTS labels", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE labels (slug VARCHAR(80) PRIMARY KEY, title VARCHAR(80))",
        &[],
    )
    .await
    .unwrap();
    let mut label = Label::queryOn(&db).make(Attributes::new()).unwrap();
    assert!(matches!(
        label.fill(attrs([("slug", "old".into())])),
        Err(DatabaseError::MassAssignment(key)) if key == "slug"
    ));
    label.forceFill(attrs([
        ("slug", "old".into()),
        ("title", "Original".into()),
    ]));
    assert!(label.save().await.unwrap());
    assert_eq!(label.getKey(), Some(&"old".into()));
    label.setAttribute("slug", "new");
    assert!(label.save().await.unwrap());
    assert!(Label::queryOn(&db).find("old").await.unwrap().is_none());
    assert!(Label::queryOn(&db).find("new").await.unwrap().is_some());
    assert!(label.delete().await.unwrap());
    assert!(!label.state().exists());
    db.statement("DROP TABLE labels", &[]).await.unwrap();
    verify_casts(&db, mysql).await;
}

#[tokio::test]
async fn model_persistence_and_scopes_on_sqlite() {
    verify(
        connect(json!({"driver":"sqlite","database":":memory:"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn model_persistence_and_scopes_on_mysql_8_4() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(connect(json!({"url":url})), true).await;
    }
}

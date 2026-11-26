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

    fn match_results<'a>(
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
        model.to_array().map(Json::Object).map(Some)
    }
}

#[tokio::test]
async fn model_json_stops_at_repeated_models_and_retries_projection_errors() {
    let reference = Arc::new(OnceLock::new());
    let relation = SelfReference(reference.clone());
    let mut model = Article::new_instance(attrs([("title", "Post".into())])).unwrap();
    model.load("selfReference", &relation).await.unwrap();
    assert!(model.to_json().is_err());
    let model = Arc::new(model);
    assert!(reference.set(Arc::downgrade(&model)).is_ok());
    let expected = json!({"title": "Post", "self_reference": {"title": "Post"}});
    for _ in 0..2 {
        assert_eq!(
            serde_json::from_str::<Json>(&model.to_json().unwrap()).unwrap(),
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

    fn cast_using(factor: &i64) -> DatabaseResult<Self> {
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
    fn from_attributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        match (raw.get(name), raw.get("right_code")) {
            (Some(Value::Text(left)), Some(Value::Text(right))) => Ok(Some(Self {
                left: left.clone(),
                right: right.clone(),
            })),
            _ => Ok(None),
        }
    }

    fn to_attributes(
        value: Option<&Self>,
        name: &str,
        _: &Attributes,
    ) -> DatabaseResult<CastWrite> {
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

    fn from_attributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        Codes::from_attributes(name, raw).map(|value| value.map(Self))
    }

    fn to_attributes(
        value: Option<&Self>,
        name: &str,
        raw: &Attributes,
    ) -> DatabaseResult<CastWrite> {
        Codes::to_attributes(value.map(|value| &value.0), name, raw)
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
    fn from_value(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Text(name) if name == "Draft" => Ok(Self::Draft),
            Value::Text(name) if name == "Published" => Ok(Self::Published),
            _ => Err(DatabaseError::InvalidQuery("status case")),
        }
    }

    fn into_value(self) -> Value {
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
    fn from_value(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Integer(1) => Ok(Self::Normal),
            Value::Integer(2) => Ok(Self::Urgent),
            _ => Err(DatabaseError::InvalidQuery("priority backing value")),
        }
    }

    fn into_value(self) -> Value {
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
            .date_format("%Y-%m-%d %H:%M:%S%.6f")
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

    fn serialize_date(&self, date: jiff::Timestamp) -> DatabaseResult<String> {
        Ok(date.strftime("%Y-%m-%d").to_string())
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
            .primary_key("article_id")
            .fillable(&["title", "account", "status"])
            .soft_deletes("deleted_at");
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

    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
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
            .or_where("status", "=", "review");
        Ok(())
    }
}

struct Label(ModelState);

impl Model for Label {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("labels")
            .primary_key("slug")
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
    let query = || CastRecord::query_on(db);
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
        .find_or_fail(model.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        loaded.get_raw_original("occurred_at"),
        Some(&"2025-06-01 08:30:00.123456".into())
    );
    assert_eq!(
        loaded.get_attribute_value("occurred_at").unwrap(),
        Some("2025-06-01T12:30:00.123456Z".into())
    );
    assert_eq!(
        loaded.get_attribute_value("day").unwrap(),
        Some("2025-06-01T04:00:00.000000Z".into())
    );
    assert_eq!(
        loaded.get_attribute_value("epoch").unwrap(),
        Some((-1).into())
    );
    loaded
        .set_attribute_value("occurred_at", "2025-06-01T14:30:00.123456+02:00")
        .unwrap();
    assert!(loaded.original_is_equivalent("occurred_at").unwrap());
    let utc = connect_with_timezone(json!({"driver":"sqlite","database":":memory:"}), "UTC");
    let other = CastRecord::query_on(&utc)
        .make(attrs([(
            "occurred_at",
            "2025-06-01T12:30:00.123456Z".into(),
        )]))
        .unwrap();
    assert_eq!(
        other.get_attribute("occurred_at"),
        Some(&"2025-06-01 12:30:00.123456".into())
    );
    assert_eq!(
        other.get_attribute_value("occurred_at").unwrap(),
        loaded.get_attribute_value("occurred_at").unwrap()
    );
    assert!(
        loaded
            .set_attribute_value("occurred_at", "2025-03-09 02:30:00")
            .is_err()
    );
    assert!(!loaded.is_dirty().unwrap());
    let mut fallback = query().make(Attributes::new()).unwrap();
    fallback
        .set_attribute_value("occurred_at", "2025-11-02 01:30:00")
        .unwrap();
    assert_eq!(
        fallback.get_attribute_value("occurred_at").unwrap(),
        Some("2025-11-02T05:30:00.000000Z".into())
    );
    assert_eq!(loaded.get_attribute("label"), Some(&"hello".into()));
    assert_eq!(
        loaded.get_attribute_value("label").unwrap(),
        Some("HELLO".into())
    );
    assert_eq!(
        loaded.get_attribute_value("summary").unwrap(),
        Some("hello".into())
    );
    assert_eq!(
        loaded.get_attribute_value("number").unwrap(),
        Some(42.into())
    );
    assert_eq!(
        loaded.get_attribute_value("active").unwrap(),
        Some(true.into())
    );
    assert_eq!(
        loaded.get_attribute_value("measure").unwrap(),
        Some(Value::Real(125.0))
    );
    assert_eq!(
        loaded.get_attribute_value("missing").unwrap(),
        Some(Value::Null)
    );
    assert_eq!(loaded.get_attribute_value("absent").unwrap(), None);
    assert_eq!(
        loaded.get_attribute_value("options").unwrap(),
        Some(Value::Json(json!({"mode":"demo"})))
    );
    assert!(!loaded.state().is_dirty());

    assert_eq!(
        loaded.get_attribute_value("amount").unwrap(),
        Some("9007199254740993.13".into())
    );
    let raw_amount = loaded.get_raw_original("amount").cloned().unwrap();
    loaded
        .set_attribute_value("amount", "9007199254740993.1251")
        .unwrap();
    loaded.set_attribute_value("number", 42).unwrap();
    loaded.set_attribute_value("measure", "125").unwrap();
    loaded
        .set_attribute_value("options", Value::Json(json!({"mode":"demo"})))
        .unwrap();
    assert!(loaded.state().is_dirty());
    assert!(!loaded.is_dirty().unwrap());
    loaded.save().await.unwrap();
    let unchanged = query()
        .find_or_fail(model.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(unchanged.get_raw_original("amount"), Some(&raw_amount));

    let mut example = query().make(Attributes::new()).unwrap();
    example.set_attribute_value("amount", "-1.235").unwrap();
    assert_eq!(
        example.get_attribute_value("amount").unwrap(),
        Some("-1.24".into())
    );
    example.set_attribute_value("amount", "1/8").unwrap();
    assert_eq!(
        example.get_attribute_value("amount").unwrap(),
        Some("0.13".into())
    );
    example.set_attribute_value("amount", "1e100000").unwrap();
    assert!(example.get_attribute_value("amount").is_err());

    assert!(
        loaded
            .fill(attrs([("active", "0".into()), ("label", 1.into())]))
            .is_err()
    );
    assert!(loaded.state().is_dirty());
    let reference: Json =
        serde_json::from_str(include_str!("reference/eloquent-strictness.json")).unwrap();
    assert_eq!(
        loaded.get_attribute("active"),
        Some(
            &reference["fill_cast_setter_failure"]["active"]
                .as_str()
                .unwrap()
                .into()
        )
    );
    loaded.set_attribute_value("label", " World ").unwrap();
    loaded.set_attribute_value("options", "text").unwrap();
    loaded
        .set_attribute_value("amount", "9007199254740993.135")
        .unwrap();
    assert_eq!(loaded.get_raw_original("label"), Some(&"hello".into()));
    loaded.save().await.unwrap();
    let updated = query()
        .find_or_fail(model.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        updated.get_attribute_value("amount").unwrap(),
        Some("9007199254740993.14".into())
    );
    assert_eq!(
        updated.get_attribute_value("label").unwrap(),
        Some("WORLD".into())
    );
    assert_eq!(
        updated.get_attribute_value("options").unwrap(),
        Some(Value::Json(json!("text")))
    );
    verify_model_projection(updated);
    verify_query_casts(db, model.get_key().unwrap().clone()).await;
    verify_enum_and_class_casts(db).await;
    verify_enum_collections_and_objects(db).await;
    verify_factory_casts(db).await;
    verify_json_collection_casts(db).await;
    verify_binary_and_vector_casts(db).await;
    verify_collection_projection(db, model.get_key().unwrap()).await;
    db.statement("DROP TABLE cast_records", &[]).await.unwrap();
}

async fn verify_collection_projection(db: &Connection, key: &Value) {
    let second = CastRecord::query_on(db)
        .create(attrs([("label", "Other".into())]))
        .await
        .unwrap();
    let query = || {
        let mut query = CastRecord::query_on(db);
        query.where_in("id", vec![key.clone(), second.get_key().unwrap().clone()]);
        query.order_by("id", "asc")
    };
    let mut records = query().get().await.unwrap();
    let untouched = query().get().await.unwrap();
    assert_eq!(records.len(), 2);

    records
        .set_visible(&["id", "label"])
        .merge_visible(&["summary", "number"])
        .merge_hidden(&["number"]);
    for record in records.all() {
        assert_eq!(record.get_hidden(), ["label", "invalid_blob", "number"]);
        assert_eq!(record.get_visible(), ["id", "label", "summary", "number"]);
    }
    let original = serde_json::from_str::<Json>(&records.to_json().unwrap()).unwrap();
    for record in original.as_array().unwrap() {
        assert!(record.get("summary").is_some());
        assert!(record.get("label").is_none());
        assert!(record.get("number").is_none());
    }

    records.set_appends(&["label"]);
    assert!(
        records
            .iter()
            .all(|record| record.get_appends() == ["label"])
    );
    assert!(
        records
            .to_array()
            .unwrap()
            .iter()
            .all(|row| !row.contains_key("summary"))
    );
    records.set_appends(&["summary"]).without_appends();
    assert!(records.iter().all(|record| record.get_appends().is_empty()));
    assert!(
        records
            .to_array()
            .unwrap()
            .iter()
            .all(|row| !row.contains_key("summary"))
    );
    assert!(
        untouched
            .to_array()
            .unwrap()
            .iter()
            .all(|row| row.contains_key("summary"))
    );
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
        CastRecord::query_on(db).with_casts([
            ("binary_uuid", AsBinary::uuid()),
            ("binary_ulid", AsBinary::ulid()),
            ("binary_label", AsBinary::of(BinaryFormat::Custom(label))),
            ("embedding", AsVector::cast()),
            ("embedding_blob", AsVector::cast()),
        ])
    };
    let mut model = query().make(Attributes::new()).unwrap();
    model
        .set_attribute_value("binary_uuid", UUID.to_ascii_uppercase())
        .unwrap();
    model
        .set_attribute_value("binary_ulid", ULID.to_ascii_lowercase())
        .unwrap();
    model
        .set_attribute_value("binary_label", "label:example")
        .unwrap();
    model
        .set_attribute_value("embedding", Value::Json(json!([0.5, -1.25, 3.75])))
        .unwrap();
    assert_eq!(
        model.get_attribute_value("binary_uuid").unwrap(),
        Some(UUID.into())
    );
    assert_eq!(
        model.get_attribute_value("binary_ulid").unwrap(),
        Some(ULID.into())
    );
    model.set_attribute("embedding_blob", Value::Bytes(vec![0x5b, 0, 0x80, 0x3f]));
    model.save().await.unwrap();
    let key = model.get_key().unwrap().clone();
    let mut loaded = query().find_or_fail(key.clone()).await.unwrap();
    assert_eq!(
        loaded.get_raw_original("binary_uuid"),
        Some(&Value::Bytes(vec![
            0x55, 0x0e, 0x84, 0, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0, 0
        ]))
    );
    assert_eq!(
        loaded.get_raw_original("binary_ulid"),
        Some(&Value::Bytes(vec![
            0x01, 0x56, 0x3e, 0x3a, 0xb5, 0xd3, 0xd6, 0x76, 0x4c, 0x61, 0xef, 0xb9, 0x93, 0x02,
            0xbd, 0x5b
        ]))
    );
    assert_eq!(
        loaded.get_raw_original("binary_label"),
        Some(&Value::Bytes(b"example".to_vec()))
    );
    assert_eq!(
        loaded.get_attribute_value("binary_label").unwrap(),
        Some("label:example".into())
    );
    assert_eq!(
        loaded.get_raw_original("embedding"),
        Some(&"[0.5,-1.25,3.75]".into())
    );
    let output = loaded.to_array().unwrap();
    assert_eq!(output["embedding"], json!([0.5, -1.25, 3.75]));
    assert_eq!(output["embedding_blob"], json!([1.0000108480453491]));
    assert_eq!(output["binary_uuid"], json!(UUID));
    assert_eq!(output["binary_ulid"], json!(ULID));
    assert!(
        loaded
            .set_attribute_value("binary_uuid", "invalid")
            .is_err()
    );
    assert!(
        loaded
            .set_attribute_value("binary_ulid", "81ARZ3NDEKTSV4RRFFQ69G5FAV")
            .is_err()
    );
    assert!(
        loaded
            .set_attribute_value("embedding", "not an array")
            .is_err()
    );
    assert!(
        loaded
            .set_attribute_value("embedding", Value::Json(json!(["1e999"])))
            .is_err()
    );
    assert!(!loaded.is_dirty().unwrap());
    loaded
        .set_attribute_value("binary_ulid", "01563e3a-b5d3-d676-4c61-efb99302bd5b")
        .unwrap();
    loaded
        .set_attribute_value("binary_ulid", "1AaLyDYFxmKZxXbNo18znE")
        .unwrap();
    assert!(!loaded.is_dirty().unwrap());
    loaded.set_attribute_value("binary_uuid", " ").unwrap();
    loaded
        .set_attribute_value("embedding", Value::Json(json!([1, "2.25", false])))
        .unwrap();
    loaded.save().await.unwrap();
    let loaded = query().find_or_fail(key.clone()).await.unwrap();
    assert_eq!(loaded.get_raw_original("binary_uuid"), Some(&Value::Null));
    assert_eq!(
        loaded.to_array().unwrap()["embedding"],
        json!([1.0, 2.25, 0.0])
    );
    db.statement(
        "UPDATE cast_records SET binary_uuid = ?, embedding = ? WHERE id = ?",
        &[Value::Bytes(vec![0]), "invalid".into(), key.clone()],
    )
    .await
    .unwrap();
    let mut invalid = query().find_or_fail(key.clone()).await.unwrap();
    assert!(invalid.get_attribute_value("binary_uuid").is_err());
    assert!(invalid.get_attribute_value("embedding").is_err());
    invalid.set_attribute_value("binary_uuid", UUID).unwrap();
    invalid
        .set_attribute_value("embedding", Value::Null)
        .unwrap();
    invalid.save().await.unwrap();
    let repaired = query().find_or_fail(key).await.unwrap();
    assert_eq!(
        repaired.get_attribute_value("binary_uuid").unwrap(),
        Some(UUID.into())
    );
    assert_eq!(
        repaired.get_attribute_value("embedding").unwrap(),
        Some(Value::Null)
    );
}

async fn verify_json_collection_casts(db: &Connection) {
    let query = || CastRecord::query_on(db);
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
        .set_json_collection(
            "json_collection",
            Collection::from_entries([("second", json!(2)), ("first", json!(1))]),
        )
        .unwrap();
    model.save().await.unwrap();
    let key = model.get_key().unwrap().clone();
    let row = db.select_one(
        "SELECT CASE WHEN json_array IS NULL THEN 1 ELSE 0 END AS array_null, CASE WHEN json_nullable IS NULL THEN 1 ELSE 0 END AS nullable_null FROM cast_records WHERE id = ?",
        std::slice::from_ref(&key),
    ).await.unwrap().unwrap();
    assert_eq!(row.get("array_null"), Some(&0.into()));
    assert_eq!(row.get("nullable_null"), Some(&1.into()));
    let mut loaded = query().find_or_fail(key.clone()).await.unwrap();
    assert!(loaded.get_json_collection("json_array").unwrap().is_none());
    assert!(
        loaded
            .get_json_collection("json_nullable")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        loaded
            .get_json_collection("json_collection")
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        vec![ArrayKey::from("second"), ArrayKey::from("first")]
    );
    let projected = loaded.to_array().unwrap();
    assert_eq!(projected["json_mapped"], json!([{"name":"first"}]));
    assert_eq!(projected["json_callback"], json!({"label":"VALUE"}));
    assert_eq!(
        loaded
            .get_json_collection_as::<CollectionItem>("json_mapped")
            .unwrap()
            .unwrap()
            .get(0),
        Some(&CollectionItem {
            name: "first".into()
        })
    );
    assert!(
        loaded
            .edit_json_collection("json_collection", |values| {
                values.put("third", json!(3));
                Err(DatabaseError::InvalidQuery("test edit failure"))
            })
            .is_err()
    );
    assert!(!loaded.is_dirty().unwrap());
    loaded
        .edit_json_collection("json_collection", |values| {
            values.put("third", json!(3));
            Ok(())
        })
        .unwrap();
    loaded
        .fill_json_attribute("json_array->0->name", "array")
        .unwrap();
    loaded
        .set_attribute_value("json_mapped", Value::Json(json!([{"name":1}])))
        .unwrap();
    loaded.save().await.unwrap();
    let mut invalid = query().find_or_fail(key.clone()).await.unwrap();
    assert!(invalid.get_json_collection("json_mapped").is_err());
    assert_eq!(
        invalid.get_attribute_value("json_array").unwrap(),
        Some(Value::Json(json!([{"name":"array"}])))
    );
    assert_eq!(
        invalid.get_raw_original("json_collection"),
        Some(&r#"{"second":2,"first":1,"third":3}"#.into())
    );
    invalid
        .set_json_collection(
            "json_mapped",
            Collection::make([CollectionItem {
                name: "repaired".into(),
            }]),
        )
        .unwrap();
    invalid.save().await.unwrap();
    assert_eq!(
        query().find_or_fail(key).await.unwrap().to_array().unwrap()["json_mapped"],
        json!([{"name":"repaired"}])
    );
}

async fn verify_factory_casts(db: &Connection) {
    let query = || CastRecord::query_on(db);
    let model = query()
        .create(attrs([
            ("factory_value", 9.into()),
            ("factory_inbound", 9.into()),
        ]))
        .await
        .unwrap();
    let key = model.get_key().unwrap().clone();
    let mut plain = query().find_or_fail(key.clone()).await.unwrap();
    let mut overridden = query()
        .with_casts([("factory_value", ClassCast::of::<Scaled>(&3))])
        .find_or_fail(key.clone())
        .await
        .unwrap();
    assert_eq!(plain.get_raw_original("factory_value"), Some(&18.into()));
    assert_eq!(
        plain.get_attribute_value("factory_value").unwrap(),
        Some(9.into())
    );
    assert_eq!(
        plain.get_attribute_value("factory_inbound").unwrap(),
        Some(36.into())
    );
    assert_eq!(
        plain.to_array().unwrap()["factory_value"],
        json!({"decoded":9})
    );
    assert_eq!(
        overridden.get_attribute_value("factory_value").unwrap(),
        Some(6.into())
    );
    overridden.merge_casts([("factory_value", ClassCast::of::<Scaled>(&0))]);
    let snapshot = overridden.state().get_attributes().clone();
    assert!(overridden.set_attribute_value("factory_value", 4).is_err());
    assert_eq!(overridden.state().get_attributes(), &snapshot);
    overridden.merge_casts([("factory_value", ClassCast::of::<Scaled>(&3))]);
    overridden.set_attribute_value("factory_value", 4).unwrap();
    overridden.save().await.unwrap();
    assert_eq!(
        plain.get_attribute_value("factory_value").unwrap(),
        Some(9.into())
    );
    assert_eq!(
        query()
            .find_or_fail(key)
            .await
            .unwrap()
            .get_attribute_value("factory_value")
            .unwrap(),
        Some(6.into())
    );

    plain.merge_casts([("left_code", ValueObjectCast::of::<FreshCodes>())]);
    plain
        .set_value_object(
            "left_code",
            Some(FreshCodes(Codes {
                left: "x".into(),
                right: "y".into(),
            })),
        )
        .unwrap();
    assert!(matches!(
        plain.get_value_object::<FreshCodes>("left_code").unwrap(),
        Some(std::borrow::Cow::Owned(_))
    ));
    plain
        .edit_value_object::<FreshCodes>("left_code", |value| {
            value.0.right = "z".into();
            Ok(())
        })
        .unwrap();
    assert_eq!(plain.get_attribute("right_code"), Some(&"z".into()));
    let mut collection = openrust::database::eloquent::Collection::new(vec![plain, overridden]);
    collection
        .set_visible(&["factory_value"])
        .make_visible(&["factory_inbound"])
        .make_hidden(&["factory_value"]);
    assert_eq!(
        collection.to_array().unwrap(),
        vec![
            json!({"factory_inbound":36}).as_object().unwrap().clone(),
            json!({"factory_inbound":36}).as_object().unwrap().clone(),
        ]
    );
}

async fn verify_enum_collections_and_objects(db: &Connection) {
    let query = || CastRecord::query_on(db);
    let mut model = query().make(Attributes::new()).unwrap();
    assert!(
        model
            .get_enum_collection::<Status>("enum_list")
            .unwrap()
            .is_none()
    );
    assert!(
        model
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .is_none()
    );
    model
        .set_enum_collection(
            "enum_list",
            Collection::from_entries([("second", Status::Published), ("first", Status::Draft)]),
        )
        .unwrap();
    model
        .set_enum_collection(
            "enum_array",
            Collection::from_entries([("second", Priority::Urgent), ("first", Priority::Normal)]),
        )
        .unwrap();
    model
        .set_value_object(
            "left_code",
            Some(Codes {
                left: "A".into(),
                right: "B".into(),
            }),
        )
        .unwrap();
    model.save().await.unwrap();

    let key = model.get_key().unwrap().clone();
    let mut model = query().find_or_fail(key.clone()).await.unwrap();
    let list = model
        .get_enum_collection::<Status>("enum_list")
        .unwrap()
        .unwrap();
    assert_eq!(
        list.into_iter().map(|(key, _)| key).collect::<Vec<_>>(),
        vec!["second".into(), "first".into()]
    );
    assert_eq!(
        model.get_raw_original("enum_list"),
        Some(&r#"{"second":"Published","first":"Draft"}"#.into())
    );
    assert_eq!(model.get_raw_original("enum_array"), Some(&"[2,1]".into()));
    let output = model.to_array().unwrap();
    assert_eq!(output["enum_array"], json!([2, 1]));
    assert_eq!(
        output["enum_list"],
        json!({"second":"Published","first":"Draft"})
    );
    assert_eq!(output["left_code"], json!({"left":"A","right":"B"}));
    assert!(model.get_enum_collection::<Priority>("enum_list").is_err());

    let before = std::ptr::from_ref(
        model
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .as_ref(),
    );
    let after = std::ptr::from_ref(
        model
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .as_ref(),
    );
    assert_eq!(before, after);
    assert!(
        model
            .edit_value_object::<Codes>("left_code", |value| {
                value.left.clear();
                Ok(())
            })
            .is_err()
    );
    assert!(!model.is_dirty().unwrap());
    assert_eq!(
        model
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .left,
        "A"
    );
    model
        .edit_value_object::<Codes>("left_code", |value| {
            value.right = "C".into();
            Ok(())
        })
        .unwrap();
    assert!(model.is_dirty().unwrap());
    assert_eq!(model.get_raw_original("right_code"), Some(&"B".into()));
    assert_eq!(model.get_attribute("right_code"), Some(&"C".into()));
    model
        .edit_enum_collection::<Status>("enum_list", |cases| {
            cases.put("third", Status::Published);
            Ok(())
        })
        .unwrap();
    model.save().await.unwrap();
    let mut other = query().find_or_fail(key.clone()).await.unwrap();
    assert_eq!(
        other
            .get_enum_collection::<Status>("enum_list")
            .unwrap()
            .unwrap()
            .get("third"),
        Some(&Status::Published)
    );
    assert_eq!(
        other
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .right,
        "C"
    );
    model.set_value_object::<Codes>("left_code", None).unwrap();
    assert!(model.set_attribute_value("enum_array", true).is_err());
    model.set_attribute_value("enum_list", true).unwrap();
    assert_eq!(model.get_attribute("enum_list"), Some(&"[true]".into()));
    model
        .set_attribute_value(
            "enum_list",
            Value::Json(json!(["Unknown", null, {"raw": true}])),
        )
        .unwrap();
    model.save().await.unwrap();
    assert_eq!(
        other
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .unwrap()
            .right,
        "C"
    );
    let mut invalid = query().find_or_fail(key.clone()).await.unwrap();
    assert!(
        invalid
            .get_value_object::<Codes>("left_code")
            .unwrap()
            .is_none()
    );
    assert!(invalid.get_enum_collection::<Status>("enum_list").is_err());
    invalid
        .set_enum_collection("enum_list", Collection::make([Status::Draft]))
        .unwrap();
    invalid.save().await.unwrap();
    let repaired = query().find_or_fail(key).await.unwrap();
    assert_eq!(repaired.to_array().unwrap()["enum_list"], json!(["Draft"]));
}

fn verify_model_projection(mut model: CastRecord) {
    assert_eq!(model.get_hidden(), vec!["label", "invalid_blob"]);
    assert!(model.has_appended("summary"));
    model.make_visible_if(false, &["label"]);
    assert!(!model.to_array().unwrap().contains_key("label"));
    let mut evaluations = 0;
    model.make_visible_if(
        |record: &CastRecord| {
            evaluations += 1;
            record.get_attribute("label") == Some(&"world".into())
        },
        &["label"],
    );
    assert_eq!(evaluations, 1);
    model.make_hidden_if(false, &["label"]);
    assert_eq!(model.to_array().unwrap()["label"], json!("WORLD"));
    model.make_hidden_if(|_: &CastRecord| true, &["label"]);
    model.set_attribute("invalid_blob", Value::Bytes(vec![0xff]));
    let array = model.attributes_to_array().unwrap();
    assert!(!array.contains_key("label"));
    assert!(!array.contains_key("invalid_blob"));
    assert_eq!(array["summary"], json!("world"));
    assert_eq!(array["amount"], json!("9007199254740993.14"));
    assert_eq!(array["occurred_at"], json!("2025-06-01"));
    assert_eq!(array["epoch"], json!(-1));
    assert_eq!(array["options"], json!("text"));
    assert_eq!(
        model.get_raw_original("occurred_at"),
        Some(&"2025-06-01 08:30:00.123456".into())
    );
    model.merge_casts([
        (
            "occurred_at",
            AttributeCast::DateTimeWithFormat("%H:%M:%S%.6f %:z"),
        ),
        ("day", AttributeCast::DateWithFormat("%Y/%m/%d %H:%M %:z")),
        ("missing", AttributeCast::DateWithFormat("%Y/%m/%d")),
    ]);
    let formatted = model.to_array().unwrap();
    assert_eq!(formatted["occurred_at"], json!("08:30:00.123456 -04:00"));
    assert_eq!(formatted["day"], json!("2025/06/01 00:00 -04:00"));
    assert!(formatted["missing"].is_null());
    model
        .set_attribute_value("occurred_at", "2025-06-01T14:30:00.123456+02:00")
        .unwrap();
    assert!(model.original_is_equivalent("occurred_at").unwrap());
    assert_eq!(
        model.get_attribute_value("occurred_at").unwrap(),
        Some("2025-06-01T12:30:00.123456Z".into())
    );
    model.make_visible(&["invalid_blob"]);
    assert!(model.to_json().is_err());
    model.make_hidden(&["invalid_blob"]);
    assert!(model.to_json().is_ok());
    model.merge_casts([("label", AttributeCast::Custom(&CanonicalCode))]);
    model.set_visible(&["summary"]).make_visible(&["label"]);
    assert_eq!(
        serde_json::from_str::<Json>(&model.to_json().unwrap()).unwrap(),
        json!({
            "label": "WORLD",
            "summary": "world"
        })
    );
    model.without_appends();
    assert_eq!(
        model.to_array().unwrap(),
        json!({"label":"WORLD"}).as_object().unwrap().clone()
    );
    model
        .append(&["summary", "summary"])
        .make_hidden(&["summary"]);
    assert_eq!(model.get_appends(), vec!["summary"]);
    assert!(!model.to_array().unwrap().contains_key("summary"));
}

async fn verify_enum_and_class_casts(db: &Connection) {
    let query = || CastRecord::query_on(db);
    let mut model = query().make(Attributes::new()).unwrap();
    assert_eq!(model.get_enum::<Status>("status").unwrap(), None);
    assert!(model.set_enum("status", Priority::Urgent).is_err());
    model.set_enum("status", Status::Draft).unwrap();
    assert_eq!(model.get_attribute("status"), Some(&"Draft".into()));
    assert_eq!(
        model.get_attribute_value("status").unwrap(),
        Some("Draft".into())
    );
    assert!(model.set_attribute_value("status", "Undefined").is_err());
    assert_eq!(model.get_enum("status").unwrap(), Some(Status::Draft));
    model
        .fill(attrs([
            ("priority", 2.into()),
            ("pair", " alpha / beta ".into()),
            ("code", " hello ".into()),
            ("inbound_code", " WORLD ".into()),
        ]))
        .unwrap();
    model.save().await.unwrap();
    let key = model.get_key().unwrap().clone();
    let mut loaded = query().find_or_fail(key.clone()).await.unwrap();
    assert_eq!(loaded.get_raw_original("priority"), Some(&2.into()));
    assert_eq!(loaded.get_enum("priority").unwrap(), Some(Priority::Urgent));
    assert_eq!(
        loaded.get_attribute_value("pair").unwrap(),
        Some("alpha/beta".into())
    );
    assert_eq!(loaded.get_raw_original("left_code"), Some(&"alpha".into()));
    assert_eq!(loaded.get_raw_original("right_code"), Some(&"beta".into()));
    assert_eq!(loaded.get_raw_original("code"), Some(&"hello".into()));
    let json = loaded.to_array().unwrap();
    assert_eq!(json["code"], json!({"code": "HELLO"}));
    assert_eq!(json["priority"], json!(2));
    assert_eq!(json["status"], json!("Draft"));
    assert_eq!(
        loaded.get_attribute_value("code").unwrap(),
        Some("HELLO".into())
    );
    assert_eq!(
        loaded.get_attribute_value("inbound_code").unwrap(),
        Some("world".into())
    );

    let mut snapshot = loaded.state().get_attributes().clone();
    snapshot.insert("code".into(), "changed".into());
    assert!(
        loaded
            .fill(attrs([
                ("code", "changed".into()),
                ("pair", "invalid".into())
            ]))
            .is_err()
    );
    assert_eq!(loaded.state().get_attributes(), &snapshot);
    loaded.set_attribute("code", "HELLO");
    assert!(!loaded.is_dirty().unwrap());
    loaded.save().await.unwrap();
    loaded.set_attribute_value("pair", "second/third").unwrap();
    loaded.set_attribute_value("status", Value::Null).unwrap();
    loaded.save().await.unwrap();
    let loaded = query().find_or_fail(key.clone()).await.unwrap();
    assert_eq!(loaded.get_raw_original("code"), Some(&"hello".into()));
    assert_eq!(loaded.get_enum::<Status>("status").unwrap(), None);
    assert_eq!(
        loaded.get_attribute_value("pair").unwrap(),
        Some("second/third".into())
    );
    db.statement(
        "UPDATE cast_records SET priority = ? WHERE id = ?",
        &[8.into(), key.clone()],
    )
    .await
    .unwrap();
    let mut loaded = query().find_or_fail(key.clone()).await.unwrap();
    assert!(loaded.get_enum::<Priority>("priority").is_err());
    loaded.set_enum("priority", Priority::Normal).unwrap();
    loaded.save().await.unwrap();
    let repaired = query().find_or_fail(key).await.unwrap();
    assert_eq!(
        repaired.get_enum("priority").unwrap(),
        Some(Priority::Normal)
    );
}

async fn verify_query_casts(db: &Connection, key: Value) {
    let query = || CastRecord::query_on(db);
    let mut projected = query()
        .select([
            Column::from("id"),
            Column::from("number"),
            Column::from("label"),
            Expression::new("occurred_at AS selected_at").into(),
            Expression::new("NULL AS missing_at").into(),
        ])
        .with_casts([
            ("number", AttributeCast::Float),
            ("selected_at", AttributeCast::DateTime),
            ("missing_at", AttributeCast::DateTime),
            ("label", AttributeCast::Integer),
        ])
        .with_casts([("number", AttributeCast::String)])
        .find_or_fail(key.clone())
        .await
        .unwrap();
    let expected = Some("2025-06-01T12:30:00.123456Z".into());
    assert_eq!(
        projected.get_attribute_value("selected_at").unwrap(),
        expected
    );
    assert_eq!(
        projected.get_raw_original("selected_at"),
        Some(&"2025-06-01 08:30:00.123456".into())
    );
    assert_eq!(
        projected.get_attribute_value("missing_at").unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        projected.get_attribute_value("label").unwrap(),
        Some("WORLD".into())
    );
    assert_eq!(
        projected.get_attribute_value("number").unwrap(),
        Some("  42.8x".into())
    );
    projected
        .set_attribute_value("selected_at", "2025-06-01T14:30:00.123456+02:00")
        .unwrap();
    assert!(!projected.is_dirty().unwrap());
    projected.merge_casts([(String::from("number"), AttributeCast::Float)]);
    assert_eq!(
        projected.get_attribute_value("number").unwrap(),
        Some(Value::Real(42.8))
    );

    let plain = query().find_or_fail(key).await.unwrap();
    assert_eq!(
        plain.get_attribute_value("number").unwrap(),
        Some(42.into())
    );
    assert_eq!(plain.get_attribute_value("selected_at").unwrap(), None);
    let refreshed = openrust::database::eloquent::Collection::new(vec![projected, plain])
        .fresh()
        .await
        .unwrap();
    assert_eq!(refreshed.all().len(), 2);
    for model in refreshed.iter() {
        assert_eq!(
            model.get_attribute_value("number").unwrap(),
            Some(Value::Real(42.8))
        );
        assert_eq!(model.get_attribute_value("selected_at").unwrap(), None);
    }

    let builder = query().with_casts([("number", AttributeCast::DateTime)]);
    assert!(builder.make(attrs([("number", "invalid".into())])).is_err());
    let created = builder
        .create(attrs([("number", "2025-06-01T12:30:00.123456Z".into())]))
        .await
        .unwrap();
    assert_eq!(created.get_attribute_value("number").unwrap(), expected);
    let mut siblings = query()
        .with_casts([("number", AttributeCast::String)])
        .order_by("id", "asc")
        .get()
        .await
        .unwrap()
        .into_iter();
    let mut first = siblings.next().unwrap();
    let second = siblings.next().unwrap();
    first.merge_casts([("number", AttributeCast::Integer)]);
    assert_eq!(
        first.get_attribute_value("number").unwrap(),
        Some(42.into())
    );
    assert_eq!(
        second.get_attribute_value("number").unwrap(),
        Some("2025-06-01 08:30:00.123456".into())
    );
}

async fn verify_locked_reload(db: &Connection, model: &mut Article, mysql: bool) {
    let mut wrong = connect(json!({"driver":"sqlite","database":":memory:"}))
        .begin_transaction()
        .await
        .unwrap();
    assert!(matches!(
        model.refresh_for_update(&mut wrong).await,
        Err(DatabaseError::InvalidQuery("model transaction connection"))
    ));
    wrong.roll_back(None).await.unwrap();

    let key = model.get_key().unwrap().clone();
    for commit in [true, false] {
        let mut owner = db.begin_transaction().await.unwrap();
        model.refresh_for_update(&mut owner).await.unwrap();
        assert_eq!(model.get_key(), Some(&key));
        assert_eq!(owner.transaction_level(), 1);

        if mysql {
            let mut contender = db.begin_transaction().await.unwrap();
            let sql = "SELECT article_id FROM articles WHERE article_id = ? FOR UPDATE NOWAIT";
            let bindings = std::slice::from_ref(&key);
            let blocked = contender.select_one(sql, bindings).await.unwrap_err();
            let DatabaseError::Driver { source, .. } = blocked else {
                panic!("expected MySQL row lock error");
            };
            assert_eq!(
                source
                    .as_database_error()
                    .and_then(|error| error.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
                    .map(sqlx::mysql::MySqlDatabaseError::number),
                Some(3572)
            );
            if commit {
                owner.commit().await.unwrap();
            } else {
                owner.roll_back(None).await.unwrap();
            }
            assert!(contender.select_one(sql, bindings).await.unwrap().is_some());
            contender.roll_back(None).await.unwrap();
        } else {
            owner.roll_back(None).await.unwrap();
        }
        assert!(matches!(
            model.refresh_for_update(&mut owner).await,
            Err(DatabaseError::TransactionClosed)
        ));
    }
}

async fn verify_model_reload(db: &Connection, model: &mut Article) {
    let key = model.get_key().unwrap().clone();
    db.table("articles")
        .r#where("article_id", "=", key.clone())
        .update(attrs([
            ("title", "Read back".into()),
            ("account", 99.into()),
            ("deleted_at", "2026-01-01 00:00:00".into()),
        ]))
        .await
        .unwrap();
    assert!(
        Article::query_on(db)
            .find(key.clone())
            .await
            .unwrap()
            .is_none()
    );
    model.set_attribute("article_id", 9999);
    model.set_attribute("title", "Local edit");
    let fresh = model.fresh().await.unwrap().unwrap();
    assert_eq!(fresh.get_key(), Some(&key));
    assert_eq!(fresh.get_attribute("title"), Some(&"Read back".into()));
    assert!(fresh.trashed());
    assert_eq!(model.get_attribute("title"), Some(&"Local edit".into()));
    assert_eq!(model.refresh().await.unwrap().get_key(), Some(&key));
    assert!(!model.state().is_dirty());
    assert_eq!(model.get_raw_original("title"), Some(&"Read back".into()));

    db.table("articles")
        .r#where("article_id", "=", key)
        .delete()
        .await
        .unwrap();
    assert!(model.fresh().await.unwrap().is_none());
    assert!(matches!(
        model.refresh().await,
        Err(DatabaseError::ModelNotFound("articles"))
    ));
    assert_eq!(model.get_attribute("title"), Some(&"Read back".into()));
    let mut unsaved = Article::new_instance(Attributes::new()).unwrap();
    assert!(unsaved.fresh().await.unwrap().is_none());
    assert!(!unsaved.refresh().await.unwrap().state().exists());
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

    let query = || Article::query_on(&db);
    let mut model = query()
        .create(attrs([
            ("title", "Initial".into()),
            ("account", 1.into()),
            ("status", "published".into()),
            ("protected", 99.into()),
        ]))
        .await
        .unwrap();
    assert!(model.state().exists() && model.state().was_recently_created());
    assert!(!model.state().is_dirty());
    assert_eq!(model.get_key(), Some(&1.into()));
    assert_eq!(model.get_connection_name(), Some("models"));
    assert_eq!(
        model.get_attribute("created_at"),
        model.get_attribute("updated_at")
    );
    assert!(model.get_attribute("created_at").is_some());

    let mut hydrated = query().find_or_fail(1).await.unwrap();
    assert!(!hydrated.state().was_recently_created());
    assert_eq!(hydrated.get_attribute("protected"), Some(&7.into()));
    hydrated
        .set_attribute("article_id", 11)
        .set_attribute("title", "Renamed");
    assert_eq!(hydrated.state().get_dirty().len(), 2);
    assert!(hydrated.save().await.unwrap());
    assert_eq!(
        hydrated.state().get_changes().get("title"),
        Some(&"Renamed".into())
    );
    assert!(query().find(1).await.unwrap().is_none());
    assert_eq!(
        query().find_or_fail(11).await.unwrap().get_key(),
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
    let other = model.get_key().unwrap().clone();
    let mut draft = query()
        .create(attrs([
            ("title", "Draft".into()),
            ("account", 1.into()),
            ("status", "draft".into()),
        ]))
        .await
        .unwrap();
    let draft_key = draft.get_key().unwrap().clone();

    let mut scoped = query();
    scoped
        .r#where("title", "=", "Renamed")
        .or_where("title", "=", "Other account");
    scoped.with_scope(&Published).unwrap();
    assert_eq!(scoped.get().await.unwrap().len(), 1);
    assert!(matches!(
        query().find_or_fail(other.clone()).await,
        Err(DatabaseError::ModelNotFound("articles"))
    ));
    assert!(
        query()
            .without_global_scope("account")
            .find(other.clone())
            .await
            .unwrap()
            .is_some()
    );

    assert!(draft.delete().await.unwrap());
    assert!(draft.state().exists() && draft.trashed());
    assert!(query().find(draft_key.clone()).await.unwrap().is_none());
    assert_eq!(query().only_trashed().get().await.unwrap().len(), 1);
    assert!(draft.restore().await.unwrap());
    assert!(!draft.trashed());
    assert!(query().find(draft_key.clone()).await.unwrap().is_some());

    let mut bulk = query();
    bulk.where_key(draft_key.clone());
    assert_eq!(bulk.delete().await.unwrap(), 1);
    let mut bulk = query().only_trashed();
    bulk.where_key(draft_key.clone());
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
    hydrated.set_attribute("title", "Draft");
    assert!(hydrated.save().await.is_err());
    assert_eq!(
        hydrated.state().get_original().get("title"),
        Some(&"Renamed".into())
    );
    hydrated.set_attribute("title", "Retried");
    assert!(hydrated.save().await.unwrap());

    assert!(draft.force_delete().await.unwrap());
    assert!(!draft.state().exists());
    assert!(
        query()
            .with_trashed()
            .find(draft_key)
            .await
            .unwrap()
            .is_none()
    );
    let mut bulk = query();
    bulk.where_key(other);
    assert_eq!(bulk.force_delete().await.unwrap(), 1);
    assert_eq!(query().get().await.unwrap().len(), 1);
    Box::pin(verify_locked_reload(&db, &mut hydrated, mysql)).await;
    Box::pin(verify_model_reload(&db, &mut hydrated)).await;
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
    let mut label = Label::query_on(&db).make(Attributes::new()).unwrap();
    assert!(matches!(
        label.fill(attrs([("slug", "old".into())])),
        Err(DatabaseError::MassAssignment(key)) if key == "slug"
    ));
    label
        .force_fill(attrs([
            ("slug", "old".into()),
            ("title", "Original".into()),
        ]))
        .unwrap();
    assert!(label.save().await.unwrap());
    assert_eq!(label.get_key(), Some(&"old".into()));
    label.set_attribute("slug", "new");
    assert!(label.save().await.unwrap());
    assert!(Label::query_on(&db).find("old").await.unwrap().is_none());
    assert!(Label::query_on(&db).find("new").await.unwrap().is_some());
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

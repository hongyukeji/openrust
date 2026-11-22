use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Builder, Model, ModelKeyType, ModelMetadata, ModelState, Scope, casts::Attribute,
        },
    },
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

struct Item(ModelState);
struct TextItem(ModelState);
struct Visible;
impl Scope<Item> for Visible {
    fn apply(&self, builder: &mut Builder<'_, Item>) -> DatabaseResult<()> {
        builder.r#where("visible", "=", 1);
        Ok(())
    }
}
impl Model for Item {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("parity_model_keys").timestamps(None, None);
        &META
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
        &[("visible", &Visible)]
    }
    fn attribute(name: &str) -> Option<Attribute> {
        (name == "label").then(|| {
            Attribute::get(|value, _| match value {
                Some(Value::Text(text)) => Ok(text.to_uppercase().into()),
                _ => Ok(Value::Null),
            })
        })
    }
}
struct ValueItem(ModelState);
struct ValueVisible;
impl Scope<ValueItem> for ValueVisible {
    fn apply(&self, builder: &mut Builder<'_, ValueItem>) -> DatabaseResult<()> {
        builder.r#where("visible", "=", 1);
        Ok(())
    }
}
impl Model for ValueItem {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("parity_model_values").timestamps(None, None);
        &META
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
        &[("visible", &ValueVisible)]
    }
    fn attribute(name: &str) -> Option<Attribute> {
        (name == "label").then(|| {
            Attribute::get(|value, _| match value {
                Some(Value::Text(text)) => Ok(text.to_uppercase().into()),
                _ => Ok(Value::Null),
            })
        })
    }
}
impl Model for TextItem {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("parity_model_text_keys")
            .incrementing(false)
            .timestamps(None, None);
        &META
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
    fn getKeyType(&self) -> ModelKeyType {
        ModelKeyType::String
    }
}
fn connection() -> Connection {
    let config = match std::env::var("OPENRUST_TEST_ELOQUENT_PARITY_URL") {
        Ok(url) => json!({"url":url}),
        Err(_) => {
            assert!(std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_PARITY").is_none());
            json!({"driver":"sqlite","database":":memory:"})
        }
    };
    DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"test","connections":{"test":config}}),
        )]))),
        ".",
    )
    .connection(None)
    .unwrap()
}
async fn setup() -> Connection {
    setup_table("parity_model_keys").await
}
async fn setup_table(table: &str) -> Connection {
    let db = connection();
    db.statement(&format!("create table {table} (id integer primary key, label varchar(64), visible integer, nullable_value varchar(64))"), &[]).await.unwrap();
    for (id, text, visible) in [(1, "ada", 1), (2, "grace", 1), (3, "hidden", 0)] {
        db.statement(
            &format!("insert into {table} (id,label,visible) values (?,?,?)"),
            &[id.into(), text.into(), visible.into()],
        )
        .await
        .unwrap();
    }
    db
}
async fn ids(query: Builder<'static, Item>) -> Vec<i64> {
    let mut ids: Vec<_> = query
        .get()
        .await
        .unwrap()
        .into_iter()
        .map(|model| match model.getKey().unwrap() {
            Value::Integer(id) => *id,
            other => panic!("整数主键类型错误：{other:?}"),
        })
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn key_filters_preserve_scalar_list_null_or_groups_and_global_scopes() {
    let db = setup().await;
    let mut query = Item::queryOn(&db);
    query.whereKey(1);
    assert_eq!(ids(query).await, [1]);
    let mut query = Item::queryOn(&db);
    query.whereKey([1, 2, 3]);
    assert_eq!(ids(query).await, [1, 2]);
    let mut query = Item::queryOn(&db);
    query.whereKey(vec![Value::Text("2suffix".into()), Value::Null]);
    assert_eq!(ids(query).await, [2]);
    let mut query = Item::queryOn(&db);
    query.whereKeyNot(vec![1, 3]);
    assert_eq!(ids(query).await, [2]);
    let mut query = Item::queryOn(&db);
    query.whereKeyNot(1);
    assert_eq!(ids(query).await, [2]);
    let mut query = Item::queryOn(&db);
    query.whereKey(Vec::<Value>::new());
    assert!(ids(query).await.is_empty());
    let mut query = Item::queryOn(&db);
    query.whereKeyNot(Vec::<Value>::new());
    assert_eq!(ids(query).await, [1, 2]);
    let mut query = Item::queryOn(&db);
    query.whereKey(Value::Null);
    assert!(ids(query).await.is_empty());
    let mut query = Item::queryOn(&db);
    query.whereKeyNot(Value::Null);
    assert_eq!(ids(query).await, [1, 2]);
    let mut query = Item::queryOn(&db);
    query.whereKey(1).orWhereKey([2, 3]);
    assert_eq!(ids(query).await, [1, 2]);
    let mut query = Item::queryOn(&db);
    query.whereKey(1).orWhereKeyNot([1, 3]);
    assert_eq!(ids(query).await, [1, 2]);
    let mut query = Item::queryOn(&db);
    query.whereKey(3).orWhereKey(3);
    assert!(ids(query).await.is_empty(), "OR 主键不能绕过全局作用域");
    let mut query = Item::queryOn(&db);
    query.whereKey(3).orWhereKeyNot(3);
    assert_eq!(ids(query).await, [1, 2]);
    assert!(Item::queryOn(&db).find(1).await.unwrap().is_some());
}

#[tokio::test]
async fn text_keys_cast_scalars_but_preserve_list_strings_and_empty_sets() {
    let db = connection();
    db.statement(
        "create table parity_model_text_keys (id varchar(64) primary key)",
        &[],
    )
    .await
    .unwrap();
    for key in ["", "1", "7", "007", "x' OR 1=1 --"] {
        db.statement(
            "insert into parity_model_text_keys (id) values (?)",
            &[key.into()],
        )
        .await
        .unwrap();
    }
    for (key, expected) in [
        (Value::Integer(7), "7"),
        (Value::Bool(true), "1"),
        (Value::Bool(false), ""),
    ] {
        let mut query = TextItem::queryOn(&db);
        query.whereKey(key);
        assert_eq!(
            query.first().await.unwrap().unwrap().getKey(),
            Some(&expected.into())
        );
    }
    let mut query = TextItem::queryOn(&db);
    query.whereKey(["007", "x' OR 1=1 --"]);
    assert_eq!(query.get().await.unwrap().len(), 2);
    let mut query = TextItem::queryOn(&db);
    query.whereKeyNot(["007", "x' OR 1=1 --"]);
    assert_eq!(query.get().await.unwrap().len(), 3);
    let mut query = TextItem::queryOn(&db);
    query.whereKey(Vec::<Value>::new());
    assert!(query.get().await.unwrap().is_empty());
    let mut query = TextItem::queryOn(&db);
    query.whereKeyNot(Vec::<Value>::new());
    assert_eq!(query.get().await.unwrap().len(), 5);
}

#[tokio::test]
async fn single_value_queries_hydrate_accessors_preserve_projection_and_report_cardinality() {
    let db = setup_table("parity_model_values").await;
    let mut query = ValueItem::queryOn(&db);
    query.whereKey(1);
    assert_eq!(
        query.value("parity_model_values.label").await.unwrap(),
        "ADA".into()
    );
    let mut query = ValueItem::queryOn(&db);
    query.whereKey(1);
    assert_eq!(query.soleValue("label").await.unwrap(), "ADA".into());
    let mut query = ValueItem::queryOn(&db);
    query.whereKey(2);
    assert_eq!(query.valueOrFail("label").await.unwrap(), "GRACE".into());
    let mut query = ValueItem::queryOn(&db);
    query.whereKey(1);
    assert_eq!(
        query.valueOrFail("nullable_value").await.unwrap(),
        Value::Null
    );
    let mut query = ValueItem::queryOn(&db);
    query.whereKey(99);
    assert_eq!(query.value("label").await.unwrap(), Value::Null);
    for sole in [false, true] {
        let mut query = ValueItem::queryOn(&db);
        query.whereKey(99);
        let result = if sole {
            query.soleValue("label").await
        } else {
            query.valueOrFail("label").await
        };
        assert!(matches!(
            result,
            Err(DatabaseError::ModelNotFound("parity_model_values"))
        ));
    }
    assert!(matches!(
        ValueItem::queryOn(&db).limit(1).soleValue("label").await,
        Err(DatabaseError::MultipleModelsFound {
            table: "parity_model_values",
            count: 2
        })
    ));
    let mut query = ValueItem::queryOn(&db).select(["id"]);
    query.whereKey(1);
    assert_eq!(
        query.valueOrFail("label").await.unwrap(),
        Value::Null,
        "既有投影不能被单值接口覆盖"
    );
    assert_eq!(
        ValueItem::queryOn(&db)
            .orderBy("id", "desc")
            .valueOrFail("label")
            .await
            .unwrap(),
        "GRACE".into()
    );
    db.statement("drop table parity_model_values", &[])
        .await
        .unwrap();
    assert!(ValueItem::queryOn(&db).valueOrFail("label").await.is_err());
}

fn fixture_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(*value),
        serde_json::Value::Number(value) => Value::Integer(value.as_i64().unwrap()),
        serde_json::Value::String(value) => Value::Text(value.clone()),
        _ => panic!("主键参考只接收标量或列表"),
    }
}
async fn fixture_ids<M: Model>(db: &Connection, case: &serde_json::Value) -> serde_json::Value {
    let mut query = M::queryOn(db);
    let method = case["method"].as_str().unwrap();
    if method.starts_with("or") {
        query.r#where(
            "id",
            "=",
            if case["kind"] == "integer" {
                Value::Integer(1)
            } else {
                Value::Text("1".into())
            },
        );
    }
    let input = match &case["input"] {
        serde_json::Value::Array(values) => {
            openrust::database::eloquent::Key::Many(values.iter().map(fixture_value).collect())
        }
        value => openrust::database::eloquent::Key::Scalar(fixture_value(value)),
    };
    match method {
        "whereKey" => query.whereKey(input),
        "whereKeyNot" => query.whereKeyNot(input),
        "orWhereKey" => query.orWhereKey(input),
        "orWhereKeyNot" => query.orWhereKeyNot(input),
        _ => panic!("未知主键方法"),
    };
    serde_json::Value::Array(
        query
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap()
            .into_iter()
            .map(|model| match model.getKey().unwrap() {
                Value::Integer(value) => json!(value),
                Value::Text(value) => json!(value),
                value => panic!("参考主键类型错误：{value:?}"),
            })
            .collect(),
    )
}

#[tokio::test]
async fn sqlite_key_and_model_value_results_match_181_fixed_php_cases() {
    // 固定上游参考使用真实 PDO SQLite；本用例不冒充外部驱动验收。
    let db=DatabaseManager::new(Arc::new(Repository::new(BTreeMap::from([("database".into(),json!({"default":"reference","connections":{"reference":{"driver":"sqlite","database":":memory:"}}}))]))),".").connection(None).unwrap();
    db.statement("create table parity_model_keys (id integer primary key, label varchar(64), visible integer, nullable_value varchar(64))", &[]).await.unwrap();
    for (id, label, visible) in [(1, "ada", 1), (2, "grace", 1), (3, "hidden", 0)] {
        db.statement(
            "insert into parity_model_keys (id,label,visible) values (?,?,?)",
            &[id.into(), label.into(), visible.into()],
        )
        .await
        .unwrap();
    }
    db.statement(
        "create table parity_model_text_keys (id varchar(64) primary key)",
        &[],
    )
    .await
    .unwrap();
    for key in ["", "1", "7", "007", "x' OR 1=1 --"] {
        db.statement(
            "insert into parity_model_text_keys (id) values (?)",
            &[key.into()],
        )
        .await
        .unwrap();
    }
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/eloquent-key-values.json")).unwrap();
    assert_eq!(reference["keys"].as_array().unwrap().len(), 136);
    for case in reference["keys"].as_array().unwrap() {
        let result = if case["kind"] == "integer" {
            fixture_ids::<Item>(&db, case).await
        } else {
            fixture_ids::<TextItem>(&db, case).await
        };
        assert_eq!(result, case["ids"], "{case}");
    }
    assert_eq!(reference["values"].as_array().unwrap().len(), 45);
    for case in reference["values"].as_array().unwrap() {
        let mut query = Item::queryOn(&db).orderBy("id", "asc");
        if !case["id"].is_null() {
            query.whereKey(fixture_value(&case["id"]));
        }
        let column = case["column"].as_str().unwrap();
        let result = match case["method"].as_str().unwrap() {
            "value" => query.value(column).await,
            "soleValue" => query.soleValue(column).await,
            "valueOrFail" => query.valueOrFail(column).await,
            _ => panic!("未知单值方法"),
        };
        let result = match result {
            Ok(Value::Null) => json!({"ok":null}),
            Ok(Value::Text(value)) => json!({"ok":value}),
            Err(DatabaseError::ModelNotFound(_)) => json!({"error":"ModelNotFound"}),
            Err(DatabaseError::MultipleModelsFound { count, .. }) => {
                json!({"error":"MultipleModelsFound","count":count})
            }
            other => panic!("参考返回未覆盖：{other:?}"),
        };
        assert_eq!(result, case["result"], "{case}");
    }
}

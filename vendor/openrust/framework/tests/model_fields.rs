use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseManager, Value,
        eloquent::{
            Model, ModelMetadata, ModelState, model_fields,
            relations::{HasMany, Relation},
        },
    },
};
use serde_json::json;

struct Person(ModelState);
model_fields! {
    impl Person {
        pub const ID = "id";
        /// Rust 名称与数据库列名可以不同。
        pub const NAME = "display_name";
        pub const ORDER = "order";
        pub const PARENT_ID = "parent_id";
        pub const ABSENT = "not_in_database";
    }
}
impl Model for Person {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("field_reference_people").timestamps(None, None);
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
}

fn connection() -> Connection {
    let configuration = match std::env::var("OPENRUST_FIELD_DATABASE").as_deref() {
        Ok("postgres") => {
            json!({"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("需要隔离 PostgreSQL")})
        }
        Ok("mysql") => {
            json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").expect("需要隔离 MySQL")})
        }
        Ok(other) => panic!("未知验收数据库：{other}"),
        Err(_) => json!({"driver":"sqlite","database":":memory:"}),
    };
    DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"test","connections":{"test":configuration}}),
        )]))),
        ".",
    )
    .connection(None)
    .unwrap()
}

#[tokio::test]
async fn field_references_use_existing_sql_model_and_relationship_paths() {
    let db = connection();
    let quote = if db.getDriverName() == "mysql" {
        '`'
    } else {
        '"'
    };
    db.statement(&format!("create table field_reference_people (id integer primary key, display_name varchar(80), {quote}order{quote} integer, parent_id integer)"), &[]).await.unwrap();
    for (id, name, parent) in [
        (1, "张三", Value::Null),
        (2, "O'Reilly' OR 1=1 --", 1.into()),
    ] {
        db.table("field_reference_people")
            .insert(&[[
                (Person::ID.into(), id.into()),
                (Person::NAME.into(), name.into()),
                (Person::ORDER.into(), id.into()),
                (Person::PARENT_ID.into(), parent),
            ]
            .into()])
            .await
            .unwrap();
    }
    let mut named = db
        .table("field_reference_people")
        .r#where(Person::NAME, "=", "张三");
    let mut string = db
        .table("field_reference_people")
        .r#where("display_name", "=", "张三");
    assert_eq!(named.toSql().unwrap(), string.toSql().unwrap());
    assert_eq!(
        named.get_bindings().unwrap(),
        string.get_bindings().unwrap()
    );
    let named = named
        .select([Person::ID, Person::NAME])
        .orderBy(Person::ORDER, "desc")
        .get()
        .await
        .unwrap();
    let string = string
        .select(["id", "display_name"])
        .orderBy("order", "desc")
        .get()
        .await
        .unwrap();
    assert_eq!(named, string);
    assert_eq!(named.len(), 1);
    let malicious = db
        .table("field_reference_people")
        .r#where(Person::NAME, "=", "O'Reilly' OR 1=1 --")
        .get()
        .await
        .unwrap();
    assert_eq!(malicious.len(), 1);
    assert_eq!(
        db.table("field_reference_people")
            .where_null(Person::PARENT_ID)
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db.table("field_reference_people")
            .r#where(Person::PARENT_ID, "=", Value::Null)
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    let qualified_id = format!("p.{}", Person::ID);
    let qualified_parent = format!("c.{}", Person::PARENT_ID);
    let children = db
        .table("field_reference_people as p")
        .join(
            "field_reference_people as c",
            &qualified_id,
            "=",
            &qualified_parent,
        )
        .select([format!("c.{}", Person::NAME).as_str()])
        .orderBy(&qualified_id, "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(children.len(), 1);
    let mut query = Person::queryOn(&db);
    query.r#where(Person::ID, "=", 1);
    let parent = query.first().await.unwrap().unwrap();
    assert_eq!(
        parent.getAttribute(Person::NAME),
        Some(&Value::from("张三"))
    );
    let relation = HasMany::<Person, Person>::new(Person::ID, Person::PARENT_ID);
    assert_eq!(relation.getResults(&parent).await.unwrap().len(), 1);
    let mut query = Person::queryOn(&db);
    query.r#where(Person::ID, "=", 1);
    assert_eq!(
        query
            .update([(Person::NAME.into(), "李四".into())].into())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table("field_reference_people")
            .r#where(Person::NAME, "=", "李四")
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    // 声明不证明数据库列存在；错误仍由实际执行返回。
    assert!(
        db.table("field_reference_people")
            .r#where(
                &format!("field_reference_people.{}", Person::ABSENT),
                "=",
                1
            )
            .get()
            .await
            .is_err()
    );
    // 动态属性和原有原始表达式入口保持可用。
    let mut parent = parent;
    parent.setAttribute("runtime_only", "dynamic");
    assert_eq!(
        parent.getAttribute("runtime_only"),
        Some(&Value::from("dynamic"))
    );
    assert_eq!(
        db.table("field_reference_people")
            .selectRaw("count(*) as total", vec![])
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    db.getSchemaBuilder()
        .dropIfExists("field_reference_people")
        .await
        .unwrap();
}

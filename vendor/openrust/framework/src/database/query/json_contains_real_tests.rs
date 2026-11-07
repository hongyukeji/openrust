//! 真实PG的json/jsonb包含行为与写范围；经公开 `QueryBuilder` 执行。

use crate::{
    Error, Result,
    database::{
        Db,
        sea_orm::{ConnectionTrait, Value},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use serde_json::json;

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
#[ignore = "PostgreSQL 实库留待发布前数据库矩阵"]
async fn postgres_json_contains_preserves_json_types_unicode_empty_null_and_write_ranges()
-> Result<()> {
    let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    if !url.starts_with("postgres://") && !url.starts_with("postgresql://") {
        return Err(Error::custom(
            "test_json_contains_postgres_required",
            "PostgreSQL JSON 包含发布验收需要 PostgreSQL 连接",
        ));
    }
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let outcome: Result<()> = async {
        let connection = database.connection()?;
        connection.execute_unprepared("CREATE TABLE json_contains_items(id BIGINT PRIMARY KEY,doc_b JSONB,doc_j JSON,label TEXT,flag BIGINT)").await.map_err(Error::internal)?;
        let db = Db::default(); db.initialize(connection)?;
        let documents = [
            (1i64, Some(json!(["en","中文"]))),
            (2, Some(json!(["fr"]))),
            (3, Some(json!([]))),
            (4, Some(json!({"language":"中文","meta":{}}))),
            (5, Some(json!({}))),
            (6, Some(serde_json::Value::Null)),
            (7, None),
            (8, Some(json!({"languages":["en"]}))),
        ];
        for (id, document) in &documents {
            let document = Value::Json(document.clone().map(Box::new));
            db.table("json_contains_items")?.insert(vec![("id",(*id).into()),("doc_b",document.clone()),("doc_j",document),("label","original".into()),("flag",0i64.into())]).await?;
        }
        for column in ["doc_b", "doc_j"] {
            for (needle, expected) in [(json!(["en"]),1),(json!(["missing"]),0),(json!(["中文"]),1),(json!([]),3),(json!({}),3),(serde_json::Value::Null,1),(json!({"languages":["en"]}),1)] {
                assert_eq!(db.table("json_contains_items")?.where_json_contains(column,needle)?.count().await?,expected);
            }
        }
        let payload=json!(["en"]);
        let query=db.table("json_contains_items")?.where_gt("id",0i64)?.where_json_contains("doc_b",payload.clone())?.where_eq("label","original")?;
        assert_eq!(query.get_bindings()?,vec![0i64.into(),Value::Json(Some(Box::new(payload))),"original".into()]);
        let rows=query.get().await?;
        assert_eq!(rows.len(),1); assert_eq!(rows[0].try_get::<i64>("","id").map_err(Error::internal)?,1);
        assert_eq!(query.update(vec![("flag",9i64.into())]).await?,1);
        let assert_preserved = |row: &crate::database::sea_orm::QueryResult, id: i64, document: &Option<serde_json::Value>| -> Result<()> {
            assert_eq!(row.try_get::<i64>("", "id").map_err(Error::internal)?, id);
            assert_eq!(row.try_get::<i64>("", "flag").map_err(Error::internal)?, if id == 1 { 9 } else { 0 });
            assert_eq!(row.try_get::<String>("", "label").map_err(Error::internal)?, "original");
            assert_eq!(row.try_get::<Option<serde_json::Value>>("", "doc_b").map_err(Error::internal)?, *document);
            assert_eq!(row.try_get::<Option<serde_json::Value>>("", "doc_j").map_err(Error::internal)?, *document);
            Ok(())
        };
        let after_update = db.table("json_contains_items")?.order_by("id", crate::database::query::SortDirection::Asc)?.get().await?;
        assert_eq!(after_update.len(), documents.len());
        for (row, (id, document)) in after_update.iter().zip(&documents) { assert_preserved(row, *id, document)?; }
        assert_eq!(db.table("json_contains_items")?.where_eq("flag",9i64)?.count().await?,1);
        assert_eq!(db.table("json_contains_items")?.where_json_contains("doc_j",json!(["fr"]))?.delete().await?,1);
        assert_eq!(db.table("json_contains_items")?.count().await?,7);
        assert_eq!(db.table("json_contains_items")?.where_eq("id",2i64)?.count().await?,0);
        let after_delete = db.table("json_contains_items")?.order_by("id", crate::database::query::SortDirection::Asc)?.get().await?;
        assert_eq!(after_delete.len(), documents.len() - 1);
        for (row, (id, document)) in after_delete.iter().zip(documents.iter().filter(|(id, _)| *id != 2)) { assert_preserved(row, *id, document)?; }
        assert_eq!(db.table("json_contains_items")?.where_eq("id",7i64)?.count().await?,1);
        assert_eq!(db.table("json_contains_items")?.where_json_contains("doc_b",serde_json::Value::Null)?.count().await?,1);
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

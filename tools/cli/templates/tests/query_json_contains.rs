//! 验证生成应用的JSON包含值绑定、账户条件及精确读写范围。

use openrust::{
    Error, Result,
    database::{
        Db,
        query::SortDirection,
        sea_orm::{ConnectionTrait, DbBackend, QueryResult, Value},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use serde_json::json;

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
// {{runtime_scope}}
async fn generated_json_contains_preserves_owner_bindings_nulls_and_exact_write_targets()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    // {{database_setup}}
    let outcome: Result<()> = async {
        let db=Db::default();db.initialize(database.connection()?)?;
        let ddl=match db.backend()? {
            DbBackend::Postgres=>"CREATE TABLE json_contains_records(id BIGINT PRIMARY KEY,owner_id BIGINT,payload JSONB,label TEXT,flag BIGINT)",
            DbBackend::MySql=>"CREATE TABLE json_contains_records(id BIGINT PRIMARY KEY,owner_id BIGINT,payload JSON,label TEXT,flag BIGINT)",
            _=>return Err(Error::custom("generated_json_contains_backend","JSON包含测试只用于PostgreSQL或MySQL生成应用")),
        };
        db.connection()?.execute_unprepared(ddl).await.map_err(Error::internal)?;
        let literal="owner-bound ' text ? $1 中文";
        let records=[
            (1i64,7i64,Some(json!(["en","中文"]))),
            (2,8,Some(json!(["en","中文"]))),
            (3,7,Some(json!(["fr"]))),
            (4,7,Some(json!([]))),
            (5,7,Some(serde_json::Value::Null)),
            (6,7,None),
            (7,7,Some(json!({"language":"中文"}))),
            (8,8,Some(json!(["fr"]))),
        ];
        for (id,owner,payload) in &records {
            db.table("json_contains_records")?.insert(vec![("id",(*id).into()),("owner_id",(*owner).into()),("payload",Value::Json(payload.clone().map(Box::new))),("label",literal.into()),("flag",0i64.into())]).await?;
        }
        let needle=json!(["en"]);
        let query=db.table("json_contains_records")?.where_eq("owner_id",7i64)?.where_json_contains("payload",needle.clone())?.where_eq("label",literal)?;
        assert_eq!(query.get_bindings()?,vec![7i64.into(),Value::Json(Some(Box::new(needle))),literal.into()]);
        let rows=query.get().await?;
        assert_eq!(rows.len(),1);assert_eq!(rows[0].try_get::<i64>("","id").map_err(Error::internal)?,1);
        assert_eq!(query.count().await?,1);
        for (value,expected) in [(json!(["中文"]),1),(serde_json::Value::Null,1),(json!([]),3)] {
            assert_eq!(db.table("json_contains_records")?.where_eq("owner_id",7i64)?.where_json_contains("payload",value)?.count().await?,expected);
        }
        let null_rows=db.table("json_contains_records")?.where_eq("owner_id",7i64)?.where_json_contains("payload",serde_json::Value::Null)?.get().await?;
        assert_eq!(null_rows.len(),1);assert_eq!(null_rows[0].try_get::<i64>("","id").map_err(Error::internal)?,5);
        assert_eq!(db.table("json_contains_records")?.where_eq("owner_id",7i64)?.where_null("payload")?.count().await?,1);
        assert_eq!(query.update(vec![("flag",9i64.into())]).await?,1);
        let assert_record=|row:&QueryResult,id:i64,owner:i64,payload:&Option<serde_json::Value>|->Result<()> {
            assert_eq!(row.try_get::<i64>("","id").map_err(Error::internal)?,id);
            assert_eq!(row.try_get::<i64>("","owner_id").map_err(Error::internal)?,owner);
            assert_eq!(row.try_get::<Option<serde_json::Value>>("","payload").map_err(Error::internal)?,*payload);
            assert_eq!(row.try_get::<String>("","label").map_err(Error::internal)?,literal);
            assert_eq!(row.try_get::<i64>("","flag").map_err(Error::internal)?,if id==1 {9} else {0});
            Ok(())
        };
        let updated=db.table("json_contains_records")?.order_by("id",SortDirection::Asc)?.get().await?;
        assert_eq!(updated.len(),records.len());
        for (row,(id,owner,payload)) in updated.iter().zip(&records) {assert_record(row,*id,*owner,payload)?;}
        assert_eq!(db.table("json_contains_records")?.where_eq("owner_id",7i64)?.where_json_contains("payload",json!(["fr"]))?.where_eq("label",literal)?.delete().await?,1);
        assert_eq!(db.table("json_contains_records")?.where_eq("id",3i64)?.count().await?,0);
        let remaining=db.table("json_contains_records")?.order_by("id",SortDirection::Asc)?.get().await?;
        assert_eq!(remaining.len(),records.len()-1);
        for (row,(id,owner,payload)) in remaining.iter().zip(records.iter().filter(|(id,_,_)|*id!=3)) {assert_record(row,*id,*owner,payload)?;}
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup?;
    Ok(())
}

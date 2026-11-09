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
        qualified_selectors_preserve_groups_and_write_targets(&db).await?;
        key_exists_preserves_nulls_scope_and_write_targets(&db).await?;
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup?;
    Ok(())
}

async fn qualified_selectors_preserve_groups_and_write_targets(db: &Db) -> Result<()> {
    let table = "json_contains_records";
    let selector = "json_contains_records.payload->nested->tags";
    let marker = "qualified selector";
    for (id, owner, payload) in [
        (11_i64, 7_i64, Some(json!({"nested":{"tags":["en","blocked"]}}))),
        (12, 7, Some(json!({"nested":{"tags":["fr"]},"a'b ? $1":"needle"}))),
        (13, 7, Some(json!({"nested":{"tags":["blocked"]}}))),
        (14, 7, Some(json!({"nested":{}}))),
        (15, 7, None),
        (16, 8, Some(json!({"nested":{"tags":["fr"]}}))),
    ] {
        db.table(table)?
            .insert(vec![
                ("id", id.into()),
                ("owner_id", owner.into()),
                ("payload", Value::Json(payload.map(Box::new))),
                ("label", marker.into()),
                ("flag", 0_i64.into()),
            ])
            .await?;
    }
    let owner = || db.table(table)?.where_eq("owner_id", 7_i64)?.where_eq("label", marker);
    let group = owner()?.where_nested(|group| {
        group
            .where_json_contains(selector, json!("en"))?
            .or_where_json_contains(selector, json!("fr"))
    })?;
    assert_eq!(group.count().await?, 2);
    assert_eq!(
        owner()?
            .where_json_contains("json_contains_records.payload->a'b ? $1", json!("needle"))?
            .count()
            .await?,
        1
    );
    let negative = owner()?.where_json_doesnt_contain(selector, json!("blocked"))?;
    assert_eq!(negative.update(vec![("flag", 4_i64.into())]).await?, 1);
    assert_eq!(
        db.table(table)?.where_eq("id", 12_i64)?.where_eq("flag", 4_i64)?.count().await?,
        1
    );
    assert_eq!(
        owner()?
            .where_nested(|group| {
                group
                    .where_json_contains(selector, json!("en"))?
                    .or_where_json_doesnt_contain(selector, json!("blocked"))
            })?
            .delete()
            .await?,
        2
    );
    let rows = db.table(table)?.where_eq("label", marker)?
        .order_by("id", SortDirection::Asc)?.get().await?;
    let remaining = rows.iter()
        .map(|row| row.try_get::<i64>("", "id").map_err(Error::internal))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(remaining, [13, 14, 15, 16]);
    Ok(())
}

async fn key_exists_preserves_nulls_scope_and_write_targets(db: &Db) -> Result<()> {
    let table = "json_contains_records";
    let marker = "key existence";
    let selector = "json_contains_records.payload->language";
    let element = "payload->items[0][0]";
    for (id, owner, payload) in [
        (21_i64, 7_i64, Some(json!({"language":null,"items":[[null]],"a'b ? $1":null}))),
        (22, 7, Some(json!({"items":[]}))),
        (23, 7, None),
        (24, 8, Some(json!({"language":null,"items":[[null]]}))),
        (25, 7, Some(json!({"language":"中文"}))),
    ] {
        db.table(table)?.insert(vec![
            ("id", id.into()), ("owner_id", owner.into()), ("label", marker.into()),
            ("payload", Value::Json(payload.map(Box::new))), ("flag", 0_i64.into()),
        ]).await?;
    }
    let owner = || db.table(table)?.where_eq("owner_id", 7_i64)?.where_eq("label", marker);
    let exists = db.table(table)?.where_eq("owner_id", 7_i64)?
        .where_json_contains_key(selector)?.where_eq("label", marker)?;
    assert_eq!(exists.get_bindings()?, vec![7_i64.into(), marker.into()]);
    let page = exists.clone().order_by("id", SortDirection::Asc)?.paginate_on(1, "page", 2).await?;
    assert_eq!(page.total, 2);
    assert_eq!(key_row_ids(&page.items)?, [25]);
    assert_eq!(
        key_row_ids(&owner()?.where_json_contains_key(element)?.get().await?)?,
        [21]
    );
    assert_eq!(owner()?.where_json_contains_key("payload->a'b ? $1")?.count().await?, 1);
    if db.backend()? == DbBackend::Postgres {
        assert_eq!(owner()?.where_json_contains_key("payload->items[-1][-1]")?.count().await?, 1);
    }
    let either = owner()?.where_nested(|group| {
        group.where_json_contains_key(element)?.or_where_json_doesnt_contain_key(selector)
    })?.order_by("id", SortDirection::Asc)?;
    assert_eq!(key_row_ids(&either.get().await?)?, [21, 22, 23]);
    let reversed = owner()?.where_nested(|group| {
        group.where_json_doesnt_contain_key(selector)?.or_where_json_contains_key(element)
    })?.order_by("id", SortDirection::Asc)?;
    assert_eq!(key_row_ids(&reversed.get().await?)?, [21, 22, 23]);
    assert_eq!(exists.update(vec![("flag", 7_i64.into())]).await?, 2);
    let updated = db.table(table)?.where_eq("label", marker)?.where_eq("flag", 7_i64)?
        .order_by("id", SortDirection::Asc)?.get().await?;
    assert_eq!(key_row_ids(&updated)?, [21, 25]);
    assert_eq!(owner()?.where_json_doesnt_contain_key(selector)?.delete().await?, 2);
    let left = db.table(table)?.where_eq("label", marker)?
        .order_by("id", SortDirection::Asc)?.get().await?;
    assert_eq!(key_row_ids(&left)?, [21, 24, 25]);
    Ok(())
}

fn key_row_ids(rows: &[QueryResult]) -> Result<Vec<i64>> {
    rows.iter().map(|row| row.try_get("", "id").map_err(Error::internal)).collect()
}

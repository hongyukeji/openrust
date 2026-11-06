//! 验证生成应用的多列分组、NULL三值与无FROM子查询使用同一公开查询入口。

use openrust::{
    Error, Result,
    database::{
        Db,
        query::{MultiColumn, MultiComparison, MultiValue, RawSql},
        sea_orm::ConnectionTrait,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
async fn generated_multiple_columns_preserve_groups_nulls_and_subquery_contexts()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    // {{database_setup}}
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        db.connection()?
            .execute_unprepared(
                "CREATE TABLE multi_items (id INTEGER PRIMARY KEY, a INTEGER, b INTEGER)",
            )
            .await
            .map_err(Error::internal)?;
        for (id, a, b) in [
            (1_i64, 1_i64, Some(1_i64)),
            (2, 2, Some(3)),
            (3, 1, None),
            (4, 2, None),
        ] {
            db.table("multi_items")?
                .insert(vec![("id", id.into()), ("a", a.into()), ("b", b.into())])
                .await?;
        }
        assert_eq!(
            db.table("multi_items")?
                .where_any(["a", "b"], MultiComparison::eq(1_i64))?
                .count()
                .await?,
            2
        );
        assert_eq!(
            db.table("multi_items")?
                .where_all(["a", "b"], MultiComparison::eq(1_i64))?
                .count()
                .await?,
            1
        );
        // (2,NULL)的NOT(FALSE OR UNKNOWN)仍是UNKNOWN，不能被where_none选中。
        assert_eq!(
            db.table("multi_items")?
                .where_none(["a", "b"], MultiComparison::eq(1_i64))?
                .count()
                .await?,
            1
        );
        assert_eq!(
            db.table("multi_items")?
                .r#where("id", 2_i64)?
                .or_where_all(["a", "b"], MultiComparison::eq(1_i64))?
                .count()
                .await?,
            2
        );
        let nested = [
            MultiColumn::callback(|query| query.r#where("a", 1_i64)),
            MultiColumn::from("b"),
        ];
        assert_eq!(
            db.table("multi_items")?
                .where_all(nested, MultiComparison::Omitted)?
                .count()
                .await?,
            1
        );
        let scalar = [
            MultiColumn::callback(|query| query.select_raw(RawSql::new("?"), vec![1_i64.into()])),
            MultiColumn::from("a"),
        ];
        assert_eq!(
            db.table("multi_items")?
                .where_all(scalar, MultiComparison::eq(1_i64))?
                .count()
                .await?,
            2
        );
        let repeated =
            MultiValue::callback(|query| query.select_raw(RawSql::new("?"), vec![1_i64.into()]));
        assert_eq!(
            db.table("multi_items")?
                .where_all(["a", "b"], MultiComparison::eq(repeated))?
                .count()
                .await?,
            1
        );
        let rows = db
            .query()
            .select_raw(RawSql::new("? AS amount"), vec![7_i64.into()])?
            .get()
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| Error::custom("generated_projection_missing", "常量投影未返回行"))?;
        let amount: i64 = row.try_get("", "amount").map_err(Error::internal)?;
        assert_eq!(amount, 7);
        Ok(())
    }
    .await;
    database.close().await?;
    outcome?;
    Ok(())
}

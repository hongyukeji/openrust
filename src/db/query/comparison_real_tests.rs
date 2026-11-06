//! 真实PostgreSQL两种字符串模式下普通JSON选择器的词法边界；不执行显式RawSql输入。

use super::*;
use crate::db::Db;
use crate::{
    db::sea_orm_migration::{MigrationTrait, MigratorTrait},
    testing::TestDatabase,
};
use sea_orm::{ConnectionTrait, TransactionTrait};

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
async fn postgres_json_paths_remain_data_with_standard_strings_on_and_off() -> Result<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过PG路径词法专项：未配置OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    if !url.starts_with("postgres://") && !url.starts_with("postgresql://") {
        eprintln!("跳过PG路径词法专项：当前配置不是PostgreSQL");
        return Ok(());
    }
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let outcome: Result<()> = async {
        let connection = database.connection()?;
        connection
            .execute_unprepared("CREATE TABLE comparison_path_items (id BIGINT, document JSONB)")
            .await
            .map_err(Error::internal)?;
        let db = Db::default();
        db.initialize(connection.clone())?;
        let key = "a\\' OR true)-- $1 ?";
        for (id, boolean) in [(1i64, true), (2i64, false)] {
            let document = serde_json::json!({(key):boolean});
            db.table("comparison_path_items")?
                .insert(vec![
                    ("id", id.into()),
                    ("document", Value::Json(Some(Box::new(document)))),
                ])
                .await?;
        }
        let selector = format!("document->{key}");
        for setting in [
            "SET LOCAL standard_conforming_strings = on",
            "SET LOCAL standard_conforming_strings = off",
        ] {
            let transaction = connection.begin().await.map_err(Error::internal)?;
            transaction
                .execute_unprepared(setting)
                .await
                .map_err(Error::internal)?;
            transaction
                .execute_unprepared("SET LOCAL backslash_quote = on")
                .await
                .map_err(Error::internal)?;
            assert_eq!(
                db.table("comparison_path_items")?
                    .using_transaction(&transaction)
                    .r#where("id", 1i64)?
                    .r#where(selector.as_str(), true)?
                    .count()
                    .await?,
                1
            );
            assert_eq!(
                db.table("comparison_path_items")?
                    .using_transaction(&transaction)
                    .r#where("id", 2i64)?
                    .r#where(selector.as_str(), true)?
                    .count()
                    .await?,
                0
            );
            let query = db
                .table("comparison_path_items")?
                .using_transaction(&transaction)
                .r#where(selector.as_str(), true)?;
            assert!(query.to_sql()?.contains("E'"));
            assert_eq!(query.get_bindings()?, [] as [Value; 0]);
            drop(query);
            transaction.rollback().await.map_err(Error::internal)?;
        }
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn real_backend_sub_keeps_original_operators_and_mysql_raw_json_null_semantics() -> Result<()>
{
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过真实比较专项：未配置OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let outcome:Result<()>=async {
        let connection=database.connection()?;
        let backend=connection.get_database_backend();
        let ddl=match backend {
            DbBackend::MySql=>"CREATE TABLE comparison_sub_items (id BIGINT, document JSON, label TEXT)",
            DbBackend::Postgres=>"CREATE TABLE comparison_sub_items (id BIGINT, document JSONB, label TEXT)",
            _=>return Err(unsupported_backend()),
        };
        connection.execute_unprepared(ddl).await.map_err(Error::internal)?;
        let db=Db::default(); db.initialize(connection)?;
        for (id,document) in [(1i64,serde_json::json!({"active":null})),(2,serde_json::json!({})),(3,serde_json::json!({"active":true}))] {
            db.table("comparison_sub_items")?.insert(vec![("id",id.into()),("document",Value::Json(Some(Box::new(document)))),("label","label".into())]).await?;
        }
        let sub=db.table("comparison_sub_items")?.select(&["label"])?.r#where("id",1i64)?;
        for operator in [WhereOperator::Like,WhereOperator::BitAnd,WhereOperator::NullSafeEq] {
            let query=db.table("comparison_sub_items")?.where_compare("label",operator,&sub)?;
            let sql=query.to_sql()?;
            assert!(!sql.contains("::text")); assert!(!sql.contains("::bool"));
            assert!(sql.contains(operator.sql()));
            if operator==WhereOperator::NullSafeEq { assert!(!sql.contains("is not distinct from")); }
        }
        if backend==DbBackend::MySql {
            for raw in [false,true] {
                let column=if raw { ColumnOperand::from(RawSql::new("document->active")) } else { ColumnOperand::from("document->active") };
                assert_eq!(db.table("comparison_sub_items")?.r#where(column,Option::<i64>::None)?.count().await?,2);
            }
            assert_eq!(db.table("comparison_sub_items")?.where_compare(RawSql::new("document->active"),WhereOperator::NotEq,Option::<i64>::None)?.count().await?,1);
            assert!(matches!(db.table("comparison_sub_items")?.where_compare("id",WhereOperator::DistinctFrom,Option::<i64>::None),Err(error) if error.code()=="query_operator_invalid"));
        } else {
            assert!(matches!(db.table("comparison_sub_items")?.where_compare("id",WhereOperator::SoundsLike,Option::<i64>::None),Err(error) if error.code()=="query_operator_invalid"));
        }
        assert_eq!(db.table("comparison_sub_items")?.count().await?,3);
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

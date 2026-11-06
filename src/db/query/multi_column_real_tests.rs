//! 真实网络后端的多列分组、空查询原始投影及事务回滚。
use super::*;
use crate::db::Db;
use crate::{
    db::sea_orm_migration::{MigrationTrait, MigratorTrait},
    testing::TestDatabase,
};
use sea_orm::{ConnectionTrait, DbBackend, TransactionTrait};
struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
#[tokio::test]
async fn real_backend_multi_groups_raw_projection_and_transaction_rollback() -> Result<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过真实多列专项：未配置OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let outcome:Result<()>=async {
        let connection=database.connection()?;
        connection.execute_unprepared("CREATE TABLE multi_items(id BIGINT PRIMARY KEY,a BIGINT,b BIGINT,label VARCHAR(64))").await.map_err(Error::internal)?;
        let db=Db::default();db.initialize(connection.clone())?;
        for (id,a,b) in [(1i64,Some(1i64),Some(1i64)),(2,Some(1),Some(2)),(3,None,Some(2)),(4,None,None),(5,Some(2),Some(2))] {
            db.table("multi_items")?.insert(vec![("id",id.into()),("a",a.into()),("b",b.into()),("label","Alpha".into())]).await?;
        }
        assert_eq!(db.table("multi_items")?.where_any(["a","b"],1i64)?.count().await?,2);
        assert_eq!(db.table("multi_items")?.where_all(["a","b"],1i64)?.count().await?,1);
        assert_eq!(db.table("multi_items")?.where_none(["a","b"],1i64)?.count().await?,1);
        let rows=db.query().select_raw(RawSql::new("? AS first"),vec![11i64.into()])?.select_raw(RawSql::new("? AS second"),vec![22i64.into()])?.get().await?;
        assert_eq!(rows[0].try_get::<i64>("","first").map_err(Error::internal)?,11);
        assert_eq!(rows[0].try_get::<i64>("","second").map_err(Error::internal)?,22);
        let rhs=MultiValue::callback(|q|q.select_raw(RawSql::new("?"),vec![1i64.into()]));
        assert_eq!(db.table("multi_items")?.where_all(["a","b"],MultiComparison::eq(rhs))?.count().await?,1);
        let tx=connection.begin().await.map_err(Error::internal)?;
        let query=db.table("multi_items")?.using_transaction(&tx).where_none(["a","b"],Option::<i64>::None)?;
        assert_eq!(query.update(vec![("label","changed".into())]).await?,3);
        assert_eq!(db.table("multi_items")?.using_transaction(&tx).r#where("label","changed")?.delete().await?,3);
        drop(query);tx.rollback().await.map_err(Error::internal)?;
        assert_eq!(db.table("multi_items")?.count().await?,5);
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn real_raw_projection_driver_lexical_regions_preserve_parameter_values() -> Result<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过raw词法真实专项：未配置OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let outcome:Result<()>=async {
        let connection=database.connection()?;let backend=connection.get_database_backend();
        let db=Db::default();db.initialize(connection.clone())?;
        match backend {
            DbBackend::Postgres=>{
                for setting in ["SET LOCAL standard_conforming_strings=on","SET LOCAL standard_conforming_strings=off"] {
                    let tx=connection.begin().await.map_err(Error::internal)?;
                    tx.execute_unprepared(setting).await.map_err(Error::internal)?;
                    let query=db.query().using_transaction(&tx).select_raw(RawSql::new("?::text || $标$? $1$标$ AS value"),vec!["bound".into()])?;
                    let rows=query.get().await?;assert_eq!(rows[0].try_get::<String>("","value").map_err(Error::internal)?,"bound? $1");
                    let escaped=db.query().using_transaction(&tx).select_raw(RawSql::new(r"E'\\?' || ? AS value"),vec!["bound".into()])?;
                    let rows=escaped.get().await?;assert_eq!(rows[0].try_get::<String>("","value").map_err(Error::internal)?,r"\?bound");
                    drop(escaped);drop(query);tx.rollback().await.map_err(Error::internal)?;
                }
                let tx=connection.begin().await.map_err(Error::internal)?;
                tx.execute_unprepared("SET LOCAL standard_conforming_strings=on").await.map_err(Error::internal)?;
                let query=db.query().using_transaction(&tx).select_raw(RawSql::new(r"CASE WHEN true THEN '' ELSE'\' END || ? AS value"),vec!["safe".into()])?;
                let rows=query.get().await?;assert_eq!(rows[0].try_get::<String>("","value").map_err(Error::internal)?,"safe");
                drop(query);tx.rollback().await.map_err(Error::internal)?;
                assert!(matches!(db.query().select_raw(RawSql::new("$1 || ?"),vec!["bound".into()]),Err(error) if error.code()=="query_raw_native_bindings_unsupported"));
            }
            DbBackend::MySql=>{
                let rows=db.query().select_raw(RawSql::new("? /*! + ? */ + ? AS value"),vec![1i64.into(),2i64.into(),3i64.into()])?.get().await?;
                // MySQL将算术参数表达式的结果推断为DOUBLE，不按绑定参数的BIGINT类别读取。
                assert_eq!(rows[0].try_get::<f64>("","value").map_err(|_| Error::custom("query_raw_sum_decode", "MySQL原始算术投影结果必须能按DOUBLE读取"))?,6.0);
                assert!(matches!(db.query().select_raw(RawSql::new("1 /*!80000 + ? */ + ?"),vec![91.into(),2.into()]),Err(error) if error.code()=="query_raw_executable_comment_bindings"));
                assert!(matches!(db.query().select_raw(RawSql::new(r"'\?' AS value"),vec![]),Err(error) if error.code()=="query_raw_sql_mode_ambiguous"));
                let rows=db.query().select_raw(RawSql::new("? AS value"),vec![r"\?".into()])?.get().await?;
                assert_eq!(rows[0].try_get::<String>("","value").map_err(Error::internal)?,r"\?");
                let rows=db.query().select_raw(RawSql::new("@@session.time_zone AS value"),vec![])?.get().await?;
                assert_ne!(rows[0].try_get::<String>("","value").map_err(Error::internal)?, "");
            }
            _=>return Err(super::super::unsupported_backend()),
        }
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

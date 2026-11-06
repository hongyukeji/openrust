//! 可发现假空FROM、原始投影绑定错序、恢复丢值及占位符二次词法解释的缺陷。
#[cfg(any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres"))]
use super::super::WhereOperator;
use super::*;
#[cfg(any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres"))]
use sea_orm::ConnectionTrait;
#[cfg(feature = "db-sqlite")]
use sea_orm::Database;
use sea_orm::sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};
#[cfg(feature = "db-sqlite")]
async fn database() -> Result<Db> {
    let db = Db::default();
    db.initialize(
        Database::connect("sqlite::memory:")
            .await
            .map_err(Error::internal)?,
    )?;
    Ok(db)
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn empty_query_raw_projection_replacement_restore_and_no_table_writes() -> Result<()> {
    let db = database().await?;
    let query = db
        .query()
        .select_raw(RawSql::new("? AS first"), vec![11.into()])?
        .select_raw(RawSql::new("? AS second"), vec![22.into()])?;
    assert!(!query.to_sql()?.contains("FROM"));
    assert!(!query.to_sql()?.contains('*'));
    assert_eq!(query.get_bindings()?, vec![11.into(), 22.into()]);
    let rows = query.get().await?;
    assert_eq!(
        rows[0]
            .try_get::<i64>("", "first")
            .map_err(Error::internal)?,
        11
    );
    assert_eq!(
        rows[0]
            .try_get::<i64>("", "second")
            .map_err(Error::internal)?,
        22
    );
    assert_eq!(query.get_bindings()?, vec![11.into(), 22.into()]);
    assert_eq!(query.count().await?, 1);
    assert!(
        matches!(db.query().insert(vec![("id",1.into())]).await,Err(error) if error.code()=="query_table_required")
    );
    assert!(
        matches!(db.query().update(vec![("id",1.into())]).await,Err(error) if error.code()=="query_table_required")
    );
    assert!(matches!(db.query().delete().await,Err(error) if error.code()=="query_table_required"));
    db.primary_connection()?
        .execute_unprepared("CREATE TABLE source(id INTEGER); INSERT INTO source VALUES(7)")
        .await
        .map_err(Error::internal)?;
    let selected = db
        .query()
        .from("source")?
        .select_raw(RawSql::new("? AS removed"), vec![99.into()])?
        .select(&["id"])?;
    assert_eq!(selected.get_bindings()?, [] as [Value; 0]);
    assert_eq!(selected.value::<i64>("id").await?, Some(7));
    Ok(())
}
#[test]
fn raw_templates_protect_nonparameter_regions_and_declared_binding_order() -> Result<()> {
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let expression = raw_projection(
            "? + ? /* ? */ + length('?')",
            vec![1.into(), 2.into()],
            backend,
        )?;
        let select = Query::select().expr(expression).to_owned();
        let (sql, values) = match backend {
            DbBackend::MySql => select.build(MysqlQueryBuilder),
            DbBackend::Postgres => select.build(PostgresQueryBuilder),
            DbBackend::Sqlite => select.build(SqliteQueryBuilder),
            _ => return Err(unsupported_backend()),
        };
        assert_eq!(values.0, vec![1.into(), 2.into()]);
        assert!(sql.contains("/* ? */"));
        assert!(sql.contains("'?'"));
    }
    let expression = raw_projection(
        "? + E'\\\\?'::text || $tag$? $1$tag$ /* a /* ? */ b */ -- ?\n || '?'",
        vec![3.into()],
        DbBackend::Postgres,
    )?;
    let (sql, values) = Query::select().expr(expression).build(PostgresQueryBuilder);
    assert_eq!(values.0, vec![3.into()]);
    assert!(sql.contains("$tag$? $1$tag$"));
    let expression = raw_projection("payload ?? ?", vec!["key".into()], DbBackend::Postgres)?;
    let (sql, values) = Query::select().expr(expression).build(PostgresQueryBuilder);
    assert!(sql.contains("payload ?"));
    assert_eq!(values.0, vec!["key".into()]);
    for (sql, values) in [
        ("?", vec![]),
        ("1", vec![1.into()]),
        ("'unterminated", vec![]),
        ("$tag$unterminated", vec![]),
    ] {
        assert!(
            matches!(raw_projection(sql,values,DbBackend::Postgres),Err(error) if error.code()=="query_raw_bindings_invalid")
        );
    }
    Ok(())
}

#[test]
fn raw_lexing_keeps_quoted_identifiers_and_escaped_questions_out_of_bindings() -> Result<()> {
    for (backend, sql) in [
        (DbBackend::MySql, "? AS `literal?`, '?' AS value -- ?\n"),
        (DbBackend::Sqlite, "? AS [literal?], '?' AS value -- ?\n"),
        (
            DbBackend::Postgres,
            "? AS \"literal?\", E'\\\\?' AS value /* outer /* ? */ inner */",
        ),
    ] {
        let expression = raw_projection(sql, vec![9.into()], backend)?;
        let statement = Query::select().expr(expression).to_owned();
        let (_, values) = match backend {
            DbBackend::MySql => statement.build(MysqlQueryBuilder),
            DbBackend::Sqlite => statement.build(SqliteQueryBuilder),
            DbBackend::Postgres => statement.build(PostgresQueryBuilder),
            _ => return Err(unsupported_backend()),
        };
        assert_eq!(values.0, vec![9.into()]);
    }
    assert!(
        matches!(raw_projection("$2 || ? || $1",vec!["declared".into()],DbBackend::Postgres),Err(error) if error.code()=="query_raw_native_bindings_unsupported")
    );
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn raw_projection_bindings_precede_where_and_survive_pagination_restore() -> Result<()> {
    let db = database().await?;
    db.primary_connection()?
        .execute_unprepared("CREATE TABLE pages(id INTEGER); INSERT INTO pages VALUES(1),(2),(3)")
        .await
        .map_err(Error::internal)?;
    let query = db
        .table("pages")?
        .select_raw(RawSql::new("id, ? AS marker"), vec![17.into()])?
        .r#where("id", 2)?;
    assert_eq!(query.get_bindings()?, vec![17.into(), 2.into()]);
    assert_eq!(query.count().await?, 1);
    let page = query.paginate(1).await?;
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0]
            .try_get::<i64>("", "marker")
            .map_err(Error::internal)?,
        17
    );
    assert_eq!(query.get_bindings()?.first(), Some(&Value::from(17)));
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_cr_comment_and_native_parameters_have_real_value_witnesses() -> Result<()> {
    let db = database().await?;
    let rows = db
        .query()
        .select_raw(
            RawSql::new("1 -- hidden\r ?\n + ? AS value"),
            vec![2.into()],
        )?
        .get()
        .await?;
    assert_eq!(
        rows[0]
            .try_get::<i64>("", "value")
            .map_err(Error::internal)?,
        3
    );
    for sql in [
        "?1 + ?",
        ":named + ?",
        "@named + ?",
        "$named + ?",
        "??1 + ?",
    ] {
        assert!(
            matches!(db.query().select_raw(RawSql::new(sql),vec![9.into()]),Err(error) if error.code()=="query_raw_native_bindings_unsupported")
        );
    }
    let rows = db
        .query()
        .select_raw(RawSql::new("'?1 :named @named $named' AS value"), vec![])?
        .get()
        .await?;
    assert_eq!(
        rows[0]
            .try_get::<String>("", "value")
            .map_err(Error::internal)?,
        "?1 :named @named $named"
    );
    db.primary_connection()?
        .execute_unprepared(
            "CREATE TABLE native_items(\"foo$1\" INTEGER); INSERT INTO native_items VALUES(17)",
        )
        .await
        .map_err(Error::internal)?;
    let rows = db
        .table("native_items")?
        .select_raw(RawSql::new("foo$1 AS value"), vec![])?
        .get()
        .await?;
    assert_eq!(
        rows[0]
            .try_get::<i64>("", "value")
            .map_err(Error::internal)?,
        17
    );
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_escaped_questions_and_all_native_variable_names_cannot_create_ghost_binds()
-> Result<()> {
    let db = database().await?;
    for sql in [
        "?? + ?",
        "#abc + ?",
        ":$ + ?",
        "@$ + ?",
        "$$ + ?",
        "$::name + ?",
        ":$::suffix + ?",
    ] {
        assert!(
            matches!(db.query().select_raw(RawSql::new(sql),vec![7.into()]),Err(error) if error.code()=="query_raw_native_bindings_unsupported")
        );
    }
    db.primary_connection()?
        .execute_unprepared(
            "CREATE TABLE raw_guarded(id INTEGER); INSERT INTO raw_guarded VALUES(1),(2)",
        )
        .await
        .map_err(Error::internal)?;
    let attempted: Result<u64> = async {
        let scalar = db
            .query()
            .select_raw(RawSql::new("?? + ?"), vec![7.into()])?;
        db.table("raw_guarded")?
            .where_compare("id", WhereOperator::Eq, &scalar)?
            .delete()
            .await
    }
    .await;
    assert!(
        matches!(attempted,Err(error) if error.code()=="query_raw_native_bindings_unsupported")
    );
    assert_eq!(db.table("raw_guarded")?.count().await?, 2);
    let rows = db
        .query()
        .select_raw(
            RawSql::new("? AS value, '?? #abc :$ @$' AS literal /* ?? #abc :$ @$ */"),
            vec![7.into()],
        )?
        .get()
        .await?;
    assert_eq!(
        rows[0]
            .try_get::<i64>("", "value")
            .map_err(Error::internal)?,
        7
    );
    assert_eq!(
        rows[0]
            .try_get::<String>("", "literal")
            .map_err(Error::internal)?,
        "?? #abc :$ @$"
    );
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct FinalRawMigrator;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl crate::db::sea_orm_migration::MigratorTrait for FinalRawMigrator {
    fn migrations() -> Vec<Box<dyn crate::db::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
async fn real_final_raw_question_rules_keep_driver_values_and_guard_writes() -> Result<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过raw最终参数边界：未配置OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = crate::testing::TestDatabase::create::<FinalRawMigrator>(&url).await?;
    let outcome:Result<()>=async {
        let connection=database.connection()?;let backend=connection.get_database_backend();
        let db=Db::default();db.initialize(connection.clone())?;
        match backend {
            DbBackend::MySql=>{
                connection.execute_unprepared("CREATE TABLE raw_guarded(id BIGINT); INSERT INTO raw_guarded VALUES(1),(2)").await.map_err(Error::internal)?;
                let attempted:Result<u64>=async {
                    let scalar=db.query().select_raw(RawSql::new("?? + ?"),vec![7.into()])?;
                    db.table("raw_guarded")?.where_compare("id",WhereOperator::Eq,&scalar)?.delete().await
                }.await;
                assert!(matches!(attempted,Err(error) if error.code()=="query_raw_native_bindings_unsupported"));
                assert_eq!(db.table("raw_guarded")?.count().await?,2);
                let rows=db.query().select_raw(RawSql::new("? AS value, '?? #abc :$ @$' AS literal /* ?? #abc :$ @$ */"),vec![7i64.into()])?.get().await?;
                assert_eq!(rows[0].try_get::<i64>("","value").map_err(Error::internal)?,7);
                assert_eq!(rows[0].try_get::<String>("","literal").map_err(Error::internal)?,"?? #abc :$ @$");
                let rows=db.query().select_raw(RawSql::new("? AS value # ?? :$ @$ #abc\n"),vec![8i64.into()])?.get().await?;
                assert_eq!(rows[0].try_get::<i64>("","value").map_err(Error::internal)?,8);
            }
            DbBackend::Postgres=>{
                for (key,expected) in [("key",true),("missing",false)] {
                    let rows=db.query().select_raw(RawSql::new("?::jsonb ?? ? AS present"),vec![r#"{"key":1}"#.into(),key.into()])?.get().await?;
                    assert_eq!(rows[0].try_get::<bool>("","present").map_err(Error::internal)?,expected);
                }
                let rows=db.query().select_raw(RawSql::new("?::integer # ?::integer AS value"),vec![7.into(),3.into()])?.get().await?;
                assert_eq!(rows[0].try_get::<i32>("","value").map_err(Error::internal)?,4);
            }
            _=>return Err(unsupported_backend()),
        }
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

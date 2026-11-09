//! 验证公开 raw where 读取、写入的绑定组与错误传播。

use super::{
    Alias, Db, DbBackend, Error, Expr, MysqlQueryBuilder, PostgresQueryBuilder, Query, RawSql,
    Result, SqliteQueryBuilder, Value, factory::raw_expression,
};
use sea_orm::{ConnectOptions, Database};

async fn planning_database(url: &str) -> Result<Db> {
    let mut options = ConnectOptions::new(url);
    options.connect_lazy(true).min_connections(0);
    let db = Db::default();
    db.initialize(Database::connect(options).await.map_err(Error::internal)?)?;
    Ok(db)
}

#[tokio::test]
async fn where_raw_binds_between_other_where_terms_in_select_update_and_delete() -> Result<()> {
    // 惰性连接只提供驱动方言。此测试调用生产构造和编译路径，不访问数据库服务。
    for (url, backend) in [
        #[cfg(feature = "db-mysql")]
        ("mysql://fixture@127.0.0.1:1/openrust", DbBackend::MySql),
        #[cfg(feature = "db-postgres")]
        (
            "postgres://fixture@127.0.0.1:1/openrust",
            DbBackend::Postgres,
        ),
    ] {
        let db = planning_database(url).await?;
        let predicate = if backend == DbBackend::Postgres {
            "payload ?? ? AND '?' = '?' /* ? */"
        } else {
            "category = ? AND '?' = '?' /* ? */"
        };
        let needle = "中文 ' OR 1=1 -- ? $1";
        let query = db
            .table("records")?
            .select_raw(RawSql::new("? AS projected"), vec![91_i64.into()])?
            .where_eq("owner_id", 17_i64)?
            .where_raw(RawSql::new(predicate), vec![needle.into()])
            .where_eq("tag", "visible")?;
        let where_values = vec![17_i64.into(), needle.into(), "visible".into()];
        let mut select_values = vec![91_i64.into()];
        select_values.extend(where_values.clone());
        assert_eq!(query.get_bindings()?, select_values);
        let select = query.to_sql()?;
        let state = query.snapshot()?;
        let pairs = [(Alias::new("flag"), Expr::val(99_i64))];
        let update = state.compile_update(&pairs, false)?;
        let delete = state.compile_delete(false)?;
        assert_eq!(state.compile_update(&pairs, true)?, update);
        assert_eq!(state.compile_delete(true)?, delete);
        let mut update_values = vec![99_i64.into()];
        update_values.extend(where_values.clone());
        assert_eq!(update.1.0, update_values);
        assert_eq!(delete.1.0, where_values);
        for sql in [&select, &update.0, &delete.0] {
            assert!(sql.contains("'?' = '?' /* ? */"));
            assert!(!sql.contains(needle));
        }
        assert!(!update.0.contains("projected"));
        assert!(!delete.0.contains("projected"));
        if backend == DbBackend::Postgres {
            assert!(normalize_space(&select).contains("payload ? $3"));
            assert!(normalize_space(&update.0).contains("payload ? $3"));
            assert!(normalize_space(&delete.0).contains("payload ? $2"));
            assert!(select.contains(r#""tag" = $4"#));
            assert!(update.0.contains(r#""tag" = $4"#));
            assert!(delete.0.contains(r#""tag" = $3"#));
        }
    }
    Ok(())
}

const RAW_BOOLEAN_BACKENDS: &[(&str, DbBackend, char, &str, &str)] = &[
    #[cfg(feature = "db-mysql")]
    (
        "mysql://fixture@127.0.0.1:1/openrust",
        DbBackend::MySql,
        '`',
        "category = ?",
        "category =",
    ),
    #[cfg(feature = "db-postgres")]
    (
        "postgres://fixture@127.0.0.1:1/openrust",
        DbBackend::Postgres,
        '"',
        "payload ?? ?",
        "payload ?",
    ),
];

fn raw_bind(backend: DbBackend, index: usize) -> String {
    if backend == DbBackend::Postgres {
        format!("${index}")
    } else {
        "?".to_owned()
    }
}

#[tokio::test]
async fn raw_where_keeps_top_level_order_and_omits_the_first_or() -> Result<()> {
    for &(url, backend, quote, predicate, rendered) in RAW_BOOLEAN_BACKENDS {
        let db = planning_database(url).await?;
        let bind = |index| raw_bind(backend, index);
        let needle = "中文 ' OR 1=1 -- ? $1";
        let top = db
            .table("records")?
            .where_eq("owner_id", 17_i64)?
            .or_where_raw(
                RawSql::new(format!("{predicate} OR '?' = '?' /* ? */")),
                vec![needle.into()],
            )
            .where_eq("tag", "visible")?;
        assert_eq!(
            normalize_space(&top.to_sql()?),
            format!(
                "SELECT * FROM {quote}records{quote} WHERE ({quote}owner_id{quote} = {}) \
                 OR {rendered} {} OR '?' = '?' /* ? */ AND ({quote}tag{quote} = {})",
                bind(1),
                bind(2),
                bind(3),
            )
        );
        assert_eq!(
            top.get_bindings()?,
            vec![17_i64.into(), needle.into(), "visible".into()]
        );

        let first = db
            .table("records")?
            .where_nested(Ok)?
            .or_where_raw(RawSql::new("flag = ?"), vec![0_i64.into()]);
        assert_eq!(
            normalize_space(&first.to_sql()?),
            format!(
                "SELECT * FROM {quote}records{quote} WHERE flag = {}",
                bind(1)
            )
        );
        assert_eq!(first.get_bindings()?, vec![0_i64.into()]);
    }
    Ok(())
}

#[tokio::test]
async fn raw_where_boolean_precedence_and_nested_bindings_match_read_and_write_plans() -> Result<()>
{
    for &(url, backend, quote, predicate, rendered) in RAW_BOOLEAN_BACKENDS {
        let db = planning_database(url).await?;
        let bind = |index| raw_bind(backend, index);
        let needle = "中文 ' OR 1=1 -- ? $1";
        let nested = db
            .table("records")?
            .where_eq("owner_id", 17_i64)?
            .where_nested(|group| {
                group
                    .or_where_raw(RawSql::new(predicate), vec![needle.into()])?
                    .where_raw(RawSql::new("flag = ?"), vec![0_i64.into()])?
                    .or_where_raw(RawSql::new("tag = ?"), vec!["visible".into()])
            })?
            .where_eq("archived", 0_i64)?;
        let nested_where = |start| {
            let [owner, raw, flag, tag, archived] =
                std::array::from_fn(|index| bind(start + index));
            format!(
                "({quote}owner_id{quote} = {owner}) AND ({rendered} {raw} AND flag = {flag} \
                 OR tag = {tag} ) AND ({quote}archived{quote} = {archived})"
            )
        };
        let where_values = vec![
            17_i64.into(),
            needle.into(),
            0_i64.into(),
            "visible".into(),
            0_i64.into(),
        ];
        assert_eq!(nested.get_bindings()?, where_values);
        assert_eq!(
            normalize_space(&nested.to_sql()?),
            format!(
                "SELECT * FROM {quote}records{quote} WHERE {}",
                nested_where(1)
            )
        );

        let state = nested.snapshot()?;
        let pairs = [(Alias::new("flag"), Expr::val(99_i64))];
        let mut update_values = vec![99_i64.into()];
        update_values.extend(where_values.clone());
        for bindings in [false, true] {
            let (update, update_bindings) = state.compile_update(&pairs, bindings)?;
            assert_eq!(
                normalize_space(&update),
                format!(
                    "UPDATE {quote}records{quote} SET {quote}flag{quote} = {} WHERE {}",
                    bind(1),
                    nested_where(2)
                )
            );
            assert_eq!(update_bindings.0, update_values);

            let (delete, delete_bindings) = state.compile_delete(bindings)?;
            assert_eq!(
                normalize_space(&delete),
                format!(
                    "DELETE FROM {quote}records{quote} WHERE {}",
                    nested_where(1)
                )
            );
            assert_eq!(delete_bindings.0, where_values);
        }
    }
    Ok(())
}

#[tokio::test]
async fn raw_where_preserves_nested_comment_not_and_subquery_boundaries() -> Result<()> {
    for &(url, backend, quote, _, _) in RAW_BOOLEAN_BACKENDS {
        let db = planning_database(url).await?;
        let bind = |index| raw_bind(backend, index);
        let guarded = db
            .table("records")?
            .where_eq("owner_id", 17_i64)?
            .where_nested(|group| {
                group.where_raw(
                    RawSql::new("flag = ? OR flag = ? -- tail"),
                    vec![1_i64.into(), 2_i64.into()],
                )
            })?
            .where_null("archived")?;
        let sql = guarded.to_sql()?;
        assert!(sql.contains("-- tail\n) AND ("), "{sql}");
        assert_eq!(
            normalize_space(&sql),
            format!(
                "SELECT * FROM {quote}records{quote} WHERE ({quote}owner_id{quote} = {}) AND \
                 (flag = {} OR flag = {} -- tail ) AND ({quote}archived{quote} IS NULL)",
                bind(1),
                bind(2),
                bind(3)
            )
        );
        assert_eq!(
            guarded.get_bindings()?,
            vec![17_i64.into(), 1_i64.into(), 2_i64.into()]
        );

        let negated = db.table("records")?.where_none(
            [super::MultiColumn::callback(|query| {
                Ok(query.where_raw(
                    RawSql::new("flag = ? OR flag = ?"),
                    vec![1_i64.into(), 2_i64.into()],
                ))
            })],
            super::MultiComparison::Omitted,
        )?;
        let sql = normalize_space(&negated.to_sql()?);
        assert!(
            sql.contains(&format!(
                "NOT ((flag = {} OR flag = {} ))",
                bind(1),
                bind(2)
            )),
            "{sql}"
        );
        assert_eq!(negated.get_bindings()?, vec![1_i64.into(), 2_i64.into()]);

        let scalar = db
            .query()
            .select_raw(RawSql::new("?"), vec![9_i64.into()])?;
        let with_child = guarded.where_compare("flag", super::WhereOperator::Eq, &scalar)?;
        assert!(normalize_space(&with_child.to_sql()?).contains(&format!("SELECT {}", bind(4))));
        assert_eq!(
            with_child.get_bindings()?,
            vec![17_i64.into(), 1_i64.into(), 2_i64.into(), 9_i64.into()]
        );
    }
    Ok(())
}

fn normalize_space(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn raw_where_line_comment_cr_boundary_matches_each_backend() -> Result<()> {
    for (backend, marker) in [
        (DbBackend::MySql, "--"),
        (DbBackend::MySql, "#"),
        (DbBackend::Sqlite, "--"),
        (DbBackend::Postgres, "--"),
    ] {
        let sql = format!("first = ? {marker} hidden\r AND second = ?\n AND third = ?");
        let (values, expected_cr, expected_lf) = if backend == DbBackend::Postgres {
            (
                vec![11_i64.into(), 22_i64.into(), 33_i64.into()],
                "AND second = $2",
                "AND third = $3",
            )
        } else {
            (
                vec![11_i64.into(), 33_i64.into()],
                "AND second = ?",
                "AND third = ?",
            )
        };
        let mut query = Query::select();
        query
            .column(Alias::new("id"))
            .from(Alias::new("records"))
            .and_where(raw_expression(&sql, values.clone(), backend)?);
        let (compiled, bindings) = match backend {
            DbBackend::MySql => query.build(MysqlQueryBuilder),
            DbBackend::Sqlite => query.build(SqliteQueryBuilder),
            DbBackend::Postgres => query.build(PostgresQueryBuilder),
            _ => return Err(super::unsupported_backend()),
        };
        assert_eq!(bindings.0, values);
        let Some((before_cr, after_cr)) = compiled.split_once('\r') else {
            return Err(Error::custom("raw_test_missing_cr", "原始 SQL 丢失了 CR"));
        };
        assert!(before_cr.ends_with(&format!("{marker} hidden")));
        let Some((before_lf, after_lf)) = after_cr.split_once('\n') else {
            return Err(Error::custom("raw_test_missing_lf", "原始 SQL 丢失了 LF"));
        };
        assert_eq!(normalize_space(before_lf), expected_cr);
        assert!(normalize_space(after_lf).contains(expected_lf));
    }
    Ok(())
}

#[cfg(feature = "db-postgres")]
const POSTGRES_MODE_DEPENDENT_WHERE: &str = r"category = 'x\' || ? || ' OR tag = $1 -- '
AND flag = 1";

#[cfg(feature = "db-postgres")]
#[tokio::test]
async fn postgres_where_raw_rejects_ambiguous_quote_and_preserves_safe_regions() -> Result<()> {
    let db = planning_database("postgres://fixture@127.0.0.1:1/openrust").await?;
    for predicate in [
        POSTGRES_MODE_DEPENDENT_WHERE,
        r"category = 'x\\\' AND flag = ?",
    ] {
        let rejected = db
            .table("raw_where_modes")?
            .where_raw(RawSql::new(predicate), vec!["needle".into()]);
        assert_pg_raw_quote_ambiguous(rejected.get_bindings());
        assert_pg_raw_quote_ambiguous(rejected.to_sql());
        assert_pg_raw_quote_ambiguous(rejected.get().await);
        assert_pg_raw_quote_ambiguous(rejected.update(vec![("flag", 2_i64.into())]).await);
        assert_pg_raw_quote_ambiguous(rejected.delete().await);
    }
    for predicate in [
        r"category = 'x\\' AND flag = ?",
        r"category = $x$\' ? $1$x$ AND flag = ?",
        r"category = E'a' -- ? $1
        'b\'? $1' AND flag = ?",
    ] {
        let query = db
            .table("raw_where_modes")?
            .where_raw(RawSql::new(predicate), vec![1_i64.into()]);
        assert_eq!(query.get_bindings()?, [Value::from(1_i64)]);
        let sql = query.to_sql()?;
        let literal = predicate
            .strip_suffix(" AND flag = ?")
            .ok_or_else(|| Error::custom("query_pg_raw_fixture_invalid", "PG 原始条件夹具无效"))?;
        assert!(sql.contains(literal));
        assert!(normalize_space(&sql).contains("AND flag = $1"));
    }
    Ok(())
}

#[cfg(feature = "db-postgres")]
fn assert_pg_raw_quote_ambiguous<T>(result: Result<T>) {
    assert!(result.is_err_and(|error| error.code() == "query_raw_sql_mode_ambiguous"));
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "发布 PostgreSQL 实库验收：须配置 OPENRUST_TEST_DATABASE_URL 并显式运行"]
async fn real_postgres_raw_where_same_binding_mode_oracle() -> Result<()> {
    use crate::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
    use crate::testing::TestDatabase;
    use sea_orm::{ConnectionTrait as _, TransactionTrait as _};

    struct EmptyMigrator;
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    let url = crate::test_database_environment::database_url()
        .map_err(|_| Error::custom("query_pg_raw_fixture_missing", "需要 PostgreSQL 测试连接"))?;
    assert!(url.starts_with("postgres://") || url.starts_with("postgresql://"));
    let database = TestDatabase::create::<EmptyMigrator>(&url).await?;
    let connection = database.connection();
    let outcome = tokio::spawn(async move {
        let connection = connection?;
        let db = Db::default();
        db.initialize(connection.clone())?;
        connection
            .execute_unprepared(
                "CREATE TABLE raw_where_modes (id BIGINT PRIMARY KEY, category TEXT NOT NULL, \
                 tag TEXT NOT NULL, flag BIGINT NOT NULL)",
            )
            .await
            .map_err(Error::internal)?;
        for (id, category, tag) in [
            (1_i64, r"x\needle OR tag = $1 -- ", "other"),
            (2, "different", "needle"),
        ] {
            db.table("raw_where_modes")?
                .insert(vec![
                    ("id", id.into()),
                    ("category", category.into()),
                    ("tag", tag.into()),
                    ("flag", 1_i64.into()),
                ])
                .await?;
        }
        for (mode, expected) in [("on", 1_i64), ("off", 2_i64)] {
            let tx = connection.begin().await.map_err(Error::internal)?;
            tx.execute_unprepared(&format!("SET LOCAL standard_conforming_strings = {mode}"))
                .await
                .map_err(Error::internal)?;
            tx.execute_unprepared("SET LOCAL backslash_quote = on")
                .await
                .map_err(Error::internal)?;
            assert_pg_native_mode_target(&tx, mode, expected).await?;
            let rejected = db
                .table("raw_where_modes")?
                .using_transaction(&tx)
                .where_raw(
                    RawSql::new(POSTGRES_MODE_DEPENDENT_WHERE),
                    vec!["needle".into()],
                );
            assert_pg_raw_quote_ambiguous(rejected.update(vec![("flag", 2_i64.into())]).await);
            assert_pg_raw_quote_ambiguous(rejected.delete().await);
            let continued = db.query().using_transaction(&tx).select_raw(
                RawSql::new(
                    r"E'prefix' -- ? $1
            'suffix\'? $1' || ? AS value",
                ),
                vec!["bound".into()],
            )?;
            assert_eq!(
                continued.value::<String>("value").await?.as_deref(),
                Some("prefixsuffix'? $1bound")
            );
            drop(continued);
            drop(rejected);
            tx.rollback().await.map_err(Error::internal)?;
        }
        assert_eq!(
            db.table("raw_where_modes")?
                .where_eq("flag", 1_i64)?
                .count()
                .await?,
            2
        );
        Ok::<(), Error>(())
    })
    .await
    .map_err(Error::internal)
    .and_then(std::convert::identity);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-postgres")]
async fn assert_pg_native_mode_target(
    tx: &sea_orm::DatabaseTransaction,
    mode: &str,
    expected: i64,
) -> Result<()> {
    use sea_orm::{ConnectionTrait as _, Statement};

    // 区分 SQLx prepared cache 项，确保每个会话实际按当前设置重新解析。
    let native_sql = format!(
        "/* mode {mode} */ SELECT id FROM raw_where_modes WHERE {} ORDER BY id",
        POSTGRES_MODE_DEPENDENT_WHERE.replacen('?', "$1", 1),
    );
    let rows = tx
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            native_sql,
            [Value::from("needle")],
        ))
        .await
        .map_err(Error::internal)?;
    let [row] = rows.as_slice() else {
        return Err(Error::custom(
            "query_pg_raw_oracle_row_count",
            "PG 模式对照应只返回一行",
        ));
    };
    assert_eq!(
        row.try_get::<i64>("", "id").map_err(Error::internal)?,
        expected
    );
    Ok(())
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
async fn postgres_raw_where_compiles_pivot_placeholders_and_byte_comparison() -> Result<()> {
    let db = planning_database("postgres://fixture@127.0.0.1:1/openrust").await?;
    let query = db
        .table("roles")?
        .where_raw(
            RawSql::new(r#""role_user"."user_id" IN (?, ?)"#),
            vec![17_i64.into(), 33_i64.into()],
        )
        .where_raw(
            RawSql::new(r#""role_user"."label" = ?"#),
            vec!["active".into()],
        )
        .where_raw(
            RawSql::new(r#"convert_to("role_user"."secret", 'UTF8') = convert_to(?, 'UTF8')"#),
            vec!["encoded".into()],
        );
    assert_eq!(
        query.get_bindings()?,
        vec![
            17_i64.into(),
            33_i64.into(),
            "active".into(),
            "encoded".into()
        ]
    );
    let sql = normalize_space(&query.to_sql()?);
    assert!(sql.contains(r#""role_user"."user_id" IN ( $1 , $2 )"#));
    assert!(sql.contains(r#""role_user"."label" = $3"#));
    assert!(sql.contains("convert_to( $4 , 'UTF8')"));
    Ok(())
}

#[tokio::test]
async fn invalid_where_raw_bindings_fail_before_any_read_or_write_execution() -> Result<()> {
    let url = if cfg!(feature = "db-postgres") {
        "postgres://fixture@127.0.0.1:1/openrust"
    } else {
        "mysql://fixture@127.0.0.1:1/openrust"
    };
    let db = planning_database(url).await?;
    let guarded = db.table("records")?.where_eq("owner_id", 17_i64)?;
    for (sql, bindings) in [
        ("category = ? AND id = ?", vec![1_i64.into()]),
        ("category = ?", vec![1_i64.into(), 2_i64.into()]),
    ] {
        for invalid in [
            guarded
                .clone()
                .where_raw(RawSql::new(sql), bindings.clone()),
            guarded
                .clone()
                .or_where_raw(RawSql::new(sql), bindings.clone()),
        ] {
            let invalid = invalid.where_eq("tag", "visible")?;
            assert_invalid_bindings(invalid.get_bindings());
            assert_invalid_bindings(invalid.to_sql());
            assert_invalid_bindings(invalid.get().await);
            assert_invalid_bindings(invalid.count().await);
            assert_invalid_bindings(invalid.update(vec![("flag", 99_i64.into())]).await);
            assert_invalid_bindings(invalid.delete().await);
        }
        for or in [false, true] {
            let invalid = guarded.alias().where_nested(|group| {
                let group = group.where_eq("tag", "visible")?;
                if or {
                    group.or_where_raw(RawSql::new(sql), bindings.clone())
                } else {
                    group.where_raw(RawSql::new(sql), bindings.clone())
                }
            });
            assert_invalid_bindings(invalid);
        }
    }
    assert_eq!(guarded.get_bindings()?, vec![Value::from(17_i64)]);
    #[cfg(feature = "db-postgres")]
    {
        let invalid = guarded.where_raw(RawSql::new("category = $1"), vec![1_i64.into()]);
        assert!(
            invalid
                .delete()
                .await
                .is_err_and(|error| { error.code() == "query_raw_native_bindings_unsupported" })
        );
    }
    Ok(())
}

fn assert_invalid_bindings<T>(result: Result<T>) {
    assert_eq!(
        result.err().as_ref().map(Error::code),
        Some("query_raw_bindings_invalid")
    );
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
async fn real_mysql_where_raw_targets_only_the_bound_owner_and_text() -> Result<()> {
    use crate::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
    use crate::testing::TestDatabase;
    use sea_orm::ConnectionTrait as _;

    struct EmptyMigrator;
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    let Ok(url) = crate::test_database_environment::database_url() else {
        eprintln!("跳过真实 MySQL raw where：未设置 OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    if !url.starts_with("mysql://") {
        eprintln!("跳过真实 MySQL raw where：需要 MySQL 测试连接");
        return Ok(());
    }
    let database = TestDatabase::create::<EmptyMigrator>(&url).await?;
    let connection = database.connection();
    let outcome = tokio::spawn(async move {
        let db = Db::default();
        db.initialize(connection?)?;
        db.primary_connection()?
            .execute_unprepared(
                "CREATE TABLE raw_where_records (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
                 category VARCHAR(191) NOT NULL, tag VARCHAR(191) NOT NULL, flag BIGINT NOT NULL)",
            )
            .await
            .map_err(Error::internal)?;
        let table = db.table("raw_where_records")?;
        let needle = "中文 ' OR 1=1 -- ? $1";
        for (id, owner, category, tag) in [
            (1_i64, 17_i64, needle, "visible"),
            (2, 18, needle, "visible"),
            (3, 17, needle, "hidden"),
            (4, 17, "other", "visible"),
        ] {
            table
                .insert(vec![
                    ("id", id.into()),
                    ("owner_id", owner.into()),
                    ("category", category.into()),
                    ("tag", tag.into()),
                    ("flag", 0_i64.into()),
                ])
                .await?;
        }
        let guarded = table.clone().where_eq("owner_id", 17_i64)?;
        let invalid = guarded
            .clone()
            .where_raw(RawSql::new("category = ?"), Vec::new());
        assert_invalid_bindings(invalid.delete().await);
        assert_eq!(table.count().await?, 4);
        assert_mysql_raw_where_line_comments(&guarded, needle).await?;
        assert_mysql_raw_where_boolean(&db, needle).await?;
        assert_mysql_compound_raw_where(&db, needle).await?;
        let query = guarded
            .where_raw(
                RawSql::new("category = ? AND '?' = '?' /* ? */"),
                vec![needle.into()],
            )
            .where_eq("tag", "visible")?;
        assert_eq!(query.pluck::<i64>("id").await?, [1]);
        assert_eq!(query.count().await?, 1);
        assert_mysql_raw_where_rollback(&db, needle).await?;
        assert_eq!(query.update(vec![("flag", 99_i64.into())]).await?, 1);
        assert_eq!(
            table
                .clone()
                .where_eq("flag", 99_i64)?
                .pluck::<i64>("id")
                .await?,
            [1]
        );
        assert_eq!(query.delete().await?, 1);
        assert_eq!(
            table
                .order_by("id", super::SortDirection::Asc)?
                .pluck::<i64>("id")
                .await?,
            [2, 3, 4]
        );
        Ok::<(), Error>(())
    })
    .await
    .map_err(Error::internal)
    .and_then(std::convert::identity);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-mysql")]
async fn assert_mysql_raw_where_line_comments(
    guarded: &super::QueryBuilder<'_, '_>,
    needle: &str,
) -> Result<()> {
    for marker in ["--", "#"] {
        let commented = guarded.clone().where_raw(
            RawSql::new(format!(
                "category = ? {marker} hidden\r AND flag = ?\n AND tag = ?"
            )),
            vec![needle.into(), "visible".into()],
        );
        assert_eq!(
            commented.get_bindings()?,
            vec![
                Value::from(17_i64),
                Value::from(needle),
                Value::from("visible")
            ]
        );
        assert_eq!(commented.pluck::<i64>("id").await?, [1]);
        assert_eq!(commented.count().await?, 1);
    }
    Ok(())
}

#[cfg(feature = "db-mysql")]
async fn assert_mysql_raw_where_boolean(db: &Db, needle: &str) -> Result<()> {
    use sea_orm::TransactionTrait as _;

    let tx = db
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let table = db.table("raw_where_records")?.using_transaction(&tx);
    let top = table
        .clone()
        .where_eq("owner_id", 18_i64)?
        .or_where_raw(RawSql::new("category = ?"), vec![needle.into()])
        .where_eq("tag", "hidden")?
        .order_by("id", super::SortDirection::Asc)?;
    assert_eq!(top.pluck::<i64>("id").await?, [2, 3]);
    assert_eq!(top.count().await?, 2);

    let scoped = table
        .clone()
        .where_eq("owner_id", 17_i64)?
        .where_nested(|group| {
            group
                .where_raw(RawSql::new("category = ?"), vec![needle.into()])?
                .or_where_raw(RawSql::new("category = ?"), vec!["other".into()])
        })?
        .where_eq("tag", "visible")?;
    assert_eq!(
        scoped
            .clone()
            .order_by("id", super::SortDirection::Asc)?
            .pluck::<i64>("id")
            .await?,
        [1, 4]
    );
    assert_eq!(scoped.count().await?, 2);
    assert_eq!(scoped.update(vec![("flag", 77_i64.into())]).await?, 2);
    assert_eq!(
        table
            .clone()
            .where_eq("flag", 77_i64)?
            .order_by("id", super::SortDirection::Asc)?
            .pluck::<i64>("id")
            .await?,
        [1, 4]
    );
    assert_eq!(scoped.delete().await?, 2);
    assert_eq!(
        table
            .order_by("id", super::SortDirection::Asc)?
            .pluck::<i64>("id")
            .await?,
        [2, 3]
    );
    drop(scoped);
    drop(top);
    tx.rollback().await.map_err(Error::internal)?;
    assert_eq!(
        db.table("raw_where_records")?
            .where_eq("flag", 0_i64)?
            .count()
            .await?,
        4
    );
    Ok(())
}

#[cfg(feature = "db-mysql")]
async fn assert_mysql_compound_raw_where(db: &Db, needle: &str) -> Result<()> {
    use sea_orm::TransactionTrait as _;

    let tx = db
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let table = db.table("raw_where_records")?.using_transaction(&tx);
    for marker in ["--", "#"] {
        let blocked = table
            .clone()
            .where_raw(
                RawSql::new(format!("id = ? {marker} tail")),
                vec![1_i64.into()],
            )
            .where_null("tag")?;
        assert_eq!(blocked.count().await?, 0);
        assert_eq!(blocked.update(vec![("flag", 66_i64.into())]).await?, 0);
        assert_eq!(blocked.delete().await?, 0);
    }

    let fragment = RawSql::new("category = ? OR id = ?");
    let bindings = vec![needle.into(), 2_i64.into()];
    let owner = table.clone().where_eq("owner_id", 17_i64)?;
    let ungrouped = owner
        .clone()
        .where_raw(fragment.clone(), bindings.clone())
        .where_eq("tag", "visible")?;
    let grouped = owner
        .where_nested(|group| group.where_raw(fragment, bindings))?
        .where_eq("tag", "visible")?;
    assert_eq!(
        ungrouped
            .clone()
            .order_by("id", super::SortDirection::Asc)?
            .pluck::<i64>("id")
            .await?,
        [1, 2, 3]
    );
    assert_eq!(grouped.pluck::<i64>("id").await?, [1]);

    assert_eq!(grouped.update(vec![("flag", 77_i64.into())]).await?, 1);
    assert_eq!(
        table
            .clone()
            .where_eq("flag", 77_i64)?
            .pluck::<i64>("id")
            .await?,
        [1]
    );
    assert_eq!(ungrouped.update(vec![("flag", 88_i64.into())]).await?, 3);
    assert_eq!(
        table
            .clone()
            .where_eq("flag", 88_i64)?
            .order_by("id", super::SortDirection::Asc)?
            .pluck::<i64>("id")
            .await?,
        [1, 2, 3]
    );
    assert_eq!(grouped.delete().await?, 1);
    assert_eq!(ungrouped.delete().await?, 2);
    assert_eq!(table.pluck::<i64>("id").await?, [4]);
    drop(grouped);
    drop(ungrouped);
    drop(table);
    tx.rollback().await.map_err(Error::internal)?;
    assert_eq!(
        db.table("raw_where_records")?
            .where_eq("flag", 0_i64)?
            .count()
            .await?,
        4
    );
    Ok(())
}

#[cfg(feature = "db-mysql")]
async fn assert_mysql_raw_where_rollback(db: &Db, needle: &str) -> Result<()> {
    use sea_orm::TransactionTrait as _;

    let tx = db
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let in_tx = db.table("raw_where_records")?.using_transaction(&tx);
    let query = in_tx
        .clone()
        .where_eq("owner_id", 17_i64)?
        .where_raw(RawSql::new("category = ?"), vec![needle.into()])
        .where_eq("tag", "visible")?;
    assert_eq!(query.update(vec![("flag", 42_i64.into())]).await?, 1);
    assert_eq!(
        in_tx
            .clone()
            .where_eq("flag", 42_i64)?
            .pluck::<i64>("id")
            .await?,
        [1]
    );
    assert_eq!(query.delete().await?, 1);
    assert_eq!(in_tx.count().await?, 3);
    drop(query);
    drop(in_tx);
    tx.rollback().await.map_err(Error::internal)?;
    let table = db.table("raw_where_records")?;
    assert_eq!(table.clone().where_eq("flag", 42_i64)?.count().await?, 0);
    assert_eq!(table.count().await?, 4);
    Ok(())
}

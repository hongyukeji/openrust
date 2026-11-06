//! 可发现普通比较NULL误绑定、操作符方言误编译、右标量来源丢失及旧条件优先级回归。

use super::*;
#[cfg(feature = "db-sqlite")]
use crate::database::Db;
use sea_orm::sea_query::{
    Alias, Asterisk, MysqlQueryBuilder, PostgresQueryBuilder, Query, SqliteQueryBuilder,
};
#[cfg(feature = "db-sqlite")]
use sea_orm::{ConnectionTrait, Database, TransactionTrait};
#[cfg(feature = "db-sqlite")]
use std::sync::Arc;

#[cfg(feature = "db-sqlite")]
async fn database() -> Result<Db> {
    let connection = Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection.execute_unprepared("CREATE TABLE samples (id INTEGER PRIMARY KEY,value INTEGER,label TEXT,document TEXT); INSERT INTO samples VALUES(1,10,'Alpha','{\"active\":true}'),(2,20,'Beta','{\"active\":false}'),(3,NULL,NULL,'{}')").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    Ok(db)
}

#[test]
fn known_operators_parse_without_accepting_sql_injection() -> Result<()> {
    assert_eq!("NOT LIKE".parse::<WhereOperator>()?, WhereOperator::NotLike);
    assert_eq!("?&".parse::<WhereOperator>()?, WhereOperator::HasAllKeys);
    assert!(
        matches!("= ?; DROP TABLE samples".parse::<WhereOperator>(), Err(error) if error.code()=="query_operator_invalid")
    );
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn null_comparisons_default_eq_or_precedence_and_write_share_predicate() -> Result<()> {
    let db = database().await?;
    assert_eq!(
        db.table("samples")?
            .r#where("value", Option::<i64>::None)?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("samples")?
            .where_compare("value", WhereOperator::NotEq, Option::<i64>::None)?
            .count()
            .await?,
        2
    );
    assert!(
        matches!(db.table("samples")?.where_compare("value",WhereOperator::Gt,Option::<i64>::None),Err(error) if error.code()=="query_operator_null_invalid")
    );
    let query = db
        .table("samples")?
        .r#where("id", 1)?
        .or_where_value("id", 2)?
        .where_compare("value", WhereOperator::Gt, 15)?;
    assert_eq!(query.count().await?, 2);
    assert_eq!(query.update(vec![("label", "matched".into())]).await?, 2);
    assert_eq!(
        db.table("samples")?
            .r#where("label", "matched")?
            .count()
            .await?,
        2
    );
    assert_eq!(
        db.table("samples")?
            .where_nested(|group| group.r#where("id", 1)?.or_where_compare(
                "id",
                WhereOperator::Eq,
                2
            ))?
            .delete()
            .await?,
        2
    );
    assert_eq!(db.table("samples")?.count().await?, 1);
    Ok(())
}

#[test]
fn dialects_compile_nullsafe_like_and_postgres_bitwise_with_bound_values() -> Result<()> {
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        for (operator, expected) in [
            (
                WhereOperator::NullSafeEq,
                match backend {
                    DbBackend::MySql => "<=>",
                    DbBackend::Postgres => "is not distinct from",
                    DbBackend::Sqlite => "is",
                    _ => return Err(unsupported_backend()),
                },
            ),
            (WhereOperator::Like, "like"),
        ] {
            let expression = comparison_expr(
                Expr::col(Alias::new("value")),
                operator,
                Expr::val(7),
                backend,
            )?;
            let select = Query::select()
                .column(Asterisk)
                .from(Alias::new("samples"))
                .and_where(expression)
                .to_owned();
            let (sql, values) = match backend {
                DbBackend::MySql => select.build(MysqlQueryBuilder),
                DbBackend::Postgres => select.build(PostgresQueryBuilder),
                DbBackend::Sqlite => select.build(SqliteQueryBuilder),
                _ => return Err(unsupported_backend()),
            };
            assert!(sql.contains(expected));
            assert_eq!(values.0, vec![Value::from(7)]);
            if backend == DbBackend::Postgres && operator == WhereOperator::Like {
                assert!(sql.contains("::text"));
            }
        }
    }
    let expression = comparison_expr(
        Expr::col(Alias::new("value")),
        WhereOperator::BitAnd,
        Expr::val(1),
        DbBackend::Postgres,
    )?;
    let (sql, values) = Query::select().expr(expression).build(PostgresQueryBuilder);
    assert!(sql.contains("::bool"));
    assert_eq!(values.0, vec![Value::from(1)]);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn scalar_operands_keep_callbacks_bindings_and_reject_context_changes() -> Result<()> {
    let db = database().await?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let scalar = db
        .table("samples")?
        .select(&["value"])?
        .r#where("id", 1)?
        .before_query(move |_| {
            callback_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
    let query = db
        .table("samples")?
        .where_compare(&scalar, WhereOperator::Lt, RawSql::new("15"))?
        .where_compare("value", WhereOperator::Eq, &scalar)?;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(query.get_bindings()?, vec![Value::from(1), Value::from(1)]);
    assert_eq!(query.count().await?, 1);
    let other = database().await?;
    assert!(
        matches!(other.table("samples")?.where_compare("value",WhereOperator::Eq,&scalar),Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    let connection = db.primary_connection()?;
    let transaction = connection.begin().await.map_err(Error::internal)?;
    let rebound = query.using_transaction(&transaction);
    assert!(
        matches!(rebound.count().await,Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    drop(rebound);
    transaction.rollback().await.map_err(Error::internal)?;
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn json_boolean_null_strings_and_array_paths_share_safe_bound_comparisons() -> Result<()> {
    let db = database().await?;
    assert_eq!(
        db.table("samples")?
            .r#where("document->active", true)?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("samples")?
            .r#where("document->active", false)?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("samples")?
            .r#where("document->active", Option::<i64>::None)?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("samples")?
            .r#where("document->active", true)?
            .get_bindings()?,
        [] as [Value; 0]
    );
    let context = db.table("samples")?.snapshot()?.context();
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        for (selector, boolean, null) in [
            ("samples.document->nested->items[0]->name", false, false),
            ("document->active", true, false),
            ("document->missing", false, true),
            ("document->a'b", false, false),
            ("document->a\\'b", false, false),
            ("document->a\"b", false, false),
        ] {
            let expression = json_selector(&context, selector, backend, boolean, null)?;
            let statement = Query::select().expr(expression).to_owned();
            let (sql, values) = match backend {
                DbBackend::MySql => statement.build(MysqlQueryBuilder),
                DbBackend::Postgres => statement.build(PostgresQueryBuilder),
                DbBackend::Sqlite => statement.build(SqliteQueryBuilder),
                _ => return Err(unsupported_backend()),
            };
            assert_eq!(values.0, [] as [Value; 0]);
            assert!(!sql.contains("$1"));
            if backend == DbBackend::Postgres {
                assert!(sql.contains("->"));
                if boolean {
                    assert!(sql.contains("::jsonb"));
                }
            } else {
                assert!(sql.contains("json_extract("));
            }
            if backend == DbBackend::Postgres && selector.contains("a\\'b") {
                assert!(sql.contains(&sql_path_quote(
                    selector.rsplit("->").next().ok_or_else(invalid_operator)?
                )));
            } else if selector.contains("a'b") || selector.contains("a\\'b") {
                assert!(sql.contains("a''b"));
            }
            if backend == DbBackend::Postgres && selector.contains("items[0]") {
                assert!(sql.contains("->0->>"));
            }
        }
    }
    Ok(())
}

#[test]
fn json_index_integer_classification_matches_php_filter_not_rust_parse_alone() {
    for value in [
        "0",
        "-0",
        "+0",
        "1",
        "-1",
        "+1",
        " 1 ",
        "9223372036854775807",
        "-9223372036854775808",
    ] {
        assert!(pg_integer_attribute(value), "{value}");
    }
    for value in [
        "01",
        "-01",
        "+01",
        "00",
        "1e2",
        "9223372036854775808",
        "-9223372036854775809",
        "١",
    ] {
        assert!(!pg_integer_attribute(value), "{value}");
    }
    assert_eq!(path_segment("items[0][1]"), ("items", vec!["0", "1"]));
    assert_eq!(path_segment("items[]"), ("items[]", Vec::<&str>::new()));
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn raw_json_expression_boolean_is_expression_not_a_value_binding() -> Result<()> {
    let db = database().await?;
    let query = db
        .table("samples")?
        .r#where(RawSql::new("document->'active'"), true)?;
    assert_eq!(query.get_bindings()?, [] as [Value; 0]);
    assert!(query.to_sql()?.contains("true"));
    assert_eq!(path_segment("key[a[b]"), ("key", vec!["a[b"]));
    assert_eq!(path_segment("key[][0]"), ("key[]", vec!["0"]));
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn operator_null_rules_and_nested_scalar_provenance_apply_to_writes() -> Result<()> {
    let db = database().await?;
    for operator in [WhereOperator::Eq, WhereOperator::NullSafeEq] {
        assert_eq!(
            db.table("samples")?
                .where_compare("value", operator, Option::<i64>::None)?
                .count()
                .await?,
            1
        );
    }
    for operator in [WhereOperator::NotEq, WhereOperator::NotEqBang] {
        assert_eq!(
            db.table("samples")?
                .where_compare("value", operator, Option::<i64>::None)?
                .count()
                .await?,
            2
        );
    }
    for operator in [
        WhereOperator::Is,
        WhereOperator::IsNot,
        WhereOperator::Like,
        WhereOperator::BitAnd,
    ] {
        assert!(
            matches!(db.table("samples")?.where_compare("value",operator,Option::<i64>::None),Err(error) if error.code()=="query_operator_null_invalid")
        );
    }
    let scalar = db.table("samples")?.select(&["value"])?.r#where("id", 1)?;
    let query = db
        .table("samples")?
        .where_nested(|group| group.where_compare("value", WhereOperator::Eq, &scalar))?;
    assert_eq!(query.count().await?, 1);
    let connection = db.primary_connection()?;
    let transaction = connection.begin().await.map_err(Error::internal)?;
    let scoped_scalar = db
        .table("samples")?
        .using_transaction(&transaction)
        .select(&["value"])?
        .r#where("id", 1)?;
    assert!(
        matches!(db.table("samples")?.where_compare("value",WhereOperator::Eq,&scoped_scalar),Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    let rebound = query.using_transaction(&transaction);
    assert!(
        matches!(rebound.update(vec![("value",99.into())]).await,Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.delete().await,Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    let wrapped = db
        .table("samples")?
        .using_transaction(&transaction)
        .where_exists(&rebound);
    assert!(matches!(&wrapped, Err(error) if error.code() == "query_subquery_context_mismatch"));
    drop(wrapped);
    drop(rebound);
    drop(scoped_scalar);
    transaction.rollback().await.map_err(Error::internal)?;
    assert_eq!(db.table("samples")?.r#where("value", 10)?.count().await?, 1);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn rhs_subquery_retains_live_sql_frozen_bindings_and_does_not_consume_callbacks() -> Result<()>
{
    let db = database().await?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = Arc::clone(&calls);
    let rhs = db
        .table("samples")?
        .select(&["value"])?
        .where_not_null("value")?
        .limit(1)
        .before_query(move |_| {
            captured.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
    let parent = db
        .table("samples")?
        .where_compare("value", WhereOperator::Eq, &rhs)?;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let rhs = rhs.order_by("id", super::super::SortDirection::Desc)?;
    let rows = parent.get().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].try_get::<i64>("", "id").map_err(Error::internal)?,
        2
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    rhs.to_sql()?;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn lhs_rhs_and_following_bindings_keep_distinct_declared_order() -> Result<()> {
    let db = database().await?;
    let lhs = db.table("samples")?.select(&["value"])?.r#where("id", 1)?;
    let rhs = db.table("samples")?.select(&["value"])?.r#where("id", 2)?;
    let parent = db
        .table("samples")?
        .where_compare(&lhs, WhereOperator::Lt, &rhs)?
        .r#where("id", 3)?;
    assert_eq!(
        parent.get_bindings()?,
        vec![Value::from(1), Value::from(2), Value::from(3)]
    );
    assert_eq!(parent.count().await?, 1);
    let cyclic = db.table("samples")?;
    assert!(
        matches!(cyclic.alias().where_compare("value",WhereOperator::Eq,&cyclic),Err(error) if error.code()=="query_subquery_cycle")
    );
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn backend_only_null_operator_is_rejected_before_any_write() -> Result<()> {
    let db = database().await?;
    for operator in [
        WhereOperator::SoundsLike,
        WhereOperator::DistinctFrom,
        WhereOperator::HasKey,
    ] {
        let attempted = db
            .table("samples")?
            .where_compare("value", operator, Option::<i64>::None);
        assert!(matches!(attempted,Err(error) if error.code()=="query_operator_invalid"));
    }
    assert_eq!(db.table("samples")?.count().await?, 3);
    assert!(validate_operator_backend(WhereOperator::SoundsLike, DbBackend::MySql).is_ok());
    assert!(validate_operator_backend(WhereOperator::DistinctFrom, DbBackend::Postgres).is_ok());
    assert!(validate_operator_backend(WhereOperator::DistinctFrom, DbBackend::MySql).is_err());
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn json_root_zero_array_key_and_formfeed_follow_php_categories() -> Result<()> {
    let db = database().await?;
    db.table("samples")?
        .r#where("id", 1)?
        .update(vec![("document", r#"["root"]"#.into())])
        .await?;
    db.table("samples")?
        .r#where("id", 2)?
        .update(vec![("document", r#"{"0":["nested"]}"#.into())])
        .await?;
    assert_eq!(
        db.table("samples")?
            .r#where("document->0[0]", "root")?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("samples")?
            .r#where("document->0[0]", "nested")?
            .count()
            .await?,
        0
    );
    assert!(!pg_integer_attribute("\u{c}1"));
    assert!(pg_integer_attribute("\u{b}1"));
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn lhs_scalar_sql_with_json_arrow_keeps_boolean_as_unbound_literal() -> Result<()> {
    let db = database().await?;
    let lhs = db
        .table("samples")?
        .select(&["value"])?
        .r#where("id", 1)?
        // 3.26 尚无 JSON 箭头操作符；保留 SQL 内箭头文本，验证标量来源不被误判为 JSON 列。
        .where_raw(
            RawSql::new("json_extract(document, '$.active') is not null /* document->'active' */"),
            vec![],
        );
    let query = db.table("samples")?.r#where(&lhs, true)?;
    assert_eq!(query.get_bindings()?, vec![Value::from(1)]);
    assert!(query.to_sql()?.contains("document->'active'"));
    assert!(query.to_sql()?.contains("true"));
    assert_eq!(query.count().await?, 0);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn invalid_null_comparison_does_not_prepare_or_consume_lhs_callbacks() -> Result<()> {
    let db = database().await?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let lhs = db
        .table("samples")?
        .select(&["value"])?
        .r#where("id", 1)?
        .before_query(move |_| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
    assert!(
        matches!(db.table("samples")?.where_compare(&lhs,WhereOperator::Gt,Option::<i64>::None),Err(error) if error.code()=="query_operator_null_invalid")
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    lhs.to_sql()?;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn rhs_bindings_are_captured_after_lhs_query_callback() -> Result<()> {
    let db = database().await?;
    let rhs = db.table("samples")?.select(&["value"])?;
    let rhs_alias = rhs.alias();
    let lhs = db
        .table("samples")?
        .select(&["value"])?
        .r#where("id", 1)?
        .before_query(move |_| {
            rhs_alias.alias().r#where("id", 2)?;
            Ok(())
        });
    let parent = db
        .table("samples")?
        .where_compare(&lhs, WhereOperator::Lt, &rhs)?;
    assert_eq!(parent.get_bindings()?, vec![Value::from(1), Value::from(2)]);
    assert_eq!(parent.count().await?, 3);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn lhs_named_failure_precedes_cross_database_rhs_capture_and_validation() -> Result<()> {
    let db = database().await?;
    let other = database().await?;
    let lhs = db
        .table("samples")?
        .select(&["value"])?
        .before_query(|_| Err(Error::custom("left_failure", "左查询准备失败")));
    let rhs = other.table("samples")?.select(&["value"])?;
    assert!(
        matches!(db.table("samples")?.where_compare(&lhs,WhereOperator::Eq,&rhs),Err(error) if error.code()=="left_failure")
    );
    // 在同一跨库QB捕获工厂外计数，证明左错传播前没有开始取得右侧绑定快照。
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let value = WhereValue(ValueOperand::Sub(Box::new(move || {
        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        super::super::subqueries::DeferredChild::capture(&rhs, false)
    })));
    assert!(
        matches!(db.table("samples")?.where_compare(&lhs,WhereOperator::Eq,value),Err(error) if error.code()=="left_failure")
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

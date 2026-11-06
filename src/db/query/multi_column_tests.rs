//! 可发现多列NOT误写成逐列不等、闭包重复/顺序错误及空列提前执行leaf的缺陷。

use super::super::SortDirection;
use super::*;
use crate::db::Db;
use sea_orm::{ConnectionTrait, Database, TransactionTrait};
use std::sync::Arc;

async fn database() -> Result<Db> {
    let connection = Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection.execute_unprepared("CREATE TABLE samples (id INTEGER PRIMARY KEY,a INTEGER,b INTEGER,flag INTEGER); INSERT INTO samples VALUES(1,1,1,0),(2,1,2,1),(3,NULL,2,0),(4,NULL,NULL,1),(5,2,2,0)").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    Ok(db)
}
async fn ids(query: QueryBuilder<'_, '_>) -> Result<Vec<i64>> {
    query
        .order_by("id", SortDirection::Asc)?
        .get()
        .await?
        .into_iter()
        .map(|row| row.try_get("", "id").map_err(Error::internal))
        .collect()
}
#[tokio::test]
async fn six_methods_preserve_grouping_and_not_sql_three_valued_logic() -> Result<()> {
    let db = database().await?;
    assert_eq!(
        ids(db.table("samples")?.where_any(["a", "b"], 1)?).await?,
        [1, 2]
    );
    assert_eq!(
        ids(db.table("samples")?.where_all(["a", "b"], 1)?).await?,
        [1]
    );
    assert_eq!(
        ids(db.table("samples")?.where_none(["a", "b"], 1)?).await?,
        [5]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("flag", 1)?
            .or_where_any(["a", "b"], 1)?)
        .await?,
        [1, 2, 4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("flag", 1)?
            .or_where_all(["a", "b"], 1)?)
        .await?,
        [1, 2, 4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("flag", 1)?
            .or_where_none(["a", "b"], 1)?)
        .await?,
        [2, 4, 5]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .where_any(["a", "b"], Option::<i64>::None)?)
        .await?,
        [3, 4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .where_all(["a", "b"], Option::<i64>::None)?)
        .await?,
        [4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .where_none(["a", "b"], Option::<i64>::None)?)
        .await?,
        [1, 2, 5]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .where_any(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))?)
        .await?,
        [2, 3, 5]
    );
    Ok(())
}
#[tokio::test]
async fn mixed_columns_and_omitted_closures_keep_nested_from_and_scalar_empty_from() -> Result<()> {
    let db = database().await?;
    let nested = MultiColumn::callback(|q| {
        assert!(q.to_sql()?.contains("FROM"));
        q.r#where("a", 1)
    });
    assert_eq!(
        ids(db
            .table("samples")?
            .where_any([nested, MultiColumn::from("b")], MultiComparison::Omitted)?)
        .await?,
        [1, 2, 4]
    );
    let scalar = db.query().select_raw(RawSql::new("?"), vec![1.into()])?;
    let columns = [
        MultiColumn::from("a"),
        MultiColumn::from(RawSql::new("b")),
        MultiColumn::from(&scalar),
        MultiColumn::callback(|q| {
            assert!(!q.to_sql()?.contains("FROM"));
            q.select_raw(RawSql::new("?"), vec![1.into()])
        }),
    ];
    let query = db.table("samples")?.where_all(columns, 1)?;
    assert_eq!(
        query.get_bindings()?,
        vec![1.into(), 1.into(), 1.into(), 1.into(), 1.into(), 1.into()]
    );
    assert_eq!(ids(query).await?, [1]);
    Ok(())
}
#[tokio::test]
async fn rhs_callback_repeats_in_column_order_and_does_not_consume_before_callbacks() -> Result<()>
{
    let db = database().await?;
    let before = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&before);
    let mut sequence = 0i64;
    let value = MultiValue::callback(move |q| {
        sequence += 1;
        let observed = Arc::clone(&observed);
        q.select_raw(RawSql::new("?"), vec![sequence.into()])
            .map(|q| {
                q.before_query(move |_| {
                    observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
            })
    });
    let query = db
        .table("samples")?
        .where_all(["a", "b"], MultiComparison::eq(value))?;
    assert_eq!(query.get_bindings()?, vec![1i64.into(), 2i64.into()]);
    assert_eq!(ids(query).await?, [2]);
    assert_eq!(before.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}
#[tokio::test]
async fn empty_columns_only_validate_parameter_preparation_and_do_not_call_factories() -> Result<()>
{
    let db = database().await?;
    let empty: Vec<MultiColumn<'_>> = Vec::new();
    assert!(
        matches!(db.table("samples")?.where_any(empty,MultiComparison::compare(WhereOperator::Gt,Option::<i64>::None)),Err(error) if error.code()=="query_operator_null_invalid")
    );
    assert_eq!(
        db.table("samples")?
            .where_none(
                Vec::<MultiColumn<'_>>::new(),
                MultiComparison::compare(WhereOperator::DistinctFrom, Option::<i64>::None)
            )?
            .count()
            .await?,
        5
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let value = MultiValue::callback(move |q| {
        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(q)
    });
    assert_eq!(
        db.table("samples")?
            .where_all(Vec::<MultiColumn<'_>>::new(), MultiComparison::eq(value))?
            .count()
            .await?,
        5
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        db.table("samples")?
            .where_none([MultiColumn::callback(Ok)], MultiComparison::Omitted)?
            .count()
            .await?,
        5
    );
    Ok(())
}
#[tokio::test]
async fn explicit_null_closure_is_scalar_and_omitted_closure_never_runs_its_before_callback()
-> Result<()> {
    let db = database().await?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let column = MultiColumn::callback(move |q| {
        Ok(q.before_query(move |_| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }))
    });
    assert_eq!(
        db.table("samples")?
            .where_any([column], MultiComparison::Omitted)?
            .count()
            .await?,
        5
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let observed = Arc::clone(&calls);
    let column = MultiColumn::callback(move |q| {
        Ok(q.select_raw(RawSql::new("NULL"), vec![])?
            .before_query(move |_| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }))
    });
    assert_eq!(
        db.table("samples")?
            .where_all([column], MultiComparison::eq(Option::<i64>::None))?
            .count()
            .await?,
        5
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    Ok(())
}
#[tokio::test]
async fn grouped_reads_writes_and_transactions_share_the_same_predicate() -> Result<()> {
    let db = database().await?;
    let connection = db.primary_connection()?;
    let tx = connection.begin().await.map_err(Error::internal)?;
    let query = db
        .table("samples")?
        .using_transaction(&tx)
        .where_nested(|group| {
            group.where_any(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))
        })?;
    assert_eq!(query.count().await?, 3);
    assert!(query.exists().await?);
    assert_eq!(query.update(vec![("flag", 7.into())]).await?, 3);
    assert_eq!(
        db.table("samples")?
            .using_transaction(&tx)
            .r#where("flag", 7)?
            .delete()
            .await?,
        3
    );
    drop(query);
    tx.rollback().await.map_err(Error::internal)?;
    assert_eq!(db.table("samples")?.count().await?, 5);
    Ok(())
}

#[tokio::test]
async fn multi_callback_context_and_error_order_never_wash_source_or_run_later_rhs() -> Result<()> {
    let db = database().await?;
    let other = database().await?;
    let other_query = other.table("samples")?;
    let column = MultiColumn::callback(move |_| Ok(other_query));
    assert!(
        matches!(db.table("samples")?.where_any([column],MultiComparison::Omitted),Err(error) if error.code()=="query_nested_context_mismatch")
    );
    let column = MultiColumn::callback(|q| q.from("different"));
    assert!(
        matches!(db.table("samples")?.where_any([column],MultiComparison::Omitted),Err(error) if error.code()=="query_nested_context_mismatch")
    );
    let rhs_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&rhs_calls);
    let rhs = MultiValue::callback(move |q| {
        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(q)
    });
    let column = MultiColumn::callback(|q| {
        Ok(q.before_query(|_| Err(Error::custom("multi_left_failure", "左闭包准备失败"))))
    });
    assert!(
        matches!(db.table("samples")?.where_all([column],MultiComparison::eq(rhs)),Err(error) if error.code()=="multi_left_failure")
    );
    assert_eq!(rhs_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(db.table("samples")?.count().await?, 5);
    Ok(())
}

#[tokio::test]
async fn multi_rhs_live_sql_and_transaction_rebinding_keep_frozen_per_column_bindings() -> Result<()>
{
    let db = database().await?;
    let rhs = db
        .table("samples")?
        .select(&["a"])?
        .where_not_null("a")?
        .limit(1);
    let query = db
        .table("samples")?
        .where_any(["a", "b"], MultiComparison::eq(MultiValue::from(&rhs)))?;
    let rhs = rhs.order_by("id", SortDirection::Desc)?;
    assert_eq!(ids(query.clone()).await?, [2, 3, 5]);
    let captured = query.get_bindings()?;
    let rhs = rhs.limit(2);
    assert_eq!(query.get_bindings()?, captured);
    // LIMIT仍占同数目绑定，当前SQL来自live子节点，值沿每列接入时的计划。
    assert_eq!(ids(query.clone()).await?, [2, 3, 5]);
    let connection = db.primary_connection()?;
    let tx = connection.begin().await.map_err(Error::internal)?;
    let rebound = query.using_transaction(&tx);
    assert!(
        matches!(rebound.count().await,Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.update(vec![("flag",8.into())]).await,Err(error) if error.code()=="query_subquery_context_mismatch")
    );
    drop(rebound);
    drop(rhs);
    tx.rollback().await.map_err(Error::internal)?;
    Ok(())
}

#[tokio::test]
async fn all_six_explicit_operators_and_null_or_groups_have_independent_witnesses() -> Result<()> {
    let db = database().await?;
    assert_eq!(
        ids(db
            .table("samples")?
            .where_all(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))?)
        .await?,
        [5]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .where_none(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))?)
        .await?,
        [1]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("id", 4)?
            .or_where_any(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))?)
        .await?,
        [2, 3, 4, 5]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("id", 4)?
            .or_where_all(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))?)
        .await?,
        [4, 5]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("id", 4)?
            .or_where_none(["a", "b"], MultiComparison::compare(WhereOperator::Gt, 1))?)
        .await?,
        [1, 4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("id", 1)?
            .or_where_any(["a", "b"], Option::<i64>::None)?)
        .await?,
        [1, 3, 4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("id", 1)?
            .or_where_all(["a", "b"], Option::<i64>::None)?)
        .await?,
        [1, 4]
    );
    assert_eq!(
        ids(db
            .table("samples")?
            .r#where("id", 1)?
            .or_where_none(["a", "b"], Option::<i64>::None)?)
        .await?,
        [1, 2, 5]
    );
    Ok(())
}

#[tokio::test]
async fn group_failure_does_not_publish_half_a_group_and_new_live_edges_reject_cycles() -> Result<()>
{
    let db = database().await?;
    let query = db.table("samples")?;
    let original = query.to_sql()?;
    let columns = [
        MultiColumn::from("a"),
        MultiColumn::callback(|_| Err(Error::custom("multi_configuration_failed", "配置失败"))),
    ];
    assert!(
        matches!(query.alias().where_all(columns,1),Err(error) if error.code()=="multi_configuration_failed")
    );
    assert_eq!(query.to_sql()?, original);
    let outer = query.alias();
    let column = MultiColumn::callback(move |child| child.where_exists(&outer));
    assert!(
        matches!(query.alias().where_any([column],MultiComparison::Omitted),Err(error) if error.code()=="query_subquery_cycle")
    );
    assert_eq!(query.to_sql()?, original);
    assert_eq!(query.count().await?, 5);
    Ok(())
}

#[tokio::test]
async fn existing_rhs_query_is_recaptured_after_each_distinct_lhs_configuration() -> Result<()> {
    let db = database().await?;
    let rhs = db.table("samples")?.select(&["a"])?;
    let first_rhs = rhs.alias();
    let second_rhs = rhs.alias();
    let columns = [
        MultiColumn::callback(move |q| {
            first_rhs.alias().r#where("id", 1)?;
            q.select_raw(RawSql::new("?"), vec![10.into()])
        }),
        MultiColumn::callback(move |q| {
            second_rhs.alias().r#where("flag", 0)?;
            q.select_raw(RawSql::new("?"), vec![20.into()])
        }),
    ];
    let query = db.table("samples")?.where_all(
        columns,
        MultiComparison::compare(WhereOperator::Gt, MultiValue::from(&rhs)),
    )?;
    assert_eq!(
        query.get_bindings()?,
        vec![10.into(), 1.into(), 20.into(), 1.into(), 0.into()]
    );
    // 第一列挂接后同一个live RHS增长了一个绑定；末检必须拒绝，不能借第二列快照洗掉差异。
    assert!(
        matches!(query.count().await,Err(error) if error.code()=="query_subquery_bindings_mismatch")
    );
    Ok(())
}

#[tokio::test]
async fn ordinary_and_json_bad_left_columns_fail_before_rhs_callback() -> Result<()> {
    let db = database().await?;
    for column in ["bad;name", "bad;root->active"] {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let rhs = MultiValue::callback(move |_| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(Error::custom("wrong_rhs_failure", "不应运行右闭包"))
        });
        let parent = db.table("samples")?;
        let original = parent.to_sql()?;
        assert!(
            matches!(parent.alias().where_any([column],MultiComparison::eq(rhs)),Err(error) if error.code()=="query_identifier_invalid")
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(parent.to_sql()?, original);
    }
    Ok(())
}

#[tokio::test]
async fn failed_empty_or_nonempty_nested_callback_cannot_become_unbounded_write() -> Result<()> {
    let db = database().await?;
    for nonempty in [false, true] {
        let replacement = db.clone();
        let column = MultiColumn::callback(move |q| {
            let q = if nonempty { q.r#where("a", 1)? } else { q };
            let q = q.before_query(move |current| {
                *current = replacement.table("different")?;
                Ok(())
            });
            assert!(q.apply_before_query_callbacks().is_err());
            Ok(q)
        });
        let parent = db.table("samples")?;
        let original = parent.to_sql()?;
        let attempted: Result<u64> = async {
            parent
                .alias()
                .where_none([column], MultiComparison::Omitted)?
                .delete()
                .await
        }
        .await;
        assert!(matches!(attempted,Err(error) if error.code()=="query_callback_context_mismatch"));
        assert_eq!(parent.to_sql()?, original);
        assert_eq!(parent.count().await?, 5);
    }
    Ok(())
}

#[tokio::test]
async fn callbacks_accept_actual_borrowed_transaction_and_reject_foreign_replacement() -> Result<()>
{
    let db = database().await?;
    let connection = db.primary_connection()?;
    let tx = connection.begin().await.map_err(Error::internal)?;
    let replacement = db
        .table("samples")?
        .using_transaction(&tx)
        .r#where("a", 1)?;
    let query = db.table("samples")?.using_transaction(&tx).where_any(
        [MultiColumn::callback(move |_| Ok(replacement))],
        MultiComparison::Omitted,
    )?;
    assert_eq!(query.count().await?, 2);
    let rhs = MultiValue::callback(|q| {
        assert!(
            q.query_context()?
                .transaction
                .is_some_and(|current| std::ptr::eq(current, &raw const tx))
        );
        q.select_raw(RawSql::new("?"), vec![1.into()])
    });
    let second = db
        .table("samples")?
        .using_transaction(&tx)
        .where_all(["a", "b"], MultiComparison::eq(rhs))?;
    assert_eq!(second.count().await?, 1);
    drop(second);
    drop(query);
    tx.rollback().await.map_err(Error::internal)?;
    Ok(())
}

#[tokio::test]
async fn cancelled_multi_read_releases_operation_lease_and_can_retry_without_replaying_callback()
-> Result<()> {
    use std::future::Future;
    let mut options = sea_orm::ConnectOptions::new("sqlite::memory:");
    options
        .max_connections(1)
        .min_connections(1)
        .sqlx_logging(false);
    let connection = Database::connect(options).await.map_err(Error::internal)?;
    connection.execute_unprepared("CREATE TABLE cancelled_items(id INTEGER,a INTEGER,b INTEGER); INSERT INTO cancelled_items VALUES(1,1,0),(2,0,1),(3,0,0)").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection.clone())?;
    let tx = connection.begin().await.map_err(Error::internal)?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let query = db
        .table("cancelled_items")?
        .select_raw(RawSql::new("id, ? AS marker"), vec![42.into()])?
        .where_any(["a", "b"], 1)?
        .before_query(move |_| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
    let mut operation = Box::pin(query.get());
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(operation.as_mut().poll(&mut context).is_pending());
    assert!(matches!(query.to_sql(),Err(error) if error.code()=="query_subquery_busy"));
    drop(operation);
    tx.rollback().await.map_err(Error::internal)?;
    let rows = query.get().await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]
            .try_get::<i64>("", "marker")
            .map_err(Error::internal)?,
        42
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn closure_cannot_replace_actual_transaction_even_when_database_handle_is_unchanged()
-> Result<()> {
    let db = database().await?;
    let other = database().await?;
    let connection = db.primary_connection()?;
    let other_connection = other.primary_connection()?;
    let tx = connection.begin().await.map_err(Error::internal)?;
    let other_tx = other_connection.begin().await.map_err(Error::internal)?;
    let rhs = MultiValue::callback(|q| Ok(q.using_transaction(&other_tx)));
    let attempted = db
        .table("samples")?
        .using_transaction(&tx)
        .where_any(["a"], MultiComparison::eq(rhs));
    assert!(matches!(&attempted,Err(error) if error.code()=="query_subquery_context_mismatch"));
    drop(attempted);
    other_tx.rollback().await.map_err(Error::internal)?;
    tx.rollback().await.map_err(Error::internal)?;
    Ok(())
}

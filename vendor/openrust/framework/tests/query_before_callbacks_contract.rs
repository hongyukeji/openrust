//! 公共 Query API 的执行前回调生命周期；每例隔离数据库并显式回收。
#![cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]

use openrust::db::query::{ColumnOperand, ExistsOrResult, RawSql};
use openrust::db::sea_orm::{ConnectionTrait, QueryResult, TransactionTrait};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::testing::TestDatabase;
use openrust::{Db, Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct EmptyMigrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

struct Fixture {
    db: Db,
    database: TestDatabase,
}
impl Fixture {
    async fn create() -> Result<Option<Self>> {
        let supplied_url = std::env::var("OPENRUST_TEST_DATABASE_URL").ok();
        #[cfg(feature = "db-sqlite")]
        let supplied_url = supplied_url.or_else(|| Some("sqlite::memory:".to_owned()));
        let Some(url) = supplied_url else {
            eprintln!("跳过公共查询回调测试：未配置真实数据库且未启用 SQLite");
            return Ok(None);
        };
        let database = TestDatabase::create::<EmptyMigrator>(&url).await?;
        let db = Db::default();
        let setup = async {
            // 命名SQLite URI由框架连接器配置，复用TestDatabase已建立的物理连接池。
            let connection = database.connection()?;
            db.initialize(connection)?;
            db.primary_connection()?
                .execute_unprepared(
                    "CREATE TABLE callback_items (id BIGINT NOT NULL, label VARCHAR(32) NOT NULL)",
                )
                .await
                .map_err(Error::internal)?;
            for id in 1_i64..=5 {
                db.table("callback_items")?
                    .insert(vec![
                        ("id", id.into()),
                        ("label", format!("row-{id}").into()),
                    ])
                    .await?;
            }
            Ok::<(), Error>(())
        }
        .await;
        let fixture = Self { db, database };
        if let Err(error) = setup {
            sqlite_diagnostic("fixture setup", &error);
            fixture.close().await?;
            return Err(error);
        }
        Ok(Some(fixture))
    }
    async fn close(self) -> Result<()> {
        let connection = self.db.primary_connection();
        let close = match connection {
            Ok(connection) => connection.close().await.map_err(Error::internal),
            Err(_) => Ok(()),
        };
        let cleanup = self.database.close().await;
        close?;
        cleanup
    }
}
fn sqlite_diagnostic(stage: &str, error: &Error) {
    // 无服务环境只使用自有SQLite fixture，可输出其SQL诊断；真实服务不输出source链。
    if std::env::var_os("OPENRUST_TEST_DATABASE_URL").is_none() {
        eprintln!("owned SQLite {stage}: {error}");
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            eprintln!("owned SQLite source: {cause}");
            source = cause.source();
        }
    }
}

fn check(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom("query_callback_contract_failed", message))
    }
}
fn first_row(rows: &[QueryResult]) -> Result<&QueryResult> {
    rows.first()
        .ok_or_else(|| Error::custom("query_callback_fixture_empty", "测试查询未返回首行"))
}
fn hits(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

// 断言通过 Result 返回，确保行为失败时仍走本例数据库回收。
macro_rules! case {
    ($name:ident, |$db:ident| $body:block) => {
        #[tokio::test]
        async fn $name() -> Result<()> {
            let Some(fixture) = Fixture::create().await? else { return Ok(()); };
            let $db = &fixture.db;
            let outcome = async { $body Ok::<(), Error>(()) }.await;
            let cleanup = fixture.close().await;
            outcome?;
            cleanup
        }
    };
}

case!(callbacks_run_in_order_and_modify_bindings, |db| {
    let counter = AtomicUsize::new(0);
    let query = db
        .table("callback_items")?
        .before_query(|q| {
            check(
                counter.fetch_add(1, Ordering::SeqCst) == 0,
                "首回调顺序错误",
            )?;
            *q = q.clone().where_gt("id", 1_i64)?;
            Ok(())
        })
        .before_query(|q| {
            check(
                counter.fetch_add(1, Ordering::SeqCst) == 1,
                "次回调顺序错误",
            )?;
            *q = q.clone().where_lt("id", 4_i64)?;
            Ok(())
        });
    check(query.get().await?.len() == 2, "回调条件未生效")?;
    check(
        query.get_bindings()?.len() == 2 && hits(&counter) == 2,
        "绑定或顺序不匹配",
    )?;
});

case!(sql_preview_consumes_once_and_preserves_mutation, |db| {
    let counter = AtomicUsize::new(0);
    let query = db.table("callback_items")?.before_query(|q| {
        counter.fetch_add(1, Ordering::SeqCst);
        *q = q.clone().where_eq("id", 2_i64)?;
        Ok(())
    });
    let first = query.to_sql()?;
    check(
        query.to_sql()? == first && hits(&counter) == 1,
        "预览重复执行回调",
    )?;
    check(
        query.get().await?.len() == 1 && hits(&counter) == 1,
        "预览未消费回调或丢失条件",
    )?;
});

case!(binding_reads_do_not_run_locally_borrowed_callback, |db| {
    let counter = AtomicUsize::new(0);
    let query = db.table("callback_items")?.before_query(|q| {
        counter.fetch_add(1, Ordering::SeqCst);
        *q = q.clone().where_eq("id", 3_i64)?;
        Ok(())
    });
    check(
        query.get_bindings()?.is_empty() && hits(&counter) == 0,
        "绑定读取执行了回调",
    )?;
    query.apply_before_query_callbacks()?;
    check(
        hits(&counter) == 1 && query.get_bindings()?.len() == 1,
        "局部借用回调未执行",
    )?;
});

case!(
    ordinary_parent_clone_has_independent_callback_consumption,
    |db| {
        let counter = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let cloned = query.clone();
        cloned.to_sql()?;
        check(hits(&counter) == 1, "克隆未执行回调")?;
        query.to_sql()?;
        check(hits(&counter) == 2, "父查询克隆共享了消费状态")?;
    }
);

case!(
    callback_failure_keeps_modifications_and_registration_for_retry,
    |db| {
        let counter = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|q| {
            *q = q.clone().where_gt("id", 1_i64)?;
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(Error::custom("retry_callback", "首次拒绝"));
            }
            Ok(())
        });
        let failure = query
            .to_sql()
            .err()
            .ok_or_else(|| Error::custom("missing_error", "首次回调应失败"))?;
        check(
            failure.code() == "retry_callback" && query.get_bindings()?.len() == 1,
            "错误或部分修改丢失",
        )?;
        query.to_sql()?;
        check(
            hits(&counter) == 2 && query.get_bindings()?.len() == 2,
            "回调列表未保留",
        )?;
        query.to_sql()?;
        check(hits(&counter) == 2, "成功后未消费列表")?;
    }
);

case!(
    database_failure_occurs_after_callbacks_have_been_consumed,
    |db| {
        let counter = AtomicUsize::new(0);
        let query = db.table("missing_callback_table")?.before_query(|_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        check(query.get().await.is_err(), "缺表未失败")?;
        check(
            query.get().await.is_err() && hits(&counter) == 1,
            "SQL失败重跑了已消费回调",
        )?;
    }
);

case!(
    new_callbacks_do_not_run_in_snapshot_and_are_cleared_on_success,
    |db| {
        let nested = Arc::new(AtomicUsize::new(0));
        let query = db.table("callback_items")?.before_query({
            let nested = Arc::clone(&nested);
            move |q| {
                let nested = Arc::clone(&nested);
                *q = q.clone().before_query(move |_| {
                    nested.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                });
                Ok(())
            }
        });
        query.to_sql()?;
        query.to_sql()?;
        check(hits(&nested) == 0, "执行期间新增回调被执行或未被清空")?;
    }
);

case!(get_restores_original_projection_after_success, |db| {
    let query = db.table("callback_items")?.before_query(|q| {
        *q = q.clone().select(&["id"])?;
        Ok(())
    });
    let rows = query.get().await?;
    check(
        first_row(&rows)?.try_get::<String>("", "label").is_err(),
        "回调投影未用于本次get",
    )?;
    let rows = query.get().await?;
    first_row(&rows)?
        .try_get::<String>("", "label")
        .map_err(Error::internal)?;
});

case!(pluck_restores_original_projection_after_success, |db| {
    let query = db.table("callback_items")?;
    check(query.pluck::<i64>("id").await?.len() == 5, "pluck结果错误")?;
    let rows = query.get().await?;
    first_row(&rows)?
        .try_get::<String>("", "label")
        .map_err(Error::internal)?;
});

case!(failed_get_and_pluck_leave_current_projection, |db| {
    let query = db.table("callback_items")?.before_query(|q| {
        // SQLite双引号未知列可能成为字符串；用不存在的未引号WHERE列确定触发数据库失败。
        *q = q.clone().select(&["label"])?.where_raw(
            RawSql::new("callback_missing_column = ?"),
            vec![1_i64.into()],
        );
        Ok(())
    });
    check(query.get().await.is_err(), "错误SQL未失败")?;
    check(query.to_sql()?.contains("label"), "get失败恢复了投影")?;
    let query = db.table("callback_items")?.where_raw(
        RawSql::new("callback_missing_column = ?"),
        vec![1_i64.into()],
    );
    check(
        query.pluck::<String>("label").await.is_err(),
        "错误pluck未失败",
    )?;
    check(query.to_sql()?.contains("label"), "pluck失败恢复了投影")?;
});

case!(first_persists_limit_one_on_original_query, |db| {
    let query = db.table("callback_items")?;
    check(query.first().await?.is_some(), "first缺少行")?;
    check(query.get().await?.len() == 1, "first未保留原实例limit1")?;
});

case!(value_persists_limit_one_and_restores_projection, |db| {
    let query = db.table("callback_items")?;
    check(query.value::<i64>("id").await?.is_some(), "value缺值")?;
    let rows = query.get().await?;
    check(rows.len() == 1, "value未保留limit1")?;
    first_row(&rows)?
        .try_get::<String>("", "label")
        .map_err(Error::internal)?;
});

case!(
    count_prepares_clone_and_leaves_original_callbacks_pending,
    |db| {
        let counter = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|q| {
            counter.fetch_add(1, Ordering::SeqCst);
            *q = q.clone().where_gt("id", 3_i64)?;
            Ok(())
        });
        check(
            query.count().await? == 2 && hits(&counter) == 1,
            "count未在克隆应用回调",
        )?;
        check(query.get_bindings()?.is_empty(), "count污染原查询")?;
        check(
            query.get().await?.len() == 2 && hits(&counter) == 2,
            "原回调被count消费",
        )?;
    }
);

case!(
    empty_pagination_runs_only_count_and_keeps_parent_pending,
    |db| {
        let counter = AtomicUsize::new(0);
        let query = db
            .table("callback_items")?
            .where_eq("id", 99_i64)?
            .before_query(|_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        let page = query.paginate(2).await?;
        check(
            page.total == 0 && page.items.is_empty() && hits(&counter) == 1,
            "空分页额外读取rows",
        )?;
        query.to_sql()?;
        check(hits(&counter) == 2, "空分页消费了原回调")?;
    }
);

case!(
    nonempty_pagination_runs_count_clone_then_parent_once,
    |db| {
        let counter = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let page = query.paginate(2).await?;
        check(
            page.total == 5 && page.items.len() == 2 && hits(&counter) == 2,
            "非空分页生命周期错误",
        )?;
        query.to_sql()?;
        check(hits(&counter) == 2, "分页未消费原回调")?;
    }
);

case!(simple_pagination_prepares_parent_once, |db| {
    let counter = AtomicUsize::new(0);
    let query = db.table("callback_items")?.before_query(|_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let page = query.simple_paginate(2).await?;
    check(
        page.items.len() == 2 && page.has_more && hits(&counter) == 1,
        "simple分页额外count或回调次数错误",
    )?;
});

case!(zero_limit_chunk_does_no_callback_or_page_work, |db| {
    let counter = AtomicUsize::new(0);
    let pages = AtomicUsize::new(0);
    let query = db
        .table("missing_callback_table")?
        .limit(0)
        .before_query(|_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    query
        .chunk_by_id(2, "id", |_| {
            pages.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await?;
    check(
        hits(&counter) == 0 && hits(&pages) == 0,
        "limit0仍访问数据库或回调",
    )?;
});

case!(
    chunk_preserves_total_limit_and_first_offset_without_extra_round,
    |db| {
        let counter = AtomicUsize::new(0);
        let ids = Arc::new(std::sync::Mutex::new(Vec::new()));
        let query = db
            .table("callback_items")?
            .offset(1)
            .limit(3)
            .before_query(|_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        query
            .chunk_by_id(2, "id", |rows| {
                let ids = Arc::clone(&ids);
                async move {
                    let values = rows
                        .iter()
                        .map(|row| row.try_get::<i64>("", "id").map_err(Error::internal))
                        .collect::<Result<Vec<_>>>()?;
                    ids.lock()
                        .map_err(|_| Error::custom("fixture_lock", "测试收集锁失败"))?
                        .extend(values);
                    Ok(())
                }
            })
            .await?;
        check(
            *ids.lock()
                .map_err(|_| Error::custom("fixture_lock", "测试收集锁失败"))?
                == [2, 3, 4],
            "分块总limit或首offset错误",
        )?;
        check(hits(&counter) == 2, "分块耗尽总limit后多读一轮")?;
    }
);

case!(where_exists_does_not_execute_child_callbacks, |db| {
    let counter = AtomicUsize::new(0);
    let child = db
        .table("callback_items")?
        .where_eq("id", 1_i64)?
        .before_query(|_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    let parent = db.table("callback_items")?.where_exists(&child)?;
    check(
        parent.get_bindings()?.len() == 1 && hits(&counter) == 0,
        "whereExists绑定读取执行child或丢绑定",
    )?;
    check(
        parent.get().await?.len() == 5 && hits(&counter) == 0,
        "whereExists执行child回调",
    )?;
    child.to_sql()?;
    check(hits(&counter) == 1, "child回调被父消费")?;
});

case!(
    union_prepares_live_shared_child_only_once_across_parent_clones,
    |db| {
        let counter = AtomicUsize::new(0);
        let child = db
            .table("callback_items")?
            .select(&["id"])?
            .before_query(|_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        let parent = db
            .table("callback_items")?
            .select(&["id"])?
            .union(&child, true)?;
        check(hits(&counter) == 0, "union接入过早执行child")?;
        parent.clone().to_sql()?;
        parent.to_sql()?;
        check(hits(&counter) == 1, "父克隆未共享union child消费状态")?;
    }
);

case!(scalar_operand_consumes_child_immediately, |db| {
    let counter = AtomicUsize::new(0);
    let child = db
        .table("callback_items")?
        .select(&["id"])?
        .limit(1)
        .before_query(|_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    let parent = db.table("callback_items")?.where_between_columns(
        ColumnOperand::from(&child),
        [RawSql::new("0"), RawSql::new("9")],
    )?;
    check(hits(&counter) == 1, "标量接入未准备child")?;
    parent.to_sql()?;
    child.to_sql()?;
    check(hits(&counter) == 1, "标量child被重复消费")?;
});

case!(
    binding_snapshot_does_not_run_union_and_late_bindings_are_not_repaired,
    |db| {
        let counter = AtomicUsize::new(0);
        let child = db
            .table("callback_items")?
            .select(&["id"])?
            .before_query(|q| {
                counter.fetch_add(1, Ordering::SeqCst);
                *q = q.clone().where_eq("id", 2_i64)?;
                Ok(())
            });
        let parent = db
            .table("callback_items")?
            .select(&["id"])?
            .where_eq("id", 1_i64)?
            .union(&child, true)?;
        check(
            parent.get_bindings()?.len() == 1 && hits(&counter) == 0,
            "读父绑定执行child",
        )?;
        let prepared = parent.to_sql();
        check(hits(&counter) == 1, "union未晚执行child")?;
        check(
            parent.get_bindings()?.len() == 1,
            "union后来绑定被悄悄重新收集",
        )?;
        if prepared.is_ok() {
            let outcome = parent.get().await;
            if let Ok(rows) = outcome {
                check(rows.len() == 1, "不匹配绑定仍返回了额外union行")?;
                check(
                    first_row(&rows)?
                        .try_get::<i64>("", "id")
                        .map_err(Error::internal)?
                        == 1,
                    "不匹配绑定不能改写outer id1",
                )?;
            }
        }
    }
);

case!(
    database_replacement_and_transaction_rebinding_are_sticky_failures,
    |db| {
        let other = Db::default();
        other.initialize(db.primary_connection()?)?;
        let query = db.table("callback_items")?.before_query(|q| {
            *q = other.table("callback_items")?;
            Ok(())
        });
        check(query.to_sql().is_err(), "回调可切换数据库身份")?;
        check(query.to_sql().is_err(), "数据库归属错误未保持拒绝")?;
        let transaction = db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        let child = db.table("callback_items")?;
        let invalid = db
            .table("callback_items")?
            .where_exists(&child)?
            .using_transaction(&transaction);
        let outcome = async {
            check(invalid.exists().await.is_err(), "子查询事务归属未拒绝")?;
            check(
                invalid.clone().exists().await.is_err(),
                "克隆清除了无效事务来源",
            )
        }
        .await;
        drop(invalid);
        drop(child);
        transaction.rollback().await.map_err(Error::internal)?;
        outcome?;
    }
);

case!(
    conditional_callback_can_borrow_same_query_after_existence_check,
    |db| {
        let query = db.table("callback_items")?;
        check(
            query.doesnt_exist_or(|| query.count()).await? == ExistsOrResult::Callback(5),
            "条件回调无法复用同一查询",
        )?;
        let missing = db.table("callback_items")?.where_eq("id", 99_i64)?;
        check(
            missing.exists_or(|| missing.count()).await? == ExistsOrResult::Callback(0),
            "空查询条件回调复用失败",
        )?;
    }
);

case!(
    callback_can_capture_borrowed_child_in_same_actual_transaction,
    |db| {
        let transaction = db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        let child = db
            .table("callback_items")?
            .using_transaction(&transaction)
            .where_eq("id", 1_i64)?;
        let query = db
            .table("callback_items")?
            .using_transaction(&transaction)
            .before_query(|q| {
                *q = q.clone().where_exists(&child)?;
                Ok(())
            });
        let outcome =
            async { check(query.get().await?.len() == 5, "同事务借用子查询回调失败") }.await;
        drop(query);
        drop(child);
        transaction.rollback().await.map_err(Error::internal)?;
        outcome?;
    }
);

case!(
    callback_can_attach_same_context_cloned_peer_without_running_peer_callbacks,
    |db| {
        let pending = Arc::new(AtomicUsize::new(0));
        let query = db.table("callback_items")?.before_query({
            let pending = Arc::clone(&pending);
            move |q| {
                let pending = Arc::clone(&pending);
                let peer = q.clone().where_eq("id", 1_i64)?.before_query(move |_| {
                    pending.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                });
                *q = q.clone().where_exists(&peer)?;
                Ok(())
            }
        });
        check(query.get().await?.len() == 5, "回调内同context peer未接入")?;
        check(hits(&pending) == 0, "whereExists执行了peer自身回调")?;
    }
);

case!(
    rebinding_child_returns_new_handle_without_rebinding_existing_parent,
    |db| {
        let transaction = db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        let child = db.table("callback_items")?.where_eq("id", 1_i64)?;
        let parent = db.table("callback_items")?.where_exists(&child)?;
        let rebound = child.using_transaction(&transaction);
        let outcome = async {
            check(rebound.get().await?.len() == 1, "重绑child未在新事务执行")?;
            check(parent.get().await?.len() == 5, "旧父查询追随child事务重绑")
        }
        .await;
        drop(rebound);
        drop(parent);
        transaction.rollback().await.map_err(Error::internal)?;
        outcome?;
    }
);

case!(
    indirect_and_callback_self_cycles_are_rejected_without_corrupting_graph,
    |db| {
        let a = db.table("callback_items")?.select(&["id"])?;
        let b = db
            .table("callback_items")?
            .select(&["id"])?
            .where_exists(&a)?;
        let failure = a
            .union(&b, true)
            .err()
            .ok_or_else(|| Error::custom("missing_cycle", "祖先回引应拒绝"))?;
        check(failure.code() == "query_subquery_cycle", "间接环错误码不符")?;
        check(b.get().await?.len() == 5, "拒绝成环破坏了已存在父图")?;
        let query = db.table("callback_items")?.before_query(|q| {
            *q = q.clone().where_exists(q)?;
            Ok(())
        });
        let failure = query
            .to_sql()
            .err()
            .ok_or_else(|| Error::custom("missing_cycle", "回调自引用应拒绝"))?;
        check(failure.code() == "query_subquery_cycle", "回写环错误码不符")?;
        check(query.to_sql().is_err(), "回写环失败未保留拒绝状态")?;
    }
);

case!(same_child_can_be_referenced_twice_as_acyclic_graph, |db| {
    let child = db.table("callback_items")?.where_eq("id", 1_i64)?;
    let parent = db
        .table("callback_items")?
        .where_exists(&child)?
        .where_exists(&child)?;
    check(parent.get().await?.len() == 5, "合法DAG被误判为环")?;
    check(
        parent.get_bindings()?.len() == 2,
        "重复引用child绑定未按两次接入保存",
    )?;
});

case!(
    union_uses_captured_values_when_live_child_replaces_equal_count_bindings,
    |db| {
        let query_db = db.clone();
        let child = db
            .table("callback_items")?
            .select(&["id"])?
            .where_eq("id", 1_i64)?
            .before_query(move |q| {
                *q = query_db
                    .table("callback_items")?
                    .select(&["id"])?
                    .where_eq("id", 2_i64)?;
                Ok(())
            });
        let parent = db
            .table("callback_items")?
            .select(&["id"])?
            .where_eq("id", 99_i64)?
            .union(&child, true)?;
        let frozen = parent.get_bindings()?;
        check(
            frozen == vec![99_i64.into(), 1_i64.into()],
            "union接入未冻结原值",
        )?;
        let rows = parent.get().await?;
        check(rows.len() == 1, "冻结union值未保持单行")?;
        check(
            first_row(&rows)?
                .try_get::<i64>("", "id")
                .map_err(Error::internal)?
                == 1,
            "union悄悄换绑为child新值2",
        )?;
        check(
            parent.get_bindings()? == frozen,
            "准备child后父绑定值被替换",
        )?;
    }
);

case!(
    callback_binding_inspection_sees_previous_mutation_without_reentry,
    |db| {
        let inspected = AtomicUsize::new(0);
        let query = db
            .table("callback_items")?
            .before_query(|q| {
                *q = q.clone().where_eq("id", 2_i64)?;
                Ok(())
            })
            .before_query(|q| {
                check(
                    q.get_bindings()? == vec![2_i64.into()],
                    "回调内绑定读取未看到前序修改",
                )?;
                inspected.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        check(
            query.get().await?.len() == 1 && hits(&inspected) == 1,
            "回调内绑定读取重入或被跳过",
        )?;
    }
);

case!(
    pending_database_io_rejects_projection_overlap_and_recovers_after_cancellation,
    |db| {
        use futures_util::future::{AbortHandle, Abortable};
        use openrust::db::DbBackend;
        use std::time::Duration;
        if db.backend()? == DbBackend::Sqlite {
            eprintln!("SQLite 不适用行锁等待取消回归；该项须在真实 MySQL/PostgreSQL 执行");
            return Ok(());
        }

        let blocker = db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;

        let blocking = db
            .table("callback_items")?
            .using_transaction(&blocker)
            .where_eq("id", 1_i64)?
            .update(vec![("label", "held-row".into())])
            .await;
        if let Err(error) = blocking {
            blocker.rollback().await.map_err(Error::internal)?;
            return Err(error);
        }

        let transaction = match db.primary_connection()?.begin().await {
            Ok(transaction) => transaction,
            Err(error) => {
                blocker.rollback().await.map_err(Error::internal)?;
                return Err(Error::internal(error));
            }
        };

        let identity_sql = match db.backend()? {
            DbBackend::MySql => "SELECT CAST(CONNECTION_ID() AS SIGNED) AS connection_id",
            DbBackend::Postgres => "SELECT CAST(pg_backend_pid() AS BIGINT) AS connection_id",
            _ => {
                return Err(Error::custom(
                    "unsupported_lock_observer",
                    "后端不适用行锁观测",
                ));
            }
        };
        let mut identities = Vec::new();
        for connection in [&blocker, &transaction] {
            let row = connection
                .query_one_raw(openrust::db::sea_orm::Statement::from_string(
                    db.backend()?,
                    identity_sql,
                ))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| Error::custom("missing_connection_identity", "事务连接身份缺失"))?;
            identities.push(
                row.try_get::<i64>("", "connection_id")
                    .map_err(Error::internal)?,
            );
        }
        let blocker_id = identities[0];
        let reader_id = identities[1];
        let mut blocker = Some(blocker);
        let outcome = async {
            let calls = Arc::new(AtomicUsize::new(0));
            let reached = Arc::new(tokio::sync::Notify::new());
            let query = Arc::new(
                db.table("callback_items")?
                    .using_transaction(&transaction)
                    .select(&["id"])?
                    .where_eq("id", 1_i64)?
                    .lock_for_update()?
                    .before_query({
                        let calls = Arc::clone(&calls);
                        let reached = Arc::clone(&reached);
                        move |q| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            *q = q.clone().select(&["label"])?;
                            reached.notify_one();
                            Ok(())
                        }
                    }),
            );
            // Future 取消保留实际事务借用；无需为生成 'static 任务泄漏事务。
            let (abort, registration) = AbortHandle::new_pair();
            let pending = Abortable::new(query.get(), registration);
            let controller = async {
                let outcome = async {
                    tokio::time::timeout(Duration::from_secs(10), reached.notified())
                        .await
                        .map_err(|_| {
                            Error::custom("query_callback_notify_timeout", "查询前回调未抵达")
                        })?;
                    let overlap =
                        tokio::time::timeout(Duration::from_secs(5), query.pluck::<i64>("id"))
                            .await
                            .map_err(|_| {
                                Error::custom("query_callback_busy_timeout", "并发投影未有界拒绝")
                            })?;
                    let error = overlap
                        .err()
                        .ok_or_else(|| Error::custom("missing_busy", "并发查询应忙拒绝"))?;
                    check(error.code() == "query_subquery_busy", "并发投影错误码不符")?;
                    // 回调通知早于驱动prepare/execute；须确认服务器已进入行锁等待再取消。
                    let observer = blocker.as_ref().ok_or_else(|| Error::custom("missing_blocker", "观测事务缺失"))?;
                    let sql = match db.backend()? {
                        DbBackend::MySql => format!("SELECT CAST(COUNT(*) AS SIGNED) AS waiting FROM performance_schema.data_lock_waits w JOIN performance_schema.threads r ON r.THREAD_ID = w.REQUESTING_THREAD_ID JOIN performance_schema.threads b ON b.THREAD_ID = w.BLOCKING_THREAD_ID WHERE r.PROCESSLIST_ID = {reader_id} AND b.PROCESSLIST_ID = {blocker_id}"),
                        DbBackend::Postgres => format!("SELECT COUNT(*) AS waiting FROM pg_stat_activity WHERE pid = {reader_id} AND wait_event_type = 'Lock' AND {blocker_id} = ANY(pg_blocking_pids(pid))"),
                        _ => return Err(Error::custom("unsupported_lock_observer", "后端不适用行锁观测")),
                    };
                    tokio::time::timeout(Duration::from_secs(10), async {
                        loop {
                            let row = observer.query_one_raw(openrust::db::sea_orm::Statement::from_string(db.backend()?, sql.clone())).await.map_err(Error::internal)?
                                .ok_or_else(|| Error::custom("missing_lock_observation", "行锁观测缺失"))?;
                            if row.try_get::<i64>("", "waiting").map_err(Error::internal)? > 0 { break Ok::<(), Error>(()); }
                            tokio::task::yield_now().await;
                        }
                    }).await.map_err(|_| Error::custom("lock_wait_not_observed", "查询未进入服务器行锁等待"))??;

                    Ok(())
                }
                .await;
                abort.abort();
                outcome
            };

            let (cancelled, control_result) = tokio::join!(pending, controller);

            // 先释放真实行锁，再验证同对象的取消状态与后续可执行性。

            let released = blocker
                .take()
                .ok_or_else(|| Error::custom("missing_blocker", "待锁事务缺失"))?
                .rollback()
                .await
                .map_err(Error::internal);
            let outcome = async {
                control_result?;
                check(cancelled.is_err(), "被锁查询未观察到取消")?;
                released?;

                let sql = query.to_sql()?;

                check(sql.contains("label"), "并发pluck或取消恢复擦除了失败投影")?;

                let rows = tokio::time::timeout(Duration::from_secs(10), query.get())
                    .await
                    .map_err(|_| {
                        Error::custom("query_callback_reuse_timeout", "取消后查询未恢复")
                    })??;

                check(rows.len() == 1, "取消后查询结果错误")?;
                first_row(&rows)?
                    .try_get::<String>("", "label")
                    .map_err(Error::internal)?;
                check(
                    first_row(&rows)?.try_get::<i64>("", "id").is_err(),
                    "忙拒绝的pluck改变了原投影",
                )?;
                check(hits(&calls) == 1, "取消后重跑已消费回调")
            }
            .await;
            drop(query);
            outcome
        }
        .await;
        let blocker_cleanup = match blocker.take() {
            Some(blocker) => blocker.rollback().await.map_err(Error::internal),
            None => Ok(()),
        };

        let transaction_cleanup = transaction.rollback().await.map_err(Error::internal);

        outcome?;
        blocker_cleanup?;
        transaction_cleanup?;
    }
);

case!(
    whole_same_context_replacement_preserves_failed_callback_for_retry,
    |db| {
        let calls = AtomicUsize::new(0);
        let replacement = db.clone();
        let query = db.table("callback_items")?.before_query(|q| {
            calls.fetch_add(1, Ordering::SeqCst);
            *q = replacement.table("callback_items")?.where_eq("id", 2_i64)?;
            Err(Error::custom(
                "whole_replacement_rejected",
                "整体替换后拒绝",
            ))
        });
        for expected in [1, 2] {
            let error =
                query.get().await.err().ok_or_else(|| {
                    Error::custom("missing_retry_error", "失败回调被整体替换清除")
                })?;
            check(
                error.code() == "whole_replacement_rejected",
                "整体替换回调错误码改变",
            )?;
            check(hits(&calls) == expected, "第二次未重新执行失败回调")?;
            check(
                query.get_bindings()? == vec![2_i64.into()],
                "整体替换失败丢失部分条件",
            )?;
        }
    }
);

case!(
    implicit_clone_entry_points_reject_callback_reentry_before_recursing,
    |db| {
        use futures_util::FutureExt;
        let checks = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|q| {
            let count = q.count().now_or_never();
            check(
                matches!(count, Some(Err(ref error)) if error.code() == "query_subquery_busy"),
                "count隐式克隆未有界拒绝重入",
            )?;
            let simple = q.simple_paginate(2).now_or_never();
            check(
                matches!(simple, Some(Err(ref error)) if error.code() == "query_subquery_busy"),
                "simple分页未有界拒绝重入",
            )?;
            let chunk = q.chunk_by_id(2, "id", |_| async { Ok(()) }).now_or_never();
            check(
                matches!(chunk, Some(Err(ref error)) if error.code() == "query_subquery_busy"),
                "chunk隐式克隆未有界拒绝重入",
            )?;
            checks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        query.to_sql()?;
        check(hits(&checks) == 1, "重入探针未完成或递归执行回调")?;
    }
);

case!(
    plain_nested_conditions_can_rebind_but_nested_scalar_sources_cannot,
    |db| {
        let plain = db
            .table("callback_items")?
            .where_nested(|group| group.where_eq("id", 1_i64))?;
        let child = db.table("callback_items")?.select(&["id"])?.limit(1);
        let scalar = db.table("callback_items")?.where_nested(|group| {
            group.where_between_columns(
                ColumnOperand::from(&child),
                [RawSql::new("0"), RawSql::new("9")],
            )
        })?;
        let transaction = db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        let plain = plain.using_transaction(&transaction);
        let scalar = scalar.using_transaction(&transaction);
        let outcome = async {
            check(
                plain.get().await?.len() == 1,
                "普通nested条件被误判为跨事务子查询",
            )?;
            check(
                scalar.get().await.is_err(),
                "nested标量子查询事务重绑未拒绝",
            )
        }
        .await;
        drop(plain);
        drop(scalar);
        transaction.rollback().await.map_err(Error::internal)?;
        outcome?;
    }
);

case!(
    value_returns_first_selected_field_for_explicit_or_callback_projection,
    |db| {
        let explicit = db
            .table("callback_items")?
            .select(&["label"])?
            .where_eq("id", 1_i64)?;
        check(
            explicit.value::<String>("id").await? == Some("row-1".to_owned()),
            "value忽略了显式投影首字段",
        )?;
        let callback = db
            .table("callback_items")?
            .where_eq("id", 1_i64)?
            .before_query(|q| {
                *q = q.clone().select(&["label"])?;
                Ok(())
            });
        check(
            callback.value::<String>("id").await? == Some("row-1".to_owned()),
            "value未读取回调投影首字段",
        )?;
    }
);

case!(empty_write_prechecks_do_not_consume_callbacks, |db| {
    let calls = AtomicUsize::new(0);
    let query = db.table("callback_items")?.before_query(|_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    check(
        query.insert(Vec::new()).await.is_err(),
        "保留当前Rust空insert拒绝语义",
    )?;
    check(hits(&calls) == 0, "空insert预检查消费了回调")?;
    check(
        query.upsert(Vec::new(), &["id"], &["label"]).await.is_err(),
        "保留当前Rust空upsert拒绝语义",
    )?;
    check(hits(&calls) == 0, "空upsert预检查消费了回调")?;
    check(
        query.exists().await? && hits(&calls) == 1,
        "预检查后回调未由exists正常消费",
    )?;
});

type DropTarget =
    std::sync::Mutex<Option<Arc<openrust::db::query::QueryBuilder<'static, 'static>>>>;
struct BindingReadOnDrop {
    target: std::sync::Weak<DropTarget>,
    observed: Arc<AtomicUsize>,
}
impl Drop for BindingReadOnDrop {
    fn drop(&mut self) {
        let query = self.target.upgrade().and_then(|target| {
            target
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(Arc::clone))
        });
        if let Some(query) = query
            && query.get_bindings().is_ok()
        {
            self.observed.fetch_add(1, Ordering::SeqCst);
        }
    }
}

// 同步死锁不能靠 Tokio timeout 抢占；总控需对子进程设置外部 deadline。
case!(
    dropping_consumed_callback_capture_can_read_bindings_without_state_lock_deadlock,
    |db| {
        let target: Arc<DropTarget> = Arc::new(std::sync::Mutex::new(None));
        let observed = Arc::new(AtomicUsize::new(0));
        let capture = BindingReadOnDrop {
            target: Arc::downgrade(&target),
            observed: Arc::clone(&observed),
        };
        let query = Arc::new(db.table("callback_items")?.before_query(move |_| {
            std::hint::black_box(&capture);
            Ok(())
        }));
        *target
            .lock()
            .map_err(|_| Error::custom("fixture_lock", "释放见证锁失败"))? =
            Some(Arc::clone(&query));
        // Tokio 超时用于异步调度边界；真正同步阻塞仍由外层进程deadline约束。
        tokio::time::timeout(std::time::Duration::from_secs(10), async { query.to_sql() })
            .await
            .map_err(|_| Error::custom("drop_callback_timeout", "回调释放未完成"))??;
        check(
            hits(&observed) == 1,
            "消费回调没有释放捕获物或重入绑定读取失败",
        )?;
        target
            .lock()
            .map_err(|_| Error::custom("fixture_lock", "释放见证锁失败"))?
            .take();
    }
);

case!(
    callback_replacement_keeps_slot_busy_for_poll_and_scoped_thread,
    |db| {
        use futures_util::FutureExt;
        let entered = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|q| {
        // 只让首轮建立见证，旧实现误入回调时也不会无限生成线程。
        if entered.fetch_add(1, Ordering::SeqCst) != 0 {
            return Ok(());
        }
        *q = q.clone().where_eq("id", 1_i64)?;
        check(matches!(q.count().now_or_never(), Some(Err(ref error)) if error.code() == "query_subquery_busy"), "替换Inner后count绕过槽位忙检查")?;
        check(matches!(q.simple_paginate(2).now_or_never(), Some(Err(ref error)) if error.code() == "query_subquery_busy"), "替换Inner后simple分页绕过槽位忙检查")?;
        check(matches!(q.chunk_by_id(2, "id", |_| async { Ok(()) }).now_or_never(), Some(Err(ref error)) if error.code() == "query_subquery_busy"), "替换Inner后chunk绕过槽位忙检查")?;
        let same_slot = &*q;
        let threaded = std::thread::scope(|scope| {
            scope.spawn(|| same_slot.to_sql()).join()
        }).map_err(|_| Error::custom("query_contract_thread_failed", "作用域线程提前退出"))?;
        let error = threaded.err().ok_or_else(|| Error::custom("missing_slot_busy", "同槽位跨线程编译应拒绝"))?;
        check(error.code() == "query_subquery_busy", "同槽位跨线程忙错误码不符")
    });
        query.to_sql()?;
        check(hits(&entered) == 1, "替换Inner后重入执行了回调")?;
        check(
            query.get_bindings()? == vec![1_i64.into()],
            "替换后的条件丢失",
        )?;
    }
);

case!(
    explicit_callback_clone_can_prepare_independently_with_finite_recursion,
    |db| {
        let entered = AtomicUsize::new(0);
        let query = db.table("callback_items")?.before_query(|q| {
            if entered.fetch_add(1, Ordering::SeqCst) == 0 {
                let cloned = q.clone();
                cloned.to_sql()?;
            }
            Ok(())
        });
        query.to_sql()?;
        check(hits(&entered) == 2, "显式Clone被回调Arc去重或忙槽位误拒绝")?;
        query.to_sql()?;
        check(hits(&entered) == 2, "显式Clone准备后原回调未消费")?;
    }
);

case!(
    independent_queries_compile_concurrently_without_global_busy_rejection,
    |db| {
        let barrier = std::sync::Barrier::new(2);
        let (left, right) = std::thread::scope(|scope| {
            let worker = |id: i64| {
                let mut outcome = Ok(());
                for _ in 0..100 {
                    barrier.wait();
                    // 每轮新建独立查询；首个失败保留，但仍参加后续屏障，防测试自锁。
                    let iteration = (|| -> Result<()> {
                        let query = db.table("callback_items")?.where_eq("id", id)?;
                        query.to_sql()?;
                        check(
                            query.get_bindings()? == vec![id.into()],
                            "独立查询绑定被并行构造污染",
                        )
                    })();
                    if outcome.is_ok() {
                        outcome = iteration;
                    }
                }
                outcome
            };
            let left = scope.spawn(move || worker(1));
            let right = scope.spawn(move || worker(2));
            (left.join(), right.join())
        });
        left.map_err(|_| {
            Error::custom("query_contract_thread_failed", "首个独立查询线程提前退出")
        })??;
        right.map_err(|_| {
            Error::custom("query_contract_thread_failed", "第二个独立查询线程提前退出")
        })??;
    }
);

case!(
    active_callback_slot_cannot_reenter_indirectly_through_outer_union,
    |db| {
        let entered = AtomicUsize::new(0);
        let outer_db = db.clone();
        let query = db
            .table("callback_items")?
            .select(&["id"])?
            .before_query(|q| {
                // 旧框架若错误允许间接重入，第二轮直接返回，保持见证有限。
                if entered.fetch_add(1, Ordering::SeqCst) != 0 {
                    return Ok(());
                }
                *q = q.clone();
                let outer = outer_db.table("callback_items")?.select(&["id"])?;
                let prepared = outer.union(q, true).and_then(|outer| outer.to_sql());
                let error = prepared.err().ok_or_else(|| {
                    Error::custom("missing_union_busy", "UNION间接重入活动参数应拒绝")
                })?;
                check(
                    error.code() == "query_subquery_busy",
                    "UNION间接重入错误码不符",
                )
            });
        query.to_sql()?;
        check(hits(&entered) == 1, "UNION接入活动参数后执行了间接重入")?;
    }
);

const MYSQL_COLD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

async fn mysql_cold_connection_id(connection: &impl ConnectionTrait) -> Result<u64> {
    use openrust::db::sea_orm::{DbBackend, Statement};
    let row = tokio::time::timeout(
        MYSQL_COLD_DEADLINE,
        connection.query_one_raw(Statement::from_string(
            DbBackend::MySql,
            "SELECT CONNECTION_ID() AS connection_id".to_owned(),
        )),
    )
    .await
    .map_err(|_| Error::custom("cold_prepare_id_timeout", "连接标识读取超过期限"))?
    .map_err(Error::internal)?
    .ok_or_else(|| Error::custom("cold_prepare_id_missing", "连接标识缺失"))?;
    row.try_get("", "connection_id").map_err(Error::internal)
}

async fn mysql_cold_normal_prepare_error_preserves_connection(
    single: &Db,
    connection: &openrust::db::sea_orm::DatabaseConnection,
    original_id: u64,
) -> Result<()> {
    // 服务端正常prepare错误可恢复，不应当作取消中的协议状态未知而废弃连接。
    let invalid = single
        .table("callback_items")?
        .select(&["missing_cold_prepare_column"])?;
    let invalid_result = tokio::time::timeout(MYSQL_COLD_DEADLINE, invalid.get())
        .await
        .map_err(|_| {
            Error::custom(
                "cold_prepare_server_error_timeout",
                "正常prepare错误未及时返回",
            )
        })?;
    check(invalid_result.is_err(), "缺列prepare应返回服务端错误")?;
    check(
        mysql_cold_connection_id(connection).await? == original_id,
        "正常服务端prepare错误废弃了可用连接",
    )
}

async fn mysql_cold_cancel_transaction_query(
    single: &Db,
    tx: &openrust::db::sea_orm::DatabaseTransaction,
    original_id: u64,
) -> Result<()> {
    use futures_util::future::{AbortHandle, Abortable};
    check(
        mysql_cold_connection_id(tx).await? == original_id,
        "单连接池事务未复用原连接",
    )?;
    let write = single
        .table("callback_items")?
        .using_transaction(tx)
        .where_eq("id", 1_i64)?;
    let written = tokio::time::timeout(
        MYSQL_COLD_DEADLINE,
        write.update(vec![("label", "uncommitted-cold-prepare".into())]),
    )
    .await
    .map_err(|_| Error::custom("cold_prepare_write_timeout", "事务写入超过期限"))??;
    check(written == 1, "取消前事务写入必须实际影响一行")?;
    // 新单池从未准备这一查询形状；biased join保证先轮询get，再立即发取消。
    let query = single
        .table("callback_items")?
        .using_transaction(tx)
        .select(&["id", "label"])?
        .where_eq("id", 1_i64)?
        .where_eq("label", "uncommitted-cold-prepare")?;
    let (abort, registration) = AbortHandle::new_pair();
    let pending = Abortable::new(query.get(), registration);
    let (cancelled, ()) = tokio::join!(biased; pending, async { abort.abort(); });
    check(cancelled.is_err(), "冷prepare查询首次轮询未观察到取消")?;
    query.to_sql()?;
    let retry = tokio::time::timeout(MYSQL_COLD_DEADLINE, query.get())
        .await
        .map_err(|_| Error::custom("cold_prepare_retry_timeout", "取消后事务查询未及时失败"))?;
    check(retry.is_err(), "冷prepare取消后的事务查询假成功")
}

async fn mysql_cold_cancel_and_rollback(
    single: &Db,
    connection: &openrust::db::sea_orm::DatabaseConnection,
    original_id: u64,
) -> Result<()> {
    let transaction = tokio::time::timeout(MYSQL_COLD_DEADLINE, connection.begin())
        .await
        .map_err(|_| Error::custom("cold_prepare_begin_timeout", "事务建立超过期限"))?
        .map_err(Error::internal)?;
    let attempted = mysql_cold_cancel_transaction_query(single, &transaction, original_id).await;
    // helper返回后全部借事务查询已销毁；失败路径也执行有界rollback。
    let rollback = tokio::time::timeout(MYSQL_COLD_DEADLINE, transaction.rollback())
        .await
        .map_err(|_| {
            Error::custom(
                "cold_prepare_rollback_timeout",
                "冷prepare取消后rollback超过期限",
            )
        });
    attempted?;
    check(rollback?.is_err(), "失效事务rollback不应假成功")
}

async fn mysql_cold_verify_rollback_with_row_lock(
    single: &Db,
    connection: &openrust::db::sea_orm::DatabaseConnection,
) -> Result<()> {
    let transaction = tokio::time::timeout(MYSQL_COLD_DEADLINE, connection.begin())
        .await
        .map_err(|_| {
            Error::custom(
                "cold_prepare_replacement_begin_timeout",
                "替换连接锁验证事务建立超过期限",
            )
        })?
        .map_err(Error::internal)?;
    let outcome = async {
        let query = single
            .table("callback_items")?
            .using_transaction(&transaction)
            .select(&["label"])?
            .where_eq("id", 1_i64)?
            .lock_for_update()?;
        // 锁定读必须等旧事务释放行锁；普通MVCC快照读取不能提供这一证据。
        let rows = tokio::time::timeout(MYSQL_COLD_DEADLINE, query.get())
            .await
            .map_err(|_| {
                Error::custom(
                    "cold_prepare_replacement_timeout",
                    "替换连接锁定读取超过期限，旧行锁可能未释放",
                )
            })??;
        check(rows.len() == 1, "替换连接锁定读取未返回唯一目标行")?;
        let restored = first_row(&rows)?
            .try_get::<String>("", "label")
            .map_err(Error::internal)?;
        check(restored == "row-1", "原事务未提交写入未回滚")
    }
    .await;
    // 先结束借事务查询的作用域，再清理验证事务；读取失败也尝试有界回滚。
    let cleanup = tokio::time::timeout(MYSQL_COLD_DEADLINE, transaction.rollback())
        .await
        .map_err(|_| {
            Error::custom(
                "cold_prepare_replacement_rollback_timeout",
                "替换连接锁验证事务清理超过期限",
            )
        });
    outcome?;
    cleanup?.map_err(Error::internal)
}

async fn mysql_cold_check_replacement_and_observer(
    single: &Db,
    observer: &Db,
    connection: &openrust::db::sea_orm::DatabaseConnection,
    original_id: u64,
) -> Result<()> {
    check(
        mysql_cold_connection_id(connection).await? != original_id,
        "协议状态未知的原连接仍被复用",
    )?;
    mysql_cold_verify_rollback_with_row_lock(single, connection).await?;
    check(
        tokio::time::timeout(
            MYSQL_COLD_DEADLINE,
            observer.table("callback_items")?.count(),
        )
        .await
        .map_err(|_| Error::custom("cold_prepare_observer_timeout", "独立观察池计数超过期限"))??
            == 5,
        "独立观察池不可用",
    )?;
    check(
        tokio::time::timeout(
            MYSQL_COLD_DEADLINE,
            observer
                .table("callback_items")?
                .where_eq("id", 1_i64)?
                .value::<String>("label"),
        )
        .await
        .map_err(|_| Error::custom("cold_prepare_observer_timeout", "独立观察池读取超过期限"))??
            == Some("row-1".to_owned()),
        "独立观察池看到未提交写入",
    )
}

#[tokio::test]
async fn mysql_cold_prepare_first_poll_cancellation_retires_connection_and_rolls_back() -> Result<()>
{
    use openrust::db::sea_orm::{ConnectOptions, Database, DbBackend};
    use std::time::Duration;
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let mut extra_pool = None;
    let outcome = async {
        if fixture.db.backend()? != DbBackend::MySql {
            eprintln!(
                "MySQL冷prepare首次轮询取消回归不适用于SQLite/PostgreSQL；不构成MySQL通过证据"
            );
            return Ok(());
        }
        let mut options = ConnectOptions::new(fixture.database.url().to_owned());
        options
            .max_connections(1)
            .min_connections(0)
            .connect_timeout(Duration::from_secs(5))
            .acquire_timeout(Duration::from_secs(5));
        let connection = tokio::time::timeout(MYSQL_COLD_DEADLINE, Database::connect(options))
            .await
            .map_err(|_| Error::custom("cold_prepare_connect_timeout", "单连接池建立超过期限"))?
            .map_err(Error::internal)?;
        extra_pool = Some(connection.clone());
        let single = Db::default();
        single.initialize(connection.clone())?;
        let original_id = mysql_cold_connection_id(&connection).await?;
        mysql_cold_normal_prepare_error_preserves_connection(&single, &connection, original_id)
            .await?;
        mysql_cold_cancel_and_rollback(&single, &connection, original_id).await?;
        mysql_cold_check_replacement_and_observer(&single, &fixture.db, &connection, original_id)
            .await
    }
    .await;
    let pool_cleanup = match extra_pool {
        Some(pool) => match tokio::time::timeout(MYSQL_COLD_DEADLINE, pool.close()).await {
            Ok(result) => result.map_err(Error::internal),
            Err(_) => Err(Error::custom(
                "cold_prepare_pool_cleanup_timeout",
                "额外单连接池清理超过期限",
            )),
        },
        None => Ok(()),
    };
    let fixture_cleanup = tokio::time::timeout(MYSQL_COLD_DEADLINE, fixture.close())
        .await
        .map_err(|_| {
            Error::custom(
                "cold_prepare_fixture_cleanup_timeout",
                "隔离数据库清理超过期限",
            )
        });
    outcome?;
    pool_cleanup?;
    fixture_cleanup??;
    Ok(())
}

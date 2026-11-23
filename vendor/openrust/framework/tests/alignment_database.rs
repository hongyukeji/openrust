#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
use openrust::{
    config::Repository,
    database::{DatabaseManager, Value, query::Record},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "需要本批隔离 PostgreSQL 18 和 MySQL 8；显式运行，缺配置必须失败"]
async fn query_callbacks_consume_once_map_results_and_preserve_transactions_on_three_databases() {
    for settings in [
        json!({"driver":"sqlite","database":":memory:"}),
        json!({"url":std::env::var("ALIGNMENT_POSTGRES_URL").expect("缺 PostgreSQL 隔离连接")}),
        json!({"url":std::env::var("ALIGNMENT_MYSQL_URL").expect("缺 MySQL 隔离连接")}),
    ] {
        let manager = DatabaseManager::new(
            Arc::new(Repository::new(
                [(
                    "database".into(),
                    json!({"default":"audit","connections":{"audit":settings}}),
                )]
                .into(),
            )),
            ".",
        );
        let connection = manager.connection(None).unwrap();
        connection
            .statement(
                "create table alignment_rows (id integer primary key, name varchar(255))",
                &[],
            )
            .await
            .unwrap();
        let before = Arc::new(AtomicUsize::new(0));
        let counter = before.clone();
        connection
            .table("alignment_rows")
            .before_query(move |query| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(query)
            })
            .insert(&[
                Record::from([
                    ("id".into(), 1.into()),
                    ("name".into(), "O'Reilly ?".into()),
                ]),
                Record::from([("id".into(), 2.into()), ("name".into(), "two".into())]),
            ])
            .await
            .unwrap();
        assert_eq!(before.load(Ordering::SeqCst), 1);
        let counter = before.clone();
        let order = Arc::new(Mutex::new(Vec::new()));
        let o = order.clone();
        let mut query = connection
            .table("alignment_rows")
            .before_query(move |query| {
                counter.fetch_add(1, Ordering::SeqCst);
                o.lock().unwrap().push("before");
                Ok(query.r#where("name", "=", "O'Reilly ?"))
            });
        let sql = query.to_sql().unwrap();
        assert!(sql.contains('?'));
        assert!(!sql.contains("O'Reilly"));
        let raw = query.to_raw_sql().unwrap();
        assert!(raw.contains("O''Reilly ?"));
        assert_eq!(before.load(Ordering::SeqCst), 2);
        let o = order.clone();
        let rows = query
            .after_query(move |rows| {
                o.lock().unwrap().push("after");
                Ok(rows)
            })
            .get()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(*order.lock().unwrap(), ["before", "after"]);
        assert_eq!(before.load(Ordering::SeqCst), 2);
        assert_eq!(
            connection
                .table("alignment_rows")
                .before_query(|query| Ok(query.r#where("id", "=", 2)))
                .update(Record::from([("name".into(), "updated".into())]))
                .await
                .unwrap(),
            1
        );
        let rows = connection
            .table("alignment_rows")
            .order_by("id", "asc")
            .after_query(|mut rows| {
                rows.reverse();
                Ok(rows)
            })
            .get()
            .await
            .unwrap();
        assert_eq!(rows[0].get("id"), Some(&Value::Integer(2)));
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let mut sub = connection
            .table("alignment_rows")
            .select(["id"])
            .before_query(move |query| {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(query.r#where("id", "=", 1))
            });
        for _ in 0..2 {
            let rows = connection
                .table("alignment_rows")
                .where_in_sub("id", &mut sub)
                .unwrap()
                .get()
                .await
                .unwrap();
            assert_eq!(rows.len(), 1);
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut tx = connection.begin_transaction().await.unwrap();
        assert_eq!(
            tx.table("alignment_rows")
                .before_query(|query| Ok(query.r#where("id", "=", 1)))
                .delete()
                .await
                .unwrap(),
            1
        );
        tx.roll_back(None).await.unwrap();
        assert_eq!(
            connection.table("alignment_rows").count("*").await.unwrap(),
            2
        );
        let mut query = connection
            .table("alignment_rows")
            .before_query(|_| Err(openrust::database::DatabaseError::InvalidQuery("回调拒绝")));
        assert!(query.to_sql().is_err());
        assert!(query.to_sql().is_err());
        assert_eq!(
            connection
                .table("alignment_rows")
                .get()
                .await
                .unwrap()
                .len(),
            2
        );
        connection
            .statement("drop table alignment_rows", &[])
            .await
            .unwrap();
    }
}

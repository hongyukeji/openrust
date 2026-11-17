use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, Value,
        events::{QueryBindings, QueryRole},
        query::{Expression, Record, UpdateValue, UpsertUpdate, ValueOperand},
    },
};
use serde_json::{Value as Json, json};

fn connection(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

fn record<const N: usize>(values: [(&str, Value); N]) -> Record {
    values
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn mysql_url() -> Option<String> {
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    url
}

#[tokio::test]
async fn inserted_ids_and_conflict_returning_follow_driver_contracts() {
    let postgres = std::env::var("OPENRUST_TEST_POSTGRES_INSERT_RESULTS_URL").ok();
    assert!(
        postgres.is_some()
            || std::env::var_os("OPENRUST_REQUIRE_POSTGRES_INSERT_RESULTS").is_none()
    );
    let sqlite = connection(json!({"driver":"sqlite","database":":memory:","prefix":"app_"}));
    let mut databases = vec![(sqlite, "integer primary key autoincrement")];
    if let Some(url) = postgres {
        databases.push((
            connection(json!({"url":url,"prefix":"app_"})),
            "bigint generated always as identity primary key",
        ));
    }
    for (db, key_type) in databases {
        db.unprepared(&format!(
            "create table app_result_items (
            item_key {key_type}, code varchar(40) unique not null, label varchar(40) unique not null
        )"
        ))
        .await
        .unwrap();
        db.unprepared("create table app_result_archive (code varchar(40) primary key)")
            .await
            .unwrap();
        let table = || db.table("result_items");
        let item =
            |code: &str, label: &str| record([("code", code.into()), ("label", label.into())]);
        db.enableQueryLog();
        assert_eq!(
            table()
                .insertGetIdFor(item("a", "one"), "item_key")
                .await
                .unwrap(),
            1.into()
        );
        if key_type.starts_with("bigint") {
            assert!(
                db.getQueryLog()[0]
                    .query
                    .ends_with(r#" returning "item_key""#)
            );
        }
        assert_eq!(table().insertOrIgnore(&[item("a", "q")]).await.unwrap(), 0);
        assert_eq!(
            table().insertOrIgnore(&[item("b", "two")]).await.unwrap(),
            1
        );

        let archive = db.table("result_archive");
        let source = archive
            .subquery("result_items")
            .select(["code"])
            .r#where("code", "=", "a");
        assert_eq!(
            archive
                .insertOrIgnoreUsing(&["code"], &source)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            db.table("result_archive")
                .insertOrIgnoreUsing(&["code"], &source)
                .await
                .unwrap(),
            0
        );

        db.forgetRecordModificationState();
        assert!(
            table()
                .insertOrIgnoreReturning(&[item("a", "unused")], &["item_key"], None)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(!db.hasModifiedRecords());
        db.flushQueryLog();
        let rows = table()
            .insertOrIgnoreReturning(
                &[item("a", "ignored"), item("c", "three")],
                &["item_key as key", "code"],
                Some(&["code"]),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0].get("key"), Some(Value::Integer(_))));
        assert_eq!(rows[0].get("code"), Some(&"c".into()));
        assert!(db.hasModifiedRecords());
        assert_eq!(db.getQueryLog()[0].read_write_type, QueryRole::Write);
        assert!(
            table()
                .insertOrIgnoreReturning(&[item("d", "three")], &["item_key"], Some(&["code"]))
                .await
                .is_err()
        );
        assert_eq!(
            table().r#where("code", "=", "d").count("*").await.unwrap(),
            0
        );

        db.forgetRecordModificationState();
        let mut transaction = db.beginTransaction().await.unwrap();
        let rows = transaction
            .table("result_items")
            .insertOrIgnoreReturning(&[item("e", "rollback")], &["code"], None)
            .await
            .unwrap();
        assert_eq!(rows[0].get("code"), Some(&"e".into()));
        assert!(db.hasModifiedRecords());
        transaction.rollBack(None).await.unwrap();
        assert_eq!(
            table().r#where("code", "=", "e").count("*").await.unwrap(),
            0
        );
        assert!(matches!(
            table()
                .insertOrIgnoreReturning(&[item("x", "invalid")], &[], None)
                .await,
            Err(DatabaseError::InvalidQuery("insert returning columns"))
        ));
        db.unprepared("drop table app_result_archive")
            .await
            .unwrap();
        db.unprepared("drop table app_result_items").await.unwrap();
    }
    let sql_server = connection(json!({"url":"sqlsrv://sa:fixture@localhost/unused"}));
    let rows = &[record([("code", "untouched".into())])];
    let source = sql_server.table("source");
    assert!(matches!(
        sql_server.table("dest").insertOrIgnore(rows).await,
        Err(DatabaseError::UnsupportedOperation("insert or ignore"))
    ));
    assert!(matches!(
        sql_server
            .table("dest")
            .insertOrIgnoreUsing(&["code"], &source)
            .await,
        Err(DatabaseError::UnsupportedOperation("insert or ignore"))
    ));
    if let Some(url) = mysql_url() {
        let mysql = connection(json!({"url":url}));
        assert!(matches!(
            mysql
                .table("unused")
                .insertOrIgnoreReturning(rows, &["code"], None)
                .await,
            Err(DatabaseError::UnsupportedOperation(
                "insert or ignore returning"
            ))
        ));
    }
}

#[tokio::test]
async fn update_assignments_accept_values_expressions_and_scalar_queries() {
    let sqlite = connection(json!({"driver":"sqlite","database":":memory:","prefix":"app_"}));
    let postgres = std::env::var("OPENRUST_TEST_POSTGRES_UPDATE_VALUES_URL").ok();
    assert!(
        postgres.is_some() || std::env::var_os("OPENRUST_REQUIRE_POSTGRES_UPDATE_VALUES").is_none()
    );
    let sql_server = std::env::var("OPENRUST_TEST_SQL_SERVER_URL").ok();
    assert!(sql_server.is_some() || std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
    let mut databases = vec![(sqlite, "sqlite", "text")];
    for (url, driver, json_type) in [
        (mysql_url(), "mysql", "json"),
        (
            std::env::var("OPENRUST_TEST_MARIADB_UPDATE_VALUES_URL").ok(),
            "mariadb",
            "json",
        ),
        (postgres, "pgsql", "jsonb"),
        (sql_server, "sqlsrv", "nvarchar(max)"),
    ] {
        if let Some(url) = url {
            databases.push((
                connection(json!({"url":url,"prefix":"app_"})),
                driver,
                json_type,
            ));
        }
    }

    for (db, driver, json_type) in databases {
        for sql in [
            format!("create table app_credit_accounts (id int primary key, credits int, enabled int, note varchar(40), options {json_type})"),
            "create table app_credit_events (account_id int, amount int, state varchar(40))".into(),
            "insert into app_credit_accounts values (1,0,0,'original','{}'), (2,0,0,'original','{}'), (3,0,0,'original','{}')".into(),
            "insert into app_credit_events values (1,5,'posted'), (2,4,'posted'), (2,5,'posted'), (2,99,'draft')".into(),
        ] {
            db.unprepared(&sql).await.unwrap();
        }
        let mut transaction = db.beginTransaction().await.unwrap();
        let accounts = transaction.table("credit_accounts as a");
        let credits = accounts
            .subquery("credit_events as e")
            .selectRaw("sum(amount)", vec![])
            .whereColumn("e.account_id", "=", "a.id")
            .r#where("e.state", "=", "posted");
        db.enableQueryLog();
        assert_eq!(
            accounts
                .r#where("a.id", ">", 1)
                .orderBy("a.id", "asc")
                .limit(1)
                .updateWith(BTreeMap::from([
                    ("credits".into(), UpdateValue::Subquery(&credits)),
                    ("enabled".into(), 1.into()),
                    (
                        "note".into(),
                        Expression::new("coalesce(?, 'unset')")
                            .with_bindings(vec!["posted total".into()])
                            .into()
                    ),
                ]))
                .await
                .unwrap(),
            1,
            "{driver}"
        );
        let log = &db.getQueryLog()[0];
        assert!(log.query.contains(" = (select sum(amount)"), "{driver}");
        assert!(matches!(&log.bindings, QueryBindings::Positional(values)
            if values == &["posted".into(), 1.into(), "posted total".into(), 1.into()]));
        let row = transaction
            .table("credit_accounts")
            .r#where("id", "=", 2)
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("credits"), Some(&9.into()));
        assert_eq!(row.get("enabled"), Some(&1.into()));
        assert_eq!(row.get("note"), Some(&"posted total".into()));
        assert_eq!(
            transaction
                .table("credit_accounts")
                .r#where("id", "=", 3)
                .value("credits")
                .await
                .unwrap(),
            0.into()
        );
        transaction.rollBack(None).await.unwrap();
        assert_eq!(
            db.table("credit_accounts")
                .r#where("id", "=", 2)
                .value("credits")
                .await
                .unwrap(),
            0.into()
        );

        let account = db.table("credit_accounts as a").r#where("a.id", "=", 3);
        let empty = account
            .subquery("credit_events as e")
            .select(["e.amount"])
            .whereColumn("e.account_id", "=", "a.id");
        account
            .updateWith(BTreeMap::from([(
                "credits".into(),
                UpdateValue::Subquery(&empty),
            )]))
            .await
            .unwrap();
        assert_eq!(
            db.table("credit_accounts")
                .r#where("id", "=", 3)
                .value("credits")
                .await
                .unwrap(),
            Value::Null
        );

        let foreign = connection(json!({"driver":"sqlite","database":":memory:"}));
        assert!(matches!(
            db.table("credit_accounts")
                .updateWith(BTreeMap::from([(
                    "credits".into(),
                    UpdateValue::Subquery(&foreign.table("outside"))
                ),]))
                .await,
            Err(DatabaseError::InvalidQuery("subquery connection"))
        ));

        if matches!(driver, "mysql" | "mariadb" | "pgsql") {
            let expression = if driver == "pgsql" {
                "to_jsonb(cast(? as text))"
            } else {
                "concat(?, '')"
            };
            assert_eq!(
                db.table("credit_accounts")
                    .r#where("id", "=", 2)
                    .updateWith(BTreeMap::from([
                        (
                            "options->label".into(),
                            Expression::new(expression)
                                .with_bindings(vec!["posted".into()])
                                .into()
                        ),
                        ("options->rank".into(), 2.into()),
                    ]))
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                db.table("credit_accounts")
                    .whereJsonContains("options->label", json!("posted"))
                    .value("id")
                    .await
                    .unwrap(),
                2.into()
            );
        }
        db.unprepared("drop table app_credit_events").await.unwrap();
        db.unprepared("drop table app_credit_accounts")
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn native_query_locks_skip_and_revisit_rows() {
    let sqlite = connection(json!({"driver":"sqlite","database":":memory:"}));
    assert_eq!(
        sqlite
            .table("lock_rows")
            .lock("for update skip locked")
            .toSql()
            .unwrap(),
        sqlite.table("lock_rows").toSql().unwrap()
    );
    let postgres = std::env::var("OPENRUST_TEST_POSTGRES_LOCK_URL").ok();
    assert!(postgres.is_some() || std::env::var_os("OPENRUST_REQUIRE_POSTGRES_LOCK").is_none());
    let sql_server = std::env::var("OPENRUST_TEST_SQL_SERVER_URL").ok();
    assert!(sql_server.is_some() || std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
    for (url, native, before_where) in [
        (mysql_url(), "for update skip locked", false),
        (postgres, "for update skip locked", false),
        (sql_server, "WITH (updlock, rowlock, readpast)", true),
    ] {
        let Some(url) = url else { continue };
        let db = connection(json!({"url":url, "max_connections":3, "acquire_timeout":5}));
        db.unprepared("create table lock_rows (id int primary key)")
            .await
            .unwrap();
        db.unprepared("insert into lock_rows values (1), (2)")
            .await
            .unwrap();
        if before_where {
            db.unprepared("create index idx_lock_id on lock_rows (id)")
                .await
                .unwrap();
            let combination = db.table("lock_rows").forceIndex("idx_lock_id").lock(native);
            assert!(
                combination
                    .toSql()
                    .unwrap()
                    .contains("with (updlock, rowlock, readpast, index([idx_lock_id]))")
            );
            assert_eq!(
                combination.r#where("id", "=", 1).value("id").await.unwrap(),
                1.into()
            );
            assert!(matches!(
                db.table("lock_rows")
                    .forceIndex("idx_lock_id")
                    .lock("not a hint list")
                    .toSql(),
                Err(DatabaseError::InvalidQuery(
                    "SQL Server combined table hints"
                ))
            ));
            db.unprepared("drop index idx_lock_id on lock_rows")
                .await
                .unwrap();
        }
        let default = db.table("lock_rows").lockForUpdate().toSql().unwrap();
        assert_eq!(
            db.table("lock_rows")
                .lock(native)
                .lock(true)
                .toSql()
                .unwrap(),
            default
        );
        assert_eq!(
            db.table("lock_rows").lock(false).toSql().unwrap(),
            db.table("lock_rows").sharedLock().toSql().unwrap()
        );
        let mut holder = db.beginTransaction().await.unwrap();
        assert_eq!(
            holder
                .table("lock_rows")
                .lock(true)
                .r#where("id", "=", 1)
                .value("id")
                .await
                .unwrap(),
            1.into()
        );
        let mut reader = db.beginTransaction().await.unwrap();
        let query = reader
            .table("lock_rows")
            .lock(native)
            .r#where("id", ">", 0)
            .orderBy("id", "asc");
        let sql = query.toSql().unwrap();
        assert_eq!(
            sql.find(native).unwrap() < sql.find(" where ").unwrap(),
            before_where
        );
        assert_eq!(query.getBindings().unwrap(), [0.into()]);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), query.value("id"))
                .await
                .unwrap()
                .unwrap(),
            2.into()
        );
        reader.rollBack(None).await.unwrap();
        holder.rollBack(None).await.unwrap();
        let mut reader = db.beginTransaction().await.unwrap();
        assert_eq!(
            reader
                .table("lock_rows")
                .lock(native.to_owned())
                .orderBy("id", "asc")
                .value("id")
                .await
                .unwrap(),
            1.into()
        );
        reader.commit().await.unwrap();
        db.unprepared("drop table lock_rows").await.unwrap();
    }
}

#[tokio::test]
async fn query_index_hints_follow_driver_rules() {
    let sqlite = connection(json!({"driver":"sqlite","database":":memory:"}));
    sqlite
        .unprepared(
            "create table index_items (id integer primary key, score int);
        create index idx_score on index_items (score);
        insert into index_items values (1, 7), (2, 8);",
        )
        .await
        .unwrap();
    let table = || sqlite.table("index_items as i");
    let query = table()
        .useIndex("unused")
        .forceIndex(" idx_score ")
        .r#where("i.score", "=", 7);
    assert!(
        query
            .toSql()
            .unwrap()
            .contains(r#"as "i" indexed by idx_score where"#)
    );
    assert_eq!(query.getBindings().unwrap(), [7.into()]);
    assert_eq!(query.value("i.id").await.unwrap(), 1.into());
    for query in [
        table().useIndex("ignored"),
        table().forceIndex("idx_score").ignoreIndex("ignored"),
        table().forceIndex("PRIMARY"),
        table().forceIndex("idx_score, other"),
    ] {
        assert_eq!(query.toSql().unwrap(), table().toSql().unwrap());
    }
    assert!(matches!(
        table().forceIndex("idx_score, invalid-name").toSql(),
        Err(DatabaseError::InvalidQuery("index name"))
    ));
    sqlite.enableQueryLog();
    assert_eq!(
        sqlite
            .table("index_items")
            .forceIndex("idx_score")
            .r#where("score", "=", 8)
            .limit(1)
            .update(record([("score", 9.into())]))
            .await
            .unwrap(),
        1
    );
    assert!(
        sqlite.getQueryLog()[0]
            .query
            .contains(r#"from "index_items" indexed by idx_score"#)
    );

    let postgres = connection(json!({"url":"postgres://localhost/index_hints"}));
    assert_eq!(
        postgres
            .table("index_items")
            .forceIndex("idx_score")
            .toSql()
            .unwrap(),
        postgres.table("index_items").toSql().unwrap()
    );

    for url in [
        mysql_url(),
        std::env::var("OPENRUST_TEST_MARIADB_INDEX_URL").ok(),
    ]
    .into_iter()
    .flatten()
    {
        let db = connection(json!({"url":url, "prefix":"app_"}));
        db.unprepared(
            "create table app_index_items (id int primary key, score int, index idx_score (score))",
        )
        .await
        .unwrap();
        db.unprepared("insert into app_index_items values (1, 7), (2, 8)")
            .await
            .unwrap();
        let table = || db.table("index_items as i").r#where("i.score", "=", 7);
        for (query, expected) in [
            (
                table().useIndex("PRIMARY,idx_score"),
                "use index (PRIMARY,idx_score)",
            ),
            (
                table().useIndex("PRIMARY").forceIndex("idx_score"),
                "force index (idx_score)",
            ),
            (table().ignoreIndex("idx_score"), "ignore index (idx_score)"),
        ] {
            assert!(query.toSql().unwrap().contains(expected));
            assert_eq!(query.getBindings().unwrap(), [7.into()]);
            assert_eq!(query.value("i.id").await.unwrap(), 1.into());
        }
        for invalid in ["", "idx_score,", "idx.score", "idx_score --"] {
            assert!(matches!(
                table().forceIndex(invalid).toSql(),
                Err(DatabaseError::InvalidQuery("index name"))
            ));
        }
        db.unprepared("drop table app_index_items").await.unwrap();
    }
}

#[tokio::test]
async fn sql_server_combines_index_and_lock_hints() {
    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = connection(json!({"url":url, "prefix":"app_"}));
    db.unprepared(
        "create table dbo.app_index_items (id int primary key, score int);
        create index idx_score on dbo.app_index_items (score);
        insert into dbo.app_index_items values (1, 7), (2, 8);",
    )
    .await
    .unwrap();
    let table = || db.table("dbo.index_items as i").r#where("i.score", "=", 7);
    assert_eq!(
        table().useIndex("ignored").toSql().unwrap(),
        table().toSql().unwrap()
    );
    assert_eq!(
        table().ignoreIndex("ignored").toSql().unwrap(),
        table().toSql().unwrap()
    );
    assert!(matches!(
        table().forceIndex("idx_score, another").toSql(),
        Err(DatabaseError::InvalidQuery("index name"))
    ));
    let query = table().forceIndex("idx_score");
    assert!(
        query
            .toSql()
            .unwrap()
            .contains("as [app_i] with (index([idx_score])) where")
    );
    assert_eq!(query.value("i.id").await.unwrap(), 1.into());
    let mut transaction = db.beginTransaction().await.unwrap();
    let query = transaction
        .table("dbo.index_items as i")
        .forceIndex("idx_score")
        .lockForUpdate()
        .r#where("i.score", "=", 7);
    assert!(
        query
            .toSql()
            .unwrap()
            .contains("with (rowlock, updlock, holdlock, index([idx_score]))")
    );
    assert_eq!(query.value("i.id").await.unwrap(), 1.into());
    transaction.rollBack(None).await.unwrap();
    db.unprepared("drop table dbo.app_index_items")
        .await
        .unwrap();
}

#[tokio::test]
async fn cross_and_straight_join_variants() {
    let mut connections = vec![(
        connection(json!({"driver":"sqlite","database":":memory:","prefix":"app_"})),
        false,
    )];
    if let Some(url) = mysql_url() {
        connections.push((connection(json!({"url":url, "prefix":"app_"})), true));
    }
    if let Ok(url) = std::env::var("OPENRUST_TEST_MARIADB_JOIN_URL") {
        connections.push((connection(json!({"url":url, "prefix":"app_"})), true));
    }
    for (db, mysql_family) in connections {
        for sql in [
            "CREATE TABLE app_join_orders (id int PRIMARY KEY)",
            "CREATE TABLE app_join_labels (order_id int, code varchar(20))",
            "INSERT INTO app_join_orders VALUES (1), (2), (3)",
            "INSERT INTO app_join_labels VALUES (1, 'blue'), (2, 'blue'), (3, 'red')",
        ] {
            db.unprepared(sql).await.unwrap();
        }
        let orders = || db.table("join_orders as o");
        let labels = orders()
            .subquery("join_labels")
            .r#where("code", "=", "blue");
        let cross = orders()
            .crossJoinSub(&labels, "b")
            .unwrap()
            .r#where("o.id", ">", 1)
            .orderBy("o.id", "asc");
        assert!(cross.toSql().unwrap().contains(" cross join (select"));
        assert_eq!(cross.getBindings().unwrap(), ["blue".into(), 1.into()]);
        assert_eq!(
            cross.pluck("o.id").await.unwrap(),
            [2.into(), 2.into(), 3.into(), 3.into()]
        );

        let straight = orders().straightJoin("join_labels as b", "o.id", "=", "b.order_id");
        let straight_sub = orders()
            .straightJoinSub(&labels, "b", |join| {
                join.on("o.id", "=", "b.order_id")
                    .r#where("b.order_id", ">", 1)
            })
            .unwrap()
            .r#where("o.id", "<", 4);
        if mysql_family {
            assert!(
                straight
                    .toSql()
                    .unwrap()
                    .contains(" straight_join `app_join_labels` as `app_b` on ")
            );
            assert_eq!(straight.count("*").await.unwrap(), 3);
            assert_eq!(
                straight_sub.getBindings().unwrap(),
                ["blue".into(), 1.into(), 4.into()]
            );
            assert_eq!(straight_sub.pluck("o.id").await.unwrap(), [2.into()]);
            assert_eq!(
                orders()
                    .straightJoinWhere("join_labels as b", "b.code", "=", "red")
                    .r#where("o.id", "=", 1)
                    .count("*")
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                orders()
                    .straightJoinWith("join_labels as b", |join| {
                        join.on("o.id", "=", "b.order_id")
                            .r#where("b.code", "=", "red")
                    })
                    .value("o.id")
                    .await
                    .unwrap(),
                3.into()
            );
        } else {
            assert!(matches!(
                straight.toSql(),
                Err(DatabaseError::UnsupportedOperation("straight joins"))
            ));
            assert!(matches!(
                straight_sub.get().await,
                Err(DatabaseError::UnsupportedOperation("straight joins"))
            ));
        }
        db.unprepared("DROP TABLE app_join_labels").await.unwrap();
        db.unprepared("DROP TABLE app_join_orders").await.unwrap();
    }
}

#[tokio::test]
async fn postgres_update_from_sources_conditions_and_rollback() {
    let sqlite = connection(json!({"driver":"sqlite","database":":memory:"}));
    assert!(matches!(
        sqlite
            .table("unused")
            .updateFrom(record([("score", 1.into())]))
            .await,
        Err(DatabaseError::UnsupportedOperation("updateFrom"))
    ));
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_UPDATE_FROM_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_UPDATE_FROM").is_none());
        return;
    };
    let db = connection(json!({"url":url, "prefix":"app_"}));
    db.unprepared(
        "CREATE SCHEMA update_probe;
         CREATE TABLE update_probe.app_orders (id int PRIMARY KEY, region int, score int, options jsonb);
         CREATE TABLE update_probe.app_prices (order_id int, amount int, currency text, options jsonb);
         CREATE TABLE update_probe.app_regions (id int PRIMARY KEY, name text);
         INSERT INTO update_probe.app_orders VALUES (1, 1, 0, '{}'), (2, 1, 0, '{}'), (3, 2, 0, '{}');
         INSERT INTO update_probe.app_prices VALUES (1, 10, 'USD', '{}'), (2, 8, 'USD', '{}'), (3, 15, 'USD', '{}'), (1, 40, 'EUR', '{}');
         INSERT INTO update_probe.app_regions VALUES (1, 'west'), (2, 'east');",
    ).await.unwrap();

    let mut transaction = db.beginTransaction().await.unwrap();
    let orders = transaction.table("update_probe.orders as o");
    let prices = orders
        .subquery("update_probe.prices")
        .r#where("currency", "=", "USD");
    db.enableQueryLog();
    assert_eq!(
        orders
            .selectRaw("?", vec![999.into()])
            .joinSub(&prices, "p", |join| join
                .on("o.id", "=", "p.order_id")
                .r#where("p.amount", ">", 9))
            .unwrap()
            .joinWith("update_probe.regions as r", |join| join
                .on("o.region", "=", "r.id")
                .r#where("r.name", "=", "west"))
            .r#where("o.id", ">", 0)
            .limit(0)
            .updateFrom(BTreeMap::from([
                (
                    "o.score".into(),
                    ValueOperand::from(
                        Expression::new(r#""app_p"."amount" + ?"#).with_bindings(vec![2.into()])
                    )
                ),
                ("o.options->active".into(), true.into()),
                (
                    "o.options->note".into(),
                    Expression::new("to_jsonb(cast(? as text))")
                        .with_bindings(vec!["checked".into()])
                        .into()
                ),
            ]))
            .await
            .unwrap(),
        1
    );
    let logged = &db.getQueryLog()[0];
    assert!(logged.query.contains(r#"update "update_probe"."app_orders" as "app_o" set "score" = "app_p"."amount" + ?, "options" = jsonb_set(jsonb_set("app_o"."options"::jsonb"#));
    assert!(logged.query.contains(r#"from (select * from "update_probe"."app_prices" where "currency" = ?) as "app_p", "update_probe"."app_regions" as "app_r" where "app_o"."id" > ? and "app_o"."id" = "app_p"."order_id""#));
    assert!(
        matches!(&logged.bindings, QueryBindings::Positional(values) if values == &[
            2.into(), "active".into(), Value::Json(json!(true)), "note".into(), "checked".into(),
            "USD".into(), 0.into(), 9.into(), "west".into()
        ])
    );
    let updated = transaction
        .table("update_probe.orders")
        .r#where("id", "=", 1)
        .get()
        .await
        .unwrap();
    assert_eq!(updated[0].get("score"), Some(&12.into()));
    assert_eq!(
        updated[0].get("options"),
        Some(&Value::Json(json!({"active": true, "note": "checked"})))
    );
    transaction.rollBack(None).await.unwrap();
    assert_eq!(
        db.table("update_probe.orders")
            .r#where("score", "=", 0)
            .count("*")
            .await
            .unwrap(),
        3
    );

    assert_eq!(
        db.table("update_probe.orders as o")
            .joinWith("update_probe.regions as r", |join| join
                .on("o.region", "=", "r.id")
                .r#where("r.name", "=", "east"))
            .updateFrom(record([("score", 5.into())]))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table("update_probe.orders")
            .r#where("score", "=", 5)
            .value("id")
            .await
            .unwrap(),
        3.into()
    );
    assert_eq!(
        db.table("update_probe.orders as o")
            .joinWith("update_probe.regions as r", |join| join
                .orOn("o.region", "=", "r.id")
                .r#where("r.name", "=", "east"))
            .r#where("o.id", "=", 999)
            .updateFrom(record([("score", 7.into())]))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table("update_probe.orders")
            .r#where("score", "=", 7)
            .value("id")
            .await
            .unwrap(),
        3.into()
    );
    assert_eq!(
        db.table("update_probe.orders")
            .r#where("id", "=", 2)
            .updateFrom(record([("score", 6.into())]))
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        db.table("update_probe.orders")
            .updateFrom(Record::new())
            .await,
        Err(DatabaseError::InvalidQuery("update assignments"))
    ));
    db.unprepared("DROP SCHEMA update_probe CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn correlated_lateral_joins_on_postgres_and_sql_server() {
    for (url_var, required_var, inner_operator, left_operator) in [
        (
            "OPENRUST_TEST_POSTGRES_LATERAL_URL",
            "OPENRUST_REQUIRE_POSTGRES_LATERAL",
            "inner join lateral",
            "left join lateral",
        ),
        (
            "OPENRUST_TEST_SQL_SERVER_URL",
            "OPENRUST_REQUIRE_SQL_SERVER",
            "cross apply",
            "outer apply",
        ),
    ] {
        let Ok(url) = std::env::var(url_var) else {
            assert!(std::env::var_os(required_var).is_none());
            continue;
        };
        let db = connection(json!({"url":url, "prefix":"app_"}));
        db.unprepared(
            "CREATE TABLE app_lateral_groups (id int PRIMARY KEY);
             CREATE TABLE app_lateral_items (id int PRIMARY KEY, group_id int, score int);
             INSERT INTO app_lateral_groups VALUES (1), (2), (3);
             INSERT INTO app_lateral_items VALUES (11, 1, 7), (12, 1, 8), (21, 2, 9), (31, 3, 3);",
        )
        .await
        .unwrap();

        let groups = || {
            db.table("lateral_groups as g")
                .select(["g.id", "best.item_id"])
                .selectRaw("cast(? as int) as marker", vec![123.into()])
                .r#where("g.id", ">", 0)
                .orderBy("g.id", "asc")
        };
        let items = groups()
            .subquery("lateral_items as i")
            .select(["i.id as item_id"])
            .whereColumn("i.group_id", "=", "g.id")
            .r#where("i.score", ">", 5)
            .orderByDesc("i.score")
            .limit(1);

        let inner = groups().joinLateral(&items, "best").unwrap();
        let sql = inner.toSql().unwrap();
        assert!(sql.contains(inner_operator));
        assert_eq!(
            sql.contains(" on true"),
            inner_operator == "inner join lateral"
        );
        assert_eq!(
            inner.getBindings().unwrap(),
            [123.into(), 5.into(), 0.into()]
        );
        assert_eq!(
            inner.pluck("best.item_id").await.unwrap(),
            [12.into(), 21.into()]
        );

        let left = groups().leftJoinLateral(&items, "best").unwrap();
        assert!(left.toSql().unwrap().contains(left_operator));
        let rows = left.get().await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].get("id"), Some(&3.into()));
        assert_eq!(rows[2].get("item_id"), Some(&Value::Null));
        assert_eq!(rows[2].get("marker"), Some(&123.into()));

        let other = connection(json!({"url":url, "prefix":"app_"}));
        assert!(matches!(
            groups().joinLateral(&other.table("lateral_items"), "best"),
            Err(DatabaseError::InvalidQuery("subquery connection"))
        ));

        let mut transaction = db.beginTransaction().await.unwrap();
        transaction
            .insert("INSERT INTO app_lateral_items VALUES (32, 3, 12)", &[])
            .await
            .unwrap();
        let group = transaction
            .table("lateral_groups as g")
            .r#where("g.id", "=", 3);
        let item = group
            .subquery("lateral_items as i")
            .select(["i.id as item_id"])
            .whereColumn("i.group_id", "=", "g.id")
            .orderByDesc("i.score")
            .limit(1);
        assert_eq!(
            group
                .joinLateral(&item, "best")
                .unwrap()
                .value("best.item_id")
                .await
                .unwrap(),
            32.into()
        );
        transaction.rollBack(None).await.unwrap();
        assert_eq!(
            db.table("lateral_items")
                .r#where("id", "=", 32)
                .count("*")
                .await
                .unwrap(),
            0
        );
        db.unprepared("DROP TABLE app_lateral_items; DROP TABLE app_lateral_groups")
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn sql_server_joined_and_bounded_writes() {
    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = connection(json!({"url":url, "prefix":"app_"}));
    db.unprepared(
        "CREATE TABLE dbo.app_write_probe (id int PRIMARY KEY, group_id int, score int);
         CREATE TABLE dbo.app_write_groups (id int PRIMARY KEY, active int);
         INSERT INTO dbo.app_write_groups VALUES (1, 1), (2, 0);
         INSERT INTO dbo.app_write_probe VALUES (1, 1, 0), (2, 2, 0), (3, 1, 0), (4, 2, 0), (5, 1, 0);",
    ).await.unwrap();

    let table = || db.table("dbo.write_probe");
    let joined = || {
        db.table("dbo.write_probe AS p")
            .joinWith("dbo.write_groups as g", |join| {
                join.on("p.group_id", "=", "g.id")
                    .r#where("g.active", "=", 1)
            })
            .r#where("p.id", ">", 1)
    };

    db.enableQueryLog();
    assert_eq!(
        joined()
            .selectRaw("?", vec![99.into()])
            .limit(1)
            .increment("p.score", 7, record([("p.group_id", 1.into())]))
            .await
            .unwrap(),
        2
    );
    let logged = &db.getQueryLog()[0];
    assert!(logged.query.starts_with("update [app_p] set [score] = [app_p].[score] + ?, [group_id] = ? from [dbo].[app_write_probe] as [app_p] inner join "));
    assert!(!logged.query.contains("top ("));
    assert!(matches!(&logged.bindings, QueryBindings::Positional(values)
        if values == &[7.into(), 1.into(), 1.into(), 1.into()]));

    db.flushQueryLog();
    assert_eq!(
        table()
            .join(
                "dbo.write_groups",
                "write_probe.group_id",
                "=",
                "write_groups.id"
            )
            .r#where("write_probe.id", "=", 3)
            .update(record([("score", 8.into())]))
            .await
            .unwrap(),
        1
    );
    assert!(
        db.getQueryLog()[0]
            .query
            .starts_with("update [dbo].[app_write_probe] set ")
    );

    db.flushQueryLog();
    assert_eq!(
        table()
            .r#where("id", ">", 2)
            .orderByRaw(
                "case when [id] = ? then 0 else 1 end, [id] desc",
                vec![3.into()]
            )
            .limit(1)
            .update(record([("score", 9.into())]))
            .await
            .unwrap(),
        1
    );
    let logged = &db.getQueryLog()[0];
    assert_eq!(
        logged.query,
        "update [app_write_probe] set [score] = ? from (select top (1) * from [dbo].[app_write_probe] where [id] > ? order by case when [id] = ? then 0 else 1 end, [id] desc) as [app_write_probe]"
    );
    assert!(matches!(&logged.bindings, QueryBindings::Positional(values)
        if values == &[9.into(), 2.into(), 3.into()]));
    assert_eq!(
        table().r#where("score", "=", 9).value("id").await.unwrap(),
        3.into()
    );

    assert_eq!(
        table()
            .r#where("group_id", "=", 2)
            .limit(2)
            .update(record([("score", 6.into())]))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .r#where("score", "=", 6)
            .limit(1)
            .offset(3)
            .update(record([("score", 8.into())]))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .r#where("score", "=", 8)
            .limit(0)
            .update(record([("score", 4.into())]))
            .await
            .unwrap(),
        2
    );

    let mut transaction = db.beginTransaction().await.unwrap();
    assert_eq!(
        transaction
            .table("dbo.write_probe as p")
            .r#where("p.id", ">", 1)
            .orderBy("p.id", "desc")
            .limit(1)
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        transaction
            .table("dbo.write_probe")
            .r#where("id", "=", 5)
            .count("*")
            .await
            .unwrap(),
        0
    );
    transaction.rollBack(None).await.unwrap();
    assert_eq!(table().count("*").await.unwrap(), 5);

    assert_eq!(joined().delete().await.unwrap(), 2);
    assert_eq!(
        db.table("dbo.write_probe as p")
            .r#where("p.score", "=", 4)
            .limit(1)
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        table()
            .r#where("score", "=", 4)
            .limit(1)
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(table().value("id").await.unwrap(), 1.into());
    db.unprepared("DROP TABLE dbo.app_write_probe; DROP TABLE dbo.app_write_groups")
        .await
        .unwrap();
}

#[tokio::test]
async fn sql_server_truncate_and_json_write_boundary() {
    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    let db = connection(json!({"url":url, "prefix":"app_"}));
    db.unprepared("CREATE SCHEMA truncate_probe").await.unwrap();
    db.statement(
        "CREATE TABLE truncate_probe.app_events (id int IDENTITY(51, 1) PRIMARY KEY, label nvarchar(40), options nvarchar(max))",
        &[],
    ).await.unwrap();
    db.statement("CREATE TABLE dbo.app_events (id int PRIMARY KEY)", &[])
        .await
        .unwrap();
    db.insert("INSERT INTO dbo.app_events VALUES (71)", &[])
        .await
        .unwrap();

    let events = "truncate_probe.events";
    let insert = || {
        record([
            ("label", "seed".into()),
            ("options", Value::Json(json!({"enabled":false}))),
        ])
    };
    assert_eq!(
        db.table(events).insertGetId(insert()).await.unwrap(),
        51.into()
    );
    assert_eq!(
        db.table(events).insertGetId(insert()).await.unwrap(),
        52.into()
    );

    db.enableQueryLog();
    assert!(matches!(
        db.table(events)
            .r#where("id", "=", 51)
            .update(record([("options->enabled", true.into())]))
            .await,
        Err(DatabaseError::UnsupportedOperation(
            "SQL Server JSON path update"
        ))
    ));
    assert!(db.getQueryLog().is_empty());
    assert_eq!(
        db.table(events)
            .r#where("id", "=", 51)
            .update(record([("options", Value::Json(json!({"enabled":true})))]))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table(events)
            .whereJsonContains("options", json!(true))
            .value("id")
            .await
            .unwrap(),
        51.into()
    );

    db.statement(
        "CREATE TABLE truncate_probe.app_children (event_id int REFERENCES truncate_probe.app_events (id))",
        &[],
    ).await.unwrap();
    let blocked = db.table(events).truncate().await.unwrap_err();
    assert!(matches!(
        blocked, DatabaseError::SqlServer { source: tiberius::error::Error::Server(error), .. }
            if error.code() == 4712
    ));
    assert_eq!(db.table(events).count("*").await.unwrap(), 2);
    db.statement("DROP TABLE truncate_probe.app_children", &[])
        .await
        .unwrap();

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction.table(events).truncate().await.unwrap();
    assert_eq!(transaction.table(events).count("*").await.unwrap(), 0);
    transaction.rollBack(None).await.unwrap();
    assert_eq!(db.table(events).count("*").await.unwrap(), 2);
    assert_eq!(
        db.table(events).insertGetId(insert()).await.unwrap(),
        53.into()
    );

    db.flushQueryLog();
    db.table(events)
        .r#where("id", "=", 51)
        .truncate()
        .await
        .unwrap();
    assert_eq!(
        db.getQueryLog()[0].query,
        "truncate table [truncate_probe].[app_events]"
    );
    assert_eq!(db.table(events).count("*").await.unwrap(), 0);
    assert_eq!(db.table("dbo.events").value("id").await.unwrap(), 71.into());
    assert_eq!(
        db.table(events).insertGetId(insert()).await.unwrap(),
        51.into()
    );
    assert!(matches!(
        db.table("truncate_probe.events as e").truncate().await,
        Err(DatabaseError::InvalidQuery("truncate table alias"))
    ));
    db.unprepared("DROP TABLE truncate_probe.app_events; DROP TABLE dbo.app_events; DROP SCHEMA truncate_probe")
        .await.unwrap();
}

#[tokio::test]
async fn postgres_truncate_restarts_identities_with_configured_cascade_and_rollback() {
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_TRUNCATE_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES_TRUNCATE").is_none());
        return;
    };
    let db = connection(json!({"url":url, "prefix":"app_"}));
    let restrict = connection(json!({"url":url, "prefix":"app_", "cascade_on_truncate":false}));
    db.statement("CREATE SCHEMA truncate_probe", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE truncate_probe.app_parents (id integer GENERATED ALWAYS AS IDENTITY (START WITH 11) PRIMARY KEY, label text NOT NULL)",
        &[],
    ).await.unwrap();
    db.statement(
        "CREATE TABLE truncate_probe.app_children (id integer GENERATED ALWAYS AS IDENTITY (START WITH 101) PRIMARY KEY, parent_id integer NOT NULL REFERENCES truncate_probe.app_parents (id))",
        &[],
    ).await.unwrap();
    db.statement("CREATE TABLE public.app_parents (id integer)", &[])
        .await
        .unwrap();
    db.insert("INSERT INTO public.app_parents VALUES (71)", &[])
        .await
        .unwrap();
    db.insert(
        "INSERT INTO truncate_probe.app_parents (label) VALUES ('one'), ('two')",
        &[],
    )
    .await
    .unwrap();
    db.insert(
        "INSERT INTO truncate_probe.app_children (parent_id) VALUES (11), (12)",
        &[],
    )
    .await
    .unwrap();

    let parents = "truncate_probe.parents";
    let children = "truncate_probe.children";
    let error = restrict.table(parents).truncate().await.unwrap_err();
    let DatabaseError::Driver { source, .. } = error else {
        panic!("expected PostgreSQL driver error, got {error}");
    };
    assert_eq!(
        source
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("0A000"),
    );
    assert_eq!(db.table(parents).count("*").await.unwrap(), 2);
    assert_eq!(db.table(children).count("*").await.unwrap(), 2);

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction.table(parents).truncate().await.unwrap();
    assert_eq!(transaction.table(parents).count("*").await.unwrap(), 0);
    assert_eq!(transaction.table(children).count("*").await.unwrap(), 0);
    assert_eq!(
        transaction
            .table(parents)
            .insertGetId(record([("label", "rolled back".into())]))
            .await
            .unwrap(),
        11.into(),
    );
    transaction.rollBack(None).await.unwrap();
    assert_eq!(db.table(parents).count("*").await.unwrap(), 2);
    assert_eq!(db.table(children).count("*").await.unwrap(), 2);
    assert_eq!(
        db.table(parents)
            .insertGetId(record([("label", "next".into())]))
            .await
            .unwrap(),
        13.into(),
    );

    db.enableQueryLog();
    db.table(parents).truncate().await.unwrap();
    assert_eq!(
        db.getQueryLog()[0].query,
        r#"truncate table "truncate_probe"."app_parents" restart identity cascade"#,
    );
    assert_eq!(db.table(parents).count("*").await.unwrap(), 0);
    assert_eq!(db.table(children).count("*").await.unwrap(), 0);
    assert_eq!(
        db.table("public.parents").value("id").await.unwrap(),
        71.into()
    );
    assert_eq!(
        db.table(parents)
            .insertGetId(record([("label", "restart".into())]))
            .await
            .unwrap(),
        11.into(),
    );
    assert_eq!(
        db.table(children)
            .insertGetId(record([("parent_id", 11.into())]))
            .await
            .unwrap(),
        101.into(),
    );
    assert!(matches!(
        db.table("truncate_probe.parents as p").truncate().await,
        Err(DatabaseError::InvalidQuery("truncate table alias"))
    ));
    db.statement("DROP SCHEMA truncate_probe CASCADE", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE public.app_parents", &[])
        .await
        .unwrap();
}

async fn verify(db: Connection, mysql: bool) {
    db.unprepared("DROP TABLE IF EXISTS app_q_items; DROP TABLE IF EXISTS app_q_groups; DROP TABLE IF EXISTS app_q_archive").await.unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    db.statement(&format!("CREATE TABLE app_q_items (id {id}, code VARCHAR(20) UNIQUE, group_id BIGINT, stock BIGINT DEFAULT 0)"), &[]).await.unwrap();
    db.unprepared("CREATE TABLE app_q_groups (id BIGINT PRIMARY KEY, region VARCHAR(20)); CREATE TABLE app_q_archive (code VARCHAR(20) PRIMARY KEY); INSERT INTO app_q_groups VALUES (1, 'east'), (2, 'west')").await.unwrap();
    let items = || db.table("q_items");

    let id = items()
        .insertGetId(record([
            ("code", "a".into()),
            ("group_id", 1.into()),
            ("stock", 4.into()),
        ]))
        .await
        .unwrap();
    assert_eq!(id, 1.into());
    assert_eq!(
        items()
            .insertOrIgnore(&[record([("code", "a".into())])])
            .await
            .unwrap(),
        0
    );
    assert!(
        items()
            .insert(&[
                record([
                    ("code", "b".into()),
                    ("group_id", 2.into()),
                    ("stock", 8.into())
                ]),
                record([
                    ("stock", 12.into()),
                    ("group_id", 1.into()),
                    ("code", "c".into())
                ]),
            ])
            .await
            .unwrap()
    );
    assert!(matches!(
        items()
            .insert(&[
                record([("code", "bad".into())]),
                record([("stock", 3.into())])
            ])
            .await,
        Err(DatabaseError::InvalidQuery("insert row columns"))
    ));
    assert!(
        items()
            .upsert(
                &[record([("code", "a".into()), ("stock", 6.into())])],
                &["code"],
                Some(&["stock"])
            )
            .await
            .unwrap()
            >= 1
    );
    assert_eq!(
        items()
            .r#where("code", "=", "a")
            .value("stock")
            .await
            .unwrap(),
        6.into()
    );
    assert_eq!(
        items()
            .r#where("code", "=", "a")
            .increment("stock", 3, Record::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        items()
            .r#where("code", "=", "a")
            .decrement("stock", 1, Record::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        items()
            .r#where("code", "=", "a")
            .value("stock")
            .await
            .unwrap(),
        8.into()
    );

    assert!(
        items()
            .updateOrInsert(
                record([("code", "extra".into())]),
                record([("stock", 1.into())])
            )
            .await
            .unwrap()
    );
    assert!(
        items()
            .updateOrInsertWith(record([("code", "extra".into())]), |exists| {
                assert!(exists);
                record([("stock", 2.into())])
            })
            .await
            .unwrap()
    );
    assert_eq!(
        items()
            .r#where("code", "=", "extra")
            .value("stock")
            .await
            .unwrap(),
        2.into()
    );
    assert_eq!(
        items()
            .r#where("code", "=", "extra")
            .delete()
            .await
            .unwrap(),
        1
    );

    let json_type = if mysql { "JSON" } else { "TEXT" };
    db.statement(
        &format!("ALTER TABLE app_q_items ADD COLUMN options {json_type}"),
        &[],
    )
    .await
    .unwrap();
    items()
        .r#where("code", "=", "a")
        .update(record([(
            "options",
            Value::Json(json!({"prefs":{"existing":true}, "items":[0,1]})),
        )]))
        .await
        .unwrap();
    let mut changes = record([
        ("options->prefs->label", "O'Reilly".into()),
        ("options->prefs->enabled", true.into()),
    ]);
    if mysql {
        changes.insert("options->items[0]".to_owned(), 9.into());
    }
    assert_eq!(
        items()
            .r#where("code", "=", "a")
            .update(changes)
            .await
            .unwrap(),
        1
    );
    let options = match items()
        .r#where("code", "=", "a")
        .value("options")
        .await
        .unwrap()
    {
        Value::Json(value) => value,
        Value::Text(value) => serde_json::from_str(&value).unwrap(),
        value => panic!("unexpected JSON result {value:?}"),
    };
    assert_eq!(
        options["prefs"],
        json!({"existing":true, "enabled":true, "label":"O'Reilly"})
    );
    assert_eq!(options["items"][0], json!(if mysql { 9 } else { 0 }));

    let joined = db
        .table("q_items as i")
        .joinWith("q_groups as g", |join| {
            join.on("i.group_id", "=", "g.id")
                .r#where("g.region", "=", "east")
        })
        .r#where("i.stock", ">", 6)
        .orderBy("i.code", "asc");
    assert_eq!(joined.getBindings().unwrap(), ["east".into(), 6.into()]);
    assert_eq!(
        joined.pluck("i.code").await.unwrap(),
        ["a".into(), "c".into()]
    );

    let base = db.table("q_items as i");
    let sub = base
        .subquery("q_groups as g")
        .select(["g.id"])
        .r#where("g.region", "=", "east");
    assert_eq!(
        base.joinSub(&sub, "s", |join| join.on("i.group_id", "=", "s.id"))
            .unwrap()
            .orderBy("i.code", "asc")
            .pluck("i.code")
            .await
            .unwrap(),
        ["a".into(), "c".into()]
    );

    let base = db.table("q_items as i");
    let exists = base
        .subquery("q_groups as g")
        .whereColumn("g.id", "=", "i.group_id")
        .r#where("g.region", "=", "west");
    assert_eq!(
        base.whereExists(&exists)
            .unwrap()
            .pluck("i.code")
            .await
            .unwrap(),
        ["b".into()]
    );

    let base = items().select(["code"]).r#where("stock", "=", 8).limit(1);
    let other = base
        .subquery("q_items")
        .select(["code"])
        .r#where("code", "=", "c");
    let union = base
        .orderBy("code", "asc")
        .unionAll(&other)
        .unwrap()
        .orderBy("code", "desc");
    assert_eq!(union.getBindings().unwrap(), [8.into(), "c".into()]);
    assert_eq!(
        union.limit(2).pluck("code").await.unwrap(),
        ["c".into(), "a".into()]
    );

    let base = items();
    let inner = base
        .subquery("q_items")
        .select(["code"])
        .r#where("stock", ">", 8);
    let outer = base.fromSub(&inner, "high").unwrap().select(["high.code"]);
    assert_eq!(outer.getBindings().unwrap(), [8.into()]);
    assert_eq!(outer.get().await.unwrap()[0].get("code"), Some(&"c".into()));

    let base = db.table("q_groups as g");
    let inner = base
        .subquery("q_items as i")
        .select(["i.code"])
        .whereColumn("i.group_id", "=", "g.id")
        .orderByDesc("i.stock")
        .limit(1);
    if mysql {
        assert_eq!(
            base.joinLateral(&inner, "top_item")
                .unwrap()
                .orderBy("g.id", "asc")
                .pluck("top_item.code")
                .await
                .unwrap(),
            ["c".into(), "b".into()]
        );
    } else {
        assert!(matches!(
            base.joinLateral(&inner, "top_item"),
            Err(DatabaseError::InvalidQuery("lateral join dialect"))
        ));
    }

    let foreign = connection(json!({"driver":"sqlite","database":":memory:"})).table("q_items");
    assert!(matches!(
        items().whereExists(&foreign),
        Err(DatabaseError::InvalidQuery("subquery connection"))
    ));

    let archive = db.table("q_archive");
    let source = archive
        .subquery("q_items")
        .select(["code"])
        .r#where("code", "=", "a");
    assert_eq!(archive.insertUsing(&["code"], &source).await.unwrap(), 1);
    assert_eq!(
        db.table("q_archive")
            .insertOrIgnoreUsing(&["code"], &source)
            .await
            .unwrap(),
        0
    );

    let mut tx = db.beginTransaction().await.unwrap();
    let inserted = tx
        .table("q_items")
        .insertGetId(record([("code", "tx".into()), ("group_id", 2.into())]))
        .await
        .unwrap();
    assert_ne!(inserted, 0.into());
    let lock_sql = tx.table("q_items").lockForUpdate().toSql().unwrap();
    assert_eq!(lock_sql.ends_with(" for update"), mysql);
    assert!(
        tx.table("q_items")
            .r#where("id", "=", inserted)
            .lockForUpdate()
            .exists()
            .await
            .unwrap()
    );
    assert_eq!(
        tx.table("q_items as i")
            .join("q_groups as g", "i.group_id", "=", "g.id")
            .r#where("g.region", "=", "west")
            .update(record([("i.stock", 10.into())]))
            .await
            .unwrap(),
        2
    );
    tx.rollBack(None).await.unwrap();
    assert!(
        items()
            .r#where("code", "=", "tx")
            .doesntExist()
            .await
            .unwrap()
    );
    assert_eq!(
        items()
            .r#where("code", "=", "b")
            .value("stock")
            .await
            .unwrap(),
        8.into()
    );
    assert_eq!(
        db.table("q_items as i")
            .join("q_groups as g", "i.group_id", "=", "g.id")
            .r#where("g.region", "=", "west")
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        items()
            .orderBy("code", "desc")
            .limit(1)
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(items().pluck("code").await.unwrap(), ["a".into()]);
    let mut tx = db.beginTransaction().await.unwrap();
    if mysql {
        assert!(matches!(
            tx.table("q_items").truncate().await,
            Err(DatabaseError::InvalidQuery(
                "MySQL truncate implicitly commits"
            ))
        ));
    } else {
        tx.table("q_items").truncate().await.unwrap();
        assert_eq!(tx.table("q_items").count("*").await.unwrap(), 0);
    }
    tx.rollBack(None).await.unwrap();
    assert_eq!(items().count("*").await.unwrap(), 1);
    if !mysql {
        let mut tx = db.beginTransaction().await.unwrap();
        tx.unprepared("CREATE TEMP TABLE app_q_items (id INTEGER PRIMARY KEY AUTOINCREMENT, code TEXT); INSERT INTO app_q_items VALUES (10, 'temporary')").await.unwrap();
        tx.table("q_items").truncate().await.unwrap();
        assert_eq!(
            tx.table("q_items")
                .insertGetId(record([("code", "temp reset".into())]))
                .await
                .unwrap(),
            1.into()
        );
        assert_eq!(
            tx.table("main.q_items").value("code").await.unwrap(),
            "a".into()
        );
        tx.rollBack(None).await.unwrap();
    }
    items().truncate().await.unwrap();
    assert_eq!(
        items()
            .insertGetId(record([("code", "reset".into())]))
            .await
            .unwrap(),
        1.into()
    );
}

#[tokio::test]
async fn advanced_queries_and_writes_on_sqlite() {
    verify(
        connection(json!({"driver":"sqlite","database":":memory:","prefix":"app_"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn advanced_queries_and_writes_on_mysql_8_4() {
    if let Some(url) = mysql_url() {
        verify(
            connection(json!({"url":url,"prefix":"app_","use_upsert_alias":true})),
            true,
        )
        .await;
    }
}

#[tokio::test]
async fn mysql_pessimistic_locks_block_a_competing_writer_until_commit() {
    let Some(url) = mysql_url() else {
        return;
    };
    let db = connection(json!({"url":url,"max_connections":3,"acquire_timeout":5}));
    db.unprepared("DROP TABLE IF EXISTS query_lock_probe; CREATE TABLE query_lock_probe (id BIGINT PRIMARY KEY, n BIGINT); INSERT INTO query_lock_probe VALUES (1, 0)").await.unwrap();
    for shared in [false, true] {
        let mut holder = db.beginTransaction().await.unwrap();
        let mut waiter = db.beginTransaction().await.unwrap();
        waiter
            .unprepared("SET SESSION innodb_lock_wait_timeout = 1")
            .await
            .unwrap();
        let query = holder.table("query_lock_probe").r#where("id", "=", 1);
        let query = if shared {
            query.sharedLock()
        } else {
            query.lockForUpdate()
        };
        assert!(query.first().await.unwrap().is_some());
        let result = waiter
            .table("query_lock_probe")
            .r#where("id", "=", 1)
            .increment("n", 1, Record::new())
            .await;
        let Err(DatabaseError::Driver { source, .. }) = result else {
            panic!("expected row lock failure");
        };
        assert_eq!(
            source
                .as_database_error()
                .unwrap()
                .downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                .number(),
            1205
        );
        holder.commit().await.unwrap();
        assert_eq!(
            waiter
                .table("query_lock_probe")
                .r#where("id", "=", 1)
                .increment("n", 1, Record::new())
                .await
                .unwrap(),
            1
        );
        waiter.commit().await.unwrap();
    }
    assert_eq!(
        db.table("query_lock_probe").value("n").await.unwrap(),
        2.into()
    );
}

async fn verify_upsert_assignments(db: Connection) {
    db.unprepared("CREATE TABLE q_upsert_assignments (code VARCHAR(20) PRIMARY KEY, stock INTEGER, note VARCHAR(40) DEFAULT 'new', history INTEGER DEFAULT 4)")
        .await
        .unwrap();
    let table = || db.table("q_upsert_assignments");
    table()
        .insert(&[record([("code", "a".into()), ("stock", 1.into())])])
        .await
        .unwrap();
    db.enableQueryLog();
    let assignments = [
        UpsertUpdate::FromInsert("stock"),
        UpsertUpdate::Value("note", "customer's stock".into()),
        UpsertUpdate::Expression(
            "history",
            Expression::new("history + ?").with_bindings(vec![3.into()]),
        ),
    ];
    table()
        .upsertWith(
            &[
                record([("stock", 9.into()), ("code", "a".into())]),
                record([("code", "b".into()), ("stock", 5.into())]),
            ],
            &["code"],
            &assignments,
        )
        .await
        .unwrap();
    let entries = db.getQueryLog();
    assert!(matches!(
        &entries[0].bindings,
        QueryBindings::Positional(values) if values == &[
            "a".into(), 9.into(), "b".into(), 5.into(), "customer's stock".into(), 3.into()
        ]
    ));
    let rows = table().orderBy("code", "asc").get().await.unwrap();
    assert_eq!(rows[0].get("stock"), Some(&9.into()));
    assert_eq!(rows[0].get("note"), Some(&"customer's stock".into()));
    assert_eq!(rows[0].get("history"), Some(&7.into()));
    assert_eq!(rows[1].get("stock"), Some(&5.into()));
    assert_eq!(rows[1].get("note"), Some(&"new".into()));
    assert_eq!(rows[1].get("history"), Some(&4.into()));

    let a = record([("code", "a".into()), ("stock", 13.into())]);
    let mut tx = db.beginTransaction().await.unwrap();
    tx.table("q_upsert_assignments")
        .upsertWith(
            std::slice::from_ref(&a),
            &["code"],
            &[UpsertUpdate::Value("note", Value::Null)],
        )
        .await
        .unwrap();
    let row = tx
        .table("q_upsert_assignments")
        .r#where("code", "=", "a")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("stock"), Some(&9.into()));
    assert_eq!(row.get("note"), Some(&Value::Null));
    tx.rollBack(None).await.unwrap();
    assert_eq!(
        table()
            .r#where("code", "=", "a")
            .value("note")
            .await
            .unwrap(),
        "customer's stock".into()
    );

    assert!(matches!(
        table()
            .upsertWith(
                std::slice::from_ref(&a),
                &["code"],
                &[UpsertUpdate::FromInsert("note")]
            )
            .await,
        Err(DatabaseError::InvalidQuery("upsert update columns"))
    ));
    assert!(matches!(
        table().upsertWith(&[], &[], &[]).await,
        Err(DatabaseError::InvalidQuery("upsert conflict columns"))
    ));
    assert_eq!(
        table()
            .upsertWith(&[], &["code"], &assignments)
            .await
            .unwrap(),
        0
    );
    assert!(table().upsertWith(&[a], &["code"], &[]).await.is_err());
    db.unprepared("DROP TABLE q_upsert_assignments")
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_upsert_applies_mixed_assignments_only_on_conflict() {
    verify_upsert_assignments(connection(json!({"driver":"sqlite","database":":memory:"}))).await;
}

#[tokio::test]
async fn mysql_family_reports_changed_rows_by_default() {
    let mysql = mysql_url();
    let mariadb = std::env::var("OPENRUST_TEST_MARIADB_AFFECTED_ROWS_URL").ok();
    assert!(
        mariadb.is_some() || std::env::var_os("OPENRUST_REQUIRE_MARIADB_AFFECTED_ROWS").is_none()
    );

    for url in mysql.iter().chain(&mariadb) {
        let db = connection(json!({"url": url}));
        let matched = connection(json!({"url": url, "found_rows": true}));
        db.unprepared(
            "create table q_affected_rows (id integer primary key, stock integer not null)",
        )
        .await
        .unwrap();
        let item = |stock| record([("id", 1.into()), ("stock", Value::Integer(stock))]);
        let table = || db.table("q_affected_rows");
        let stock = |value| record([("stock", Value::Integer(value))]);
        let upsert = |row| async { table().upsert(&[row], &["id"], Some(&["stock"])).await };
        assert_eq!(upsert(item(10)).await.unwrap(), 1);
        assert_eq!(upsert(item(10)).await.unwrap(), 0);
        assert_eq!(upsert(item(11)).await.unwrap(), 2);
        assert_eq!(
            table()
                .r#where("id", "=", 2)
                .update(stock(11))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            table()
                .r#where("id", "=", 1)
                .update(stock(11))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            table()
                .r#where("id", "=", 1)
                .update(stock(12))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            matched
                .table("q_affected_rows")
                .r#where("id", "=", 1)
                .update(stock(12))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            matched
                .table("q_affected_rows")
                .upsert(&[item(12)], &["id"], Some(&["stock"]))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            db.affectingStatement(
                "update q_affected_rows set stock = ? where id = ?",
                &[12.into(), 1.into()]
            )
            .await
            .unwrap(),
            0
        );
        let mut tx = db.beginTransaction().await.unwrap();
        assert_eq!(
            tx.table("q_affected_rows")
                .r#where("id", "=", 1)
                .update(stock(12))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            tx.table("q_affected_rows")
                .r#where("id", "=", 1)
                .update(stock(13))
                .await
                .unwrap(),
            1
        );
        tx.rollBack(None).await.unwrap();
        assert_eq!(
            table().r#where("id", "=", 1).value("stock").await.unwrap(),
            12.into()
        );
        db.unprepared("drop table q_affected_rows").await.unwrap();
    }
}

#[tokio::test]
async fn mysql_upsert_applies_mixed_assignments_with_either_insert_reference() {
    let Some(url) = mysql_url() else {
        return;
    };
    for alias in [false, true] {
        verify_upsert_assignments(connection(json!({"url":url,"use_upsert_alias":alias}))).await;
    }
}

#[tokio::test]
async fn mysql_qualifies_subqueries_from_another_schema_without_executing_the_inner_connection() {
    let Some(url) = mysql_url() else {
        return;
    };
    let db = connection(json!({"url":url}));
    let mut catalog_url = url::Url::parse(&url).unwrap();
    catalog_url.set_path("/information_schema");
    catalog_url
        .set_password(Some("unused-inner-password"))
        .unwrap();
    let catalog = connection(json!({"url":catalog_url.as_str()}));
    db.unprepared("CREATE TABLE q_cross_catalog (column_name VARCHAR(40) PRIMARY KEY); INSERT INTO q_cross_catalog VALUES ('column_name'), ('unlisted'); CREATE TABLE q_cross_catalog_archive (name VARCHAR(40))").await.unwrap();

    let source = catalog
        .table("COLUMNS as c")
        .select(["c.COLUMN_NAME"])
        .r#where("c.TABLE_SCHEMA", "=", db.getDatabaseName())
        .r#where("c.TABLE_NAME", "=", "q_cross_catalog");
    let outer = db
        .table("q_cross_catalog")
        .whereInSub("column_name", &source)
        .unwrap();
    assert!(
        outer
            .toSql()
            .unwrap()
            .contains("`information_schema`.`COLUMNS` as `c`")
    );
    assert_eq!(
        outer.getBindings().unwrap(),
        vec![db.getDatabaseName().into(), "q_cross_catalog".into()]
    );
    assert_eq!(
        outer.pluck("column_name").await.unwrap(),
        vec!["column_name".into()]
    );
    assert_eq!(
        source
            .toSql()
            .unwrap()
            .matches("`information_schema`")
            .count(),
        0
    );

    let mut tx = db.beginTransaction().await.unwrap();
    assert_eq!(
        tx.table("q_cross_catalog_archive")
            .insertUsing(&["name"], &source)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        tx.table("q_cross_catalog_archive")
            .value("name")
            .await
            .unwrap(),
        "column_name".into()
    );
    tx.rollBack(None).await.unwrap();
    assert_eq!(
        db.table("q_cross_catalog_archive")
            .count("*")
            .await
            .unwrap(),
        0
    );

    let qualified = catalog
        .table("information_schema.COLUMNS as c")
        .select(["c.COLUMN_NAME"]);
    let sql = db
        .table("q_cross_catalog")
        .whereInSub("column_name", &qualified)
        .unwrap()
        .toSql()
        .unwrap();
    assert_eq!(sql.matches("`information_schema`").count(), 1);

    let prefixed = connection(json!({"url":catalog_url.as_str(),"prefix":"p_"}));
    let query = prefixed.table("COLUMNS AS c").select(["c.COLUMN_NAME"]);
    let sql = db
        .table("q_cross_catalog")
        .selectSub(&query, "matched")
        .unwrap()
        .toSql()
        .unwrap();
    assert!(sql.contains("`information_schema`.`p_COLUMNS` as `p_c`"));

    let same_database = connection(json!({"url":url}));
    let source = same_database
        .table("q_cross_catalog")
        .select(["column_name"]);
    assert_eq!(
        db.table("q_cross_catalog")
            .whereInSub("column_name", &source)
            .unwrap()
            .count("*")
            .await
            .unwrap(),
        2
    );

    let port = catalog_url.port_or_known_default().unwrap();
    catalog_url
        .set_port(Some(if port == 1 { 2 } else { 1 }))
        .unwrap();
    let foreign = connection(json!({"url":catalog_url.as_str(),"acquire_timeout":1}));
    assert!(matches!(
        db.table("q_cross_catalog")
            .whereExists(&foreign.table("COLUMNS")),
        Err(DatabaseError::InvalidQuery("subquery connection"))
    ));
    db.unprepared("DROP TABLE q_cross_catalog_archive; DROP TABLE q_cross_catalog")
        .await
        .unwrap();
}

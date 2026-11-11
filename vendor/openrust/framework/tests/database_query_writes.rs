use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{Connection, DatabaseError, DatabaseManager, Value, query::Record},
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

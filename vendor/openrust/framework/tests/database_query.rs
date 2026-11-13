use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{Connection, DatabaseError, DatabaseManager, Value},
    support::facades::DB,
};
use serde_json::{Value as Json, json};

fn connection(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default": "test", "connections": {"test": config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

#[tokio::test]
async fn compiles_dialect_identifiers_and_bindings_in_clause_order() {
    for (driver, delimiter) in [("sqlite", '"'), ("mysql", '`')] {
        let db = connection(json!({"driver": driver, "database": ":memory:", "prefix":"app_"}));
        let builder = db
            .table("items AS i")
            .orderByRaw("case when score = ? then 0 else 1 end", vec![80.into()])
            .havingRaw("count(*) > ?", vec![70.into()])
            .groupByRaw("case when score > ? then score else 0 end", vec![60.into()])
            .r#where("i.score", ">=", 20)
            .whereNested(|query| {
                query
                    .r#where("i.note", "=", Value::Null)
                    .orWhereIn("i.id", vec![30.into(), 40.into()])
                    .orWhereRaw("score = ?", vec![50.into()])
            })
            .select(["i.id AS record"])
            .selectRaw("? as provided", vec![10.into()])
            .forPage(3, 2);

        let q = |name: &str| format!("{delimiter}{name}{delimiter}");
        let (id, i, record, items, score, note) = (
            q("id"),
            q("app_i"),
            q("record"),
            q("app_items"),
            q("score"),
            q("note"),
        );
        assert_eq!(
            builder.toSql().unwrap(),
            format!(
                "select {i}.{id} as {record}, ? as provided from {items} as {i} where \
                 {i}.{score} >= ? and ({i}.{note} is null or {i}.{id} in (?, ?) or score = ?) \
                 group by case when score > ? then score else 0 end having count(*) > ? \
                 order by case when score = ? then 0 else 1 end limit 2 offset 4"
            ),
        );
        assert_eq!(
            builder.getBindings().unwrap(),
            [10, 20, 30, 40, 50, 60, 70, 80].map(Value::from),
        );
        assert_eq!(
            db.table("items").forPage(0, 0).toSql().unwrap(),
            format!("select * from {items} limit 0 offset 0"),
        );
        assert_eq!(
            db.table("items").offset(5).toSql().unwrap(),
            format!("select * from {items} offset 5"),
        );
        let quoted = db.table(&format!("a{delimiter}b")).toSql().unwrap();
        assert_eq!(
            quoted,
            format!("select * from {delimiter}app_a{delimiter}{delimiter}b{delimiter}")
        );
    }
}

#[tokio::test]
async fn rejects_invalid_operators_and_directions_before_executing_sql() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    assert!(matches!(
        db.table("items").r#where("id", "= ? OR 1=1", 1).toSql(),
        Err(DatabaseError::InvalidQuery("comparison operator"))
    ));
    assert!(matches!(
        db.table("items")
            .r#where("id", ">", Value::Null)
            .get()
            .await,
        Err(DatabaseError::InvalidQuery("operator for null value"))
    ));
    assert!(matches!(
        db.table("items")
            .orderBy("id", "asc; drop table items")
            .toSql(),
        Err(DatabaseError::InvalidQuery("order direction"))
    ));
    assert!(matches!(
        db.table("catalog..items").toSql(),
        Err(DatabaseError::InvalidQuery("identifier"))
    ));
}

async fn verify_queries(db: Connection) {
    const CREATE: &str = "CREATE TABLE app_query_items (id BIGINT PRIMARY KEY, category VARCHAR(20), score BIGINT, note VARCHAR(80))";
    db.statement("DROP TABLE IF EXISTS app_query_items", &[])
        .await
        .unwrap();
    db.statement(CREATE, &[]).await.unwrap();
    db.unprepared("INSERT INTO app_query_items VALUES (1, 'west', 5, NULL), (2, 'west', 10, 'x'), (3, 'east', 20, NULL), (4, 'east', 30, 'ok')").await.unwrap();

    db.statement(
        "ALTER TABLE app_query_items ADD COLUMN created_at DATETIME",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "UPDATE app_query_items SET created_at = ? WHERE id = 3",
        &["2026-03-09 12:34:56".into()],
    )
    .await
    .unwrap();

    let table = || db.table("query_items");
    let ids = table()
        .r#where("score", ">", 5)
        .whereNested(|query| query.whereNull("note").orWhere("category", "=", "west"))
        .orderByDesc("id")
        .pluck("id")
        .await
        .unwrap();
    assert_eq!(ids, [3.into(), 2.into()]);
    assert_eq!(
        table()
            .r#where("note", "!=", Value::Null)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .r#where("note", "<=>", Value::Null)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table().whereIn("id", Vec::new()).count("*").await.unwrap(),
        0
    );
    assert_eq!(
        table()
            .whereNotIn("id", Vec::new())
            .count("*")
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        table().whereNested(|query| query).count("*").await.unwrap(),
        4
    );
    assert_eq!(
        table()
            .whereNot(|query| query.whereBetween("score", [5.into(), 20.into()]))
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        table()
            .whereColumn("id", "<", "score")
            .count("*")
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        table()
            .whereBetween("score", [5.into(), 20.into()])
            .max("score")
            .await
            .unwrap(),
        20.into()
    );
    assert_eq!(table().distinct().count("category").await.unwrap(), 2);
    assert_eq!(
        table().r#where("id", "=", 50).sum("score").await.unwrap(),
        0.into()
    );
    assert!(table().r#where("id", "=", 1).exists().await.unwrap());
    assert!(
        table()
            .whereIn("id", vec![50.into()])
            .doesntExist()
            .await
            .unwrap()
    );
    assert_eq!(table().find(50).await.unwrap(), None);
    assert_eq!(
        table().find(3).await.unwrap().unwrap().get("score"),
        Some(&20.into())
    );
    assert_eq!(
        table()
            .r#where("note", "=", "x' OR 1=1 --")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert!(matches!(
        table().whereRaw("score > ?", Vec::new()).get().await,
        Err(DatabaseError::InvalidBindings {
            expected: 1,
            actual: 0
        })
    ));

    assert_eq!(
        table()
            .whereAny(&["category", "note"], "=", "west")
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .whereAll(&["category", "note"], "!=", Value::Null)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .whereNone(&["category", "note"], "=", "west")
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        table()
            .whereLike("category", "W%", false)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .whereLike("category", "W%", true)
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        table()
            .whereNotLike("category", "_ast", true)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        table()
            .select([DB::raw("? as input").with_bindings(vec![19.into()])])
            .first()
            .await
            .unwrap()
            .unwrap()
            .get("input"),
        Some(&19.into())
    );

    let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
    let time = chrono::NaiveTime::from_hms_opt(12, 34, 56).unwrap();
    assert_eq!(
        table()
            .whereDate("created_at", "=", date)
            .whereTime("created_at", "=", time)
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        table()
            .whereYear("created_at", "=", 2026)
            .whereMonth("created_at", "=", 3)
            .whereDay("created_at", "=", 9)
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        table()
            .whereBetweenColumns("id", ["id", "score"])
            .count("*")
            .await
            .unwrap(),
        4
    );

    let group = table()
        .select(["category"])
        .selectRaw("sum(score) as total", Vec::new())
        .groupBy(&["category"])
        .havingRaw("sum(score) > ?", vec![20.into()])
        .orderBy("category", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(group.len(), 1);
    assert_eq!(group[0].get("category"), Some(&"east".into()));
    let total = group[0].get("total").unwrap();
    assert!(total == &50.into() || total == &"50".into(), "{total:?}");

    assert_eq!(
        table()
            .orderBy("id", "asc")
            .forPage(2, 2)
            .pluck("id")
            .await
            .unwrap(),
        [3.into(), 4.into()]
    );
    assert_eq!(
        table()
            .orderBy("id", "asc")
            .offset(3)
            .limit(10)
            .pluck("id")
            .await
            .unwrap(),
        [4.into()]
    );
    assert!(table().limit(0).get().await.unwrap().is_empty());
    assert!(
        table()
            .orderBy("id", "asc")
            .forPage(3, 2)
            .get()
            .await
            .unwrap()
            .is_empty()
    );

    assert!(matches!(
        table().offset(3).get().await,
        Err(DatabaseError::Driver {
            operation: "prepare" | "select",
            ..
        })
    ));
    let mut transaction = db.beginTransaction().await.unwrap();
    transaction
        .update(
            "UPDATE app_query_items SET score = 99 WHERE id = ?",
            &[1.into()],
        )
        .await
        .unwrap();
    assert_eq!(
        transaction
            .table("query_items")
            .r#where("id", "=", 1)
            .value("score")
            .await
            .unwrap(),
        99.into()
    );
    transaction.rollBack(None).await.unwrap();
    assert_eq!(
        table().r#where("id", "=", 1).value("score").await.unwrap(),
        5.into()
    );
}

#[tokio::test]
async fn query_builder_runs_on_real_sqlite() {
    verify_queries(connection(
        json!({"driver":"sqlite","database":":memory:","prefix":"app_"}),
    ))
    .await;
}

#[tokio::test]
async fn query_builder_runs_on_mysql_8_4() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL acceptance requires an isolated database"
        );
        return;
    };
    verify_queries(connection(json!({"url":url,"prefix":"app_"}))).await;
}

async fn verify_json_predicates(db: Connection, mysql: bool) {
    db.statement("DROP TABLE IF EXISTS app_json_predicates", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE app_json_predicates (id INTEGER PRIMARY KEY, document JSON, seen INTEGER NOT NULL DEFAULT 0)",
        &[],
    )
    .await
    .unwrap();
    for (id, document) in [
        (
            1,
            Value::Json(json!({
                "settings": {"tags": ["en", "de"], "marker": null},
                "groups": [[{"name": "root"}]],
                "dotted.key": {"O'Reilly": ["quoted"]}
            })),
        ),
        (2, Value::Json(json!({"settings": {"tags": ["en"]}}))),
        (3, Value::Json(json!({"settings": {"tags": []}}))),
        (4, Value::Null),
    ] {
        db.insert(
            "INSERT INTO app_json_predicates (id, document) VALUES (?, ?)",
            &[id.into(), document],
        )
        .await
        .unwrap();
    }

    let table = || db.table("json_predicates as j");
    let tags = "j.document->settings->tags";
    let marker = "j.document->settings->marker";
    let ids = |ids: &[i64]| ids.iter().copied().map(Value::from).collect::<Vec<_>>();

    let query = table().r#where("j.id", ">", 0).whereNested(|group| {
        group
            .whereJsonContains(tags, json!("de"))
            .orWhereJsonLength(tags, "=", 0)
    });
    let expected_path = Value::from(r#"$."settings"."tags""#);
    let mut bindings = ids(&[0]);
    if mysql {
        bindings.extend([Value::Json(json!("de")), expected_path.clone()]);
        assert!(
            query
                .toSql()
                .unwrap()
                .contains("json_contains(`app_j`.`document`, ?, ?)")
        );
    } else {
        bindings.extend([expected_path.clone(), "de".into()]);
        assert!(
            query
                .toSql()
                .unwrap()
                .contains(r#"json_each("app_j"."document", ?)"#)
        );
    }
    bindings.extend([expected_path, 0.into()]);
    assert_eq!(query.getBindings().unwrap(), bindings);
    assert_eq!(
        query.orderBy("id", "asc").pluck("id").await.unwrap(),
        ids(&[1, 3])
    );

    assert_eq!(
        table()
            .whereJsonContainsKey(marker)
            .orWhereJsonContains("j.document->dotted.key->O'Reilly", json!("quoted"))
            .whereJsonContainsKey("j.document->groups[0][0]->name")
            .pluck("id")
            .await
            .unwrap(),
        ids(&[1])
    );
    assert_eq!(
        table()
            .whereJsonDoesntContainKey(marker)
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        ids(&[2, 3, 4])
    );
    assert_eq!(
        table()
            .whereJsonDoesntContain(tags, json!("de"))
            .orWhereJsonContainsKey(marker)
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        if mysql {
            ids(&[1, 2, 3])
        } else {
            ids(&[1, 2, 3, 4])
        }
    );
    assert_eq!(
        table()
            .whereJsonContainsKey(marker)
            .orWhereJsonDoesntContainKey(marker)
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        ids(&[1, 2, 3, 4])
    );
    assert_eq!(
        table()
            .whereJsonContainsKey(marker)
            .orWhereJsonDoesntContain(tags, json!("en"))
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        if mysql { ids(&[1, 3]) } else { ids(&[1, 3, 4]) }
    );
    assert!(
        table()
            .whereJsonLength(tags, ">= 0 or 1 = 1", 0)
            .toSql()
            .is_err()
    );
    assert!(
        table()
            .whereJsonContainsKey("j.document->invalid\0key")
            .toSql()
            .is_err()
    );

    if mysql {
        assert_eq!(
            table()
                .whereJsonContains(tags, json!(["en", "de"]))
                .pluck("id")
                .await
                .unwrap(),
            ids(&[1])
        );
        assert!(table().whereJsonContainsKey("j.document").toSql().is_err());
    } else {
        assert!(
            table()
                .whereJsonContains(tags, json!(["en", "de"]))
                .toSql()
                .is_err()
        );
    }

    let mut transaction = db.beginTransaction().await.unwrap();
    assert_eq!(
        transaction
            .table("json_predicates")
            .whereJsonLength("document->settings->tags", ">", 1)
            .update(BTreeMap::from([("seen".into(), 1.into())]))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        transaction
            .table("json_predicates")
            .whereJsonContainsKey("document->settings->marker")
            .value("seen")
            .await
            .unwrap(),
        1.into()
    );
    transaction.rollBack(None).await.unwrap();
    assert_eq!(
        table()
            .whereJsonContainsKey(marker)
            .value("seen")
            .await
            .unwrap(),
        0.into()
    );
}

#[tokio::test]
async fn json_predicates_run_on_sqlite() {
    verify_json_predicates(
        connection(json!({"driver":"sqlite","database":":memory:","prefix":"app_"})),
        false,
    )
    .await;
}

#[tokio::test]
async fn json_predicates_run_on_mysql_8_4() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL acceptance requires an isolated database"
        );
        return;
    };
    verify_json_predicates(connection(json!({"url":url,"prefix":"app_"})), true).await;
}

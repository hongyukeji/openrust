use std::{collections::BTreeMap, sync::Arc};

use futures_util::{StreamExt as _, TryStreamExt as _};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, Value,
        query::{Column, Expression},
    },
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

#[tokio::test]
async fn value_ranges_and_explicit_ordering_run_on_mysql_8_4() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_"}));
    db.unprepared("DROP TABLE IF EXISTS app_order_ranges")
        .await
        .unwrap();
    db.unprepared(
        "CREATE TABLE app_order_ranges (id BIGINT PRIMARY KEY, status VARCHAR(20), lower_bound INT, upper_bound INT)",
    ).await.unwrap();
    db.unprepared(
        "INSERT INTO app_order_ranges VALUES
         (1, 'pending', 1, 10), (2, 'active', 5, 5), (3, 'active', 0, 9),
         (4, 'other', 3, 7), (5, NULL, 0, 10), (6, 'active', 6, 10), (7, 'inactive', 6, 6)",
    )
    .await
    .unwrap();
    let table = || db.table("order_ranges");
    let visible = table()
        .whereValueBetween(5, ["lower_bound", "upper_bound"])
        .inOrderOf("status", vec!["active".into(), "pending".into()])
        .orderBy("id", "asc");
    assert_eq!(
        visible.toSql().unwrap(),
        "select * from `app_order_ranges` where ? between `lower_bound` and `upper_bound` order by case when `status` = ? then 0 when `status` = ? then 1 else 2 end, `id` asc"
    );
    assert_eq!(
        visible.getBindings().unwrap(),
        [5.into(), "active".into(), "pending".into()]
    );
    assert_eq!(
        visible.pluck("id").await.unwrap(),
        [2.into(), 3.into(), 1.into(), 4.into(), 5.into()]
    );
    assert_eq!(
        table()
            .whereValueNotBetween(5, ["lower_bound", "upper_bound"])
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        [6.into(), 7.into()]
    );
    assert_eq!(
        table()
            .r#where("id", "=", 1)
            .orWhereValueNotBetween(5, ["lower_bound", "upper_bound"])
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        [1.into(), 6.into(), 7.into()]
    );
    assert_eq!(
        table()
            .r#where("id", "=", 6)
            .orWhereValueBetween(5, ["lower_bound", "upper_bound"])
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        [1.into(), 2.into(), 3.into(), 4.into(), 5.into(), 6.into()]
    );

    let expressed = table()
        .whereValueBetween(
            Expression::new("(? + 1)").with_bindings(vec![4.into()]),
            [
                Column::from("lower_bound"),
                Column::from(Expression::new("upper_bound + ?").with_bindings(vec![0.into()])),
            ],
        )
        .inOrderOf(
            Expression::new("coalesce(status, ?)").with_bindings(vec!["active".into()]),
            vec!["active".into(), "pending".into()],
        )
        .orderBy("id", "asc");
    assert_eq!(
        expressed.getBindings().unwrap(),
        [
            4.into(),
            0.into(),
            "active".into(),
            "active".into(),
            "active".into(),
            "pending".into()
        ]
    );
    assert_eq!(
        expressed.pluck("id").await.unwrap(),
        [2.into(), 3.into(), 5.into(), 1.into(), 4.into()]
    );
    assert_eq!(
        table().inOrderOf("status", Vec::new()).toSql().unwrap(),
        "select * from `app_order_ranges`"
    );
    let union = table()
        .select(["id", "status"])
        .r#where("id", "=", 1)
        .unionAll(&table().select(["id", "status"]).r#where("id", "=", 2))
        .unwrap()
        .inOrderOf("status", vec!["active".into()]);
    assert_eq!(
        union.getBindings().unwrap(),
        [1.into(), 2.into(), "active".into()]
    );
    let rows = union.get().await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.get("id").unwrap())
            .collect::<Vec<_>>(),
        [&2.into(), &1.into()]
    );
    assert!(
        table()
            .inOrderOf("status.", vec!["active".into()])
            .toSql()
            .is_err()
    );
}

#[tokio::test]
async fn group_limits_rank_each_partition_on_mysql_8_4() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = connection(json!({"url":url,"prefix":"app_"}));
    db.unprepared("DROP TABLE IF EXISTS app_group_items")
        .await
        .unwrap();
    db.unprepared("CREATE TABLE app_group_items (id BIGINT PRIMARY KEY, category VARCHAR(20), score INT, active BOOL)").await.unwrap();
    db.unprepared(
        "INSERT INTO app_group_items VALUES
         (1, 'a', 100, 1), (2, 'a', 90, 1), (3, 'a', 80, 1),
         (4, 'b', 60, 1), (5, 'b', 40, 1), (6, 'b', 30, 1),
         (7, 'a', 200, 0)",
    )
    .await
    .unwrap();

    let ranked = || {
        db.table("group_items")
            .select(["id", "category", "score"])
            .r#where("active", "=", true)
            .orderByRaw("case when score >= ? then 0 else 1 end", vec![70.into()])
            .orderByDesc("score")
            .groupLimit(1, "category")
            .offset(1)
    };
    assert_eq!(
        ranked().toSql().unwrap(),
        "select * from (select `id`, `category`, `score`, row_number() over (partition by `category` order by case when score >= ? then 0 else 1 end, `score` desc) as `laravel_row` from `app_group_items` where `active` = ?) as `laravel_table` where `laravel_row` <= ? and `laravel_row` > ? order by `laravel_row`"
    );
    assert_eq!(
        ranked().getBindings().unwrap(),
        [70.into(), true.into(), 2.into(), 1.into()]
    );
    for rows in [
        ranked().get().await.unwrap(),
        ranked().cursor().try_collect::<Vec<_>>().await.unwrap(),
    ] {
        assert!(rows.iter().all(|row| row.get("laravel_row").is_none()));
        let ids = rows
            .iter()
            .map(|row| row.get("id").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&&2.into()) && ids.contains(&&5.into()));
    }
    assert_eq!(ranked().count("*").await.unwrap(), 2);
    assert!(
        !db.table("group_items")
            .groupLimit(0, "category")
            .exists()
            .await
            .unwrap()
    );
    let ids = db
        .table("group_items")
        .fromSub(&ranked(), "ranked_items")
        .unwrap()
        .orderBy("id", "asc")
        .pluck("id")
        .await
        .unwrap();
    assert_eq!(ids, [2.into(), 5.into()]);

    let union = db
        .table("group_items")
        .select(["id", "category", "score"])
        .whereIn("id", vec![1.into(), 2.into()])
        .unionAll(
            &db.table("group_items")
                .select(["id", "category", "score"])
                .whereIn("id", vec![4.into(), 5.into()]),
        )
        .unwrap();
    assert!(
        db.table("group_items")
            .unionAll(&db.table("group_items"))
            .unwrap()
            .groupLimit(1, "category")
            .toSql()
            .is_err()
    );
    let mut combined = db
        .table("group_items")
        .fromSub(&union, "candidates")
        .unwrap()
        .orderByDesc("score")
        .groupLimit(1, "category")
        .pluck("id")
        .await
        .unwrap();
    combined.sort_by_key(|value| match value {
        Value::Integer(id) => *id,
        _ => unreachable!("expected numeric id"),
    });
    assert_eq!(combined, [1.into(), 4.into()]);

    let grouped = db.table("group_items").groupLimit(1, "category");
    assert!(grouped.delete().await.is_err());
    assert_eq!(db.table("group_items").count("*").await.unwrap(), 7);
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
        let overlap = table().r#where("j.id", ">", 0).whereNested(|group| {
            group
                .whereJsonOverlaps(tags, json!(["de", "fr"]))
                .orWhereJsonDoesntOverlap(
                    tags,
                    Expression::new("json_array(?)").with_bindings(vec!["en".into()]),
                )
        });
        assert!(overlap.toSql().unwrap().contains(
            "json_overlaps(json_extract(`app_j`.`document`, ?), ?) or not json_overlaps(json_extract(`app_j`.`document`, ?), json_array(?))"
        ));
        let path = Value::from(r#"$."settings"."tags""#);
        assert_eq!(
            overlap.getBindings().unwrap(),
            vec![
                0.into(),
                path.clone(),
                Value::Json(json!(["de", "fr"])),
                path,
                "en".into()
            ]
        );
        assert_eq!(
            overlap.orderBy("j.id", "asc").pluck("id").await.unwrap(),
            ids(&[1, 3])
        );
        assert_eq!(
            table()
                .whereJsonOverlaps("j.document", json!({"settings":{"tags":["en"]}}))
                .pluck("id")
                .await
                .unwrap(),
            ids(&[2])
        );
        assert_eq!(
            table()
                .whereJsonOverlaps(marker, json!(null))
                .pluck("id")
                .await
                .unwrap(),
            ids(&[1])
        );
        assert!(
            table()
                .whereJsonDoesntOverlap(marker, json!(null))
                .pluck("id")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            table()
                .r#where("j.id", "=", 3)
                .orWhereJsonOverlaps(tags, json!("de"))
                .orderBy("j.id", "asc")
                .pluck("id")
                .await
                .unwrap(),
            ids(&[1, 3])
        );
        assert!(
            table()
                .whereJsonOverlaps("j.document->invalid\0key", json!([]))
                .toSql()
                .is_err()
        );
    } else {
        assert!(
            table()
                .whereJsonContains(tags, json!(["en", "de"]))
                .toSql()
                .is_err()
        );
        assert!(matches!(
            table().whereJsonOverlaps(tags, json!(["de"])).get().await,
            Err(DatabaseError::UnsupportedOperation("JSON overlaps"))
        ));
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
    if mysql {
        assert_eq!(
            transaction
                .table("json_predicates")
                .whereJsonOverlaps("document->settings->tags", json!(["de", "fr"]))
                .update(BTreeMap::from([("seen".into(), 2.into())]))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            transaction
                .table("json_predicates")
                .whereJsonOverlaps("document->settings->marker", json!(null))
                .value("seen")
                .await
                .unwrap(),
            2.into()
        );
    }
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

async fn verify_cursor(db: Connection) {
    db.statement("DROP TABLE IF EXISTS cursor_records", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE cursor_records (id INTEGER PRIMARY KEY, payload VARCHAR(30))",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "INSERT INTO cursor_records VALUES (1, 'one'), (2, 'two'), (3, 'three')",
        &[],
    )
    .await
    .unwrap();

    let mut unused = db.cursor("SELECT * FROM no_such_cursor_table", &[]);
    assert_eq!(db.scalar("SELECT 2", &[]).await.unwrap(), Value::Integer(2));
    // Errors are delivered by polling, never by creating the cursor.
    assert!(matches!(
        unused.next().await,
        Some(Err(DatabaseError::Driver { .. }))
    ));
    assert!(unused.next().await.is_none());

    let mut invalid = db.cursor("SELECT ?", &[]);
    assert!(matches!(
        invalid.next().await,
        Some(Err(DatabaseError::InvalidBindings {
            expected: 1,
            actual: 0
        }))
    ));
    assert!(invalid.next().await.is_none());
    let mut rows = db.table("cursor_records").orderBy("id", "asc").cursor();
    assert_eq!(
        rows.try_next().await.unwrap().unwrap().get("id"),
        Some(&Value::Integer(1))
    );
    drop(rows);
    let count = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        db.table("cursor_records").count("*"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(count, 3);

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction
        .statement("INSERT INTO cursor_records VALUES (4, 'draft')", &[])
        .await
        .unwrap();
    let rows: Vec<_> = transaction
        .table("cursor_records")
        .select(["id", "payload"])
        .r#where("id", ">", 1_i64)
        .orderBy("id", "asc")
        .cursor()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.get("id").unwrap().clone())
            .collect::<Vec<_>>(),
        [2, 3, 4].map(Value::Integer)
    );
    assert_eq!(rows[2].get("payload"), Some(&"draft".into()));
    transaction.rollBack(None).await.unwrap();
    assert_eq!(db.table("cursor_records").count("*").await.unwrap(), 3);

    let mut transaction = db.beginTransaction().await.unwrap();
    transaction
        .statement("INSERT INTO cursor_records VALUES (4, 'cancelled')", &[])
        .await
        .unwrap();
    let mut rows = transaction.cursor("SELECT id FROM cursor_records ORDER BY id", &[]);
    assert!(rows.try_next().await.unwrap().is_some());
    drop(rows);
    assert!(matches!(
        transaction.commit().await,
        Err(DatabaseError::TransactionInterrupted)
    ));
    drop(transaction);
    assert_eq!(db.table("cursor_records").count("*").await.unwrap(), 3);

    db.statement("DROP TABLE cursor_records", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn cursors_stream_on_sqlite() {
    verify_cursor(connection(
        json!({"driver":"sqlite","database":":memory:","max_connections":1}),
    ))
    .await;
}

#[tokio::test]
async fn sqlite_cursor_delivers_rows_before_a_later_query_error() {
    let db = connection(json!({"driver":"sqlite","database":":memory:","max_connections":1}));
    let mut rows = db.cursor(
        "SELECT json_extract(payload, '$.id') FROM (SELECT '{\"id\":1}' AS payload UNION ALL SELECT 'broken')",
        &[],
    );
    assert_eq!(
        rows.try_next().await.unwrap().unwrap().get_at(0),
        Some(&Value::Integer(1))
    );
    assert!(matches!(
        rows.next().await,
        Some(Err(DatabaseError::Driver { .. }))
    ));
    assert!(rows.next().await.is_none());
    assert_eq!(db.scalar("SELECT 2", &[]).await.unwrap(), Value::Integer(2));
}

#[tokio::test]
async fn cursors_stream_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify_cursor(connection(json!({"url":url,"max_connections":1}))).await;
}

#[tokio::test]
async fn prepared_mysql_rowsets_preserve_boundaries_and_connection_selection() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL acceptance requires an isolated database"
        );
        return;
    };
    let db = connection(json!({"url":url,"max_connections":1,"read":{"max_connections":1}}));
    db.unprepared("DROP PROCEDURE IF EXISTS openrust_query_rowsets")
        .await
        .unwrap();
    db.unprepared(
        "CREATE PROCEDURE openrust_query_rowsets(IN n INT) BEGIN
            SELECT n AS result;
            SELECT n AS result WHERE FALSE;
            SELECT n + 1 AS result;
        END",
    )
    .await
    .unwrap();
    db.unprepared("DROP PROCEDURE IF EXISTS openrust_query_no_rows")
        .await
        .unwrap();
    db.unprepared("CREATE PROCEDURE openrust_query_no_rows() DO 1")
        .await
        .unwrap();

    let sets = db
        .selectResultSets("CALL openrust_query_rowsets(?)", &[5.into()], true)
        .await
        .unwrap();
    assert_eq!(sets.iter().map(Vec::len).collect::<Vec<_>>(), [1, 0, 1, 0]);
    assert_eq!(sets[0][0].get("result"), Some(&5.into()));
    assert_eq!(sets[2][0].get("result"), Some(&6.into()));
    for sql in ["CALL openrust_query_no_rows()", "SELECT 5 WHERE FALSE"] {
        let sets = db.selectResultSets(sql, &[], true).await.unwrap();
        assert_eq!(sets.len(), 1);
        assert!(sets[0].is_empty());
    }
    assert!(matches!(
        db.selectResultSets("CALL openrust_query_rowsets(?)", &[], true)
            .await,
        Err(DatabaseError::InvalidBindings {
            expected: 1,
            actual: 0
        })
    ));
    assert!(
        db.selectResultSets("CALL openrust_nonexistent_rowsets()", &[], true)
            .await
            .is_err()
    );

    const CONNECTION_ID: &str = "SELECT CONNECTION_ID() AS id";
    let read = db.selectResultSets(CONNECTION_ID, &[], true).await.unwrap();
    let write = db
        .selectResultSets(CONNECTION_ID, &[], false)
        .await
        .unwrap();
    assert_eq!(
        read[0][0].get("id"),
        Some(&db.scalar(CONNECTION_ID, &[]).await.unwrap())
    );
    assert_ne!(read[0][0].get("id"), write[0][0].get("id"));

    let mut tx = db.beginTransaction().await.unwrap();
    let pinned = tx.selectResultSets(CONNECTION_ID, &[]).await.unwrap();
    assert_eq!(pinned[0][0].get("id"), write[0][0].get("id"));
    let sets = tx
        .selectResultSets("CALL openrust_query_rowsets(?)", &[3.into()])
        .await
        .unwrap();
    assert_eq!(sets[0][0].get("result"), Some(&3.into()));
    assert_eq!(sets[2][0].get("result"), Some(&4.into()));
    tx.commit().await.unwrap();

    db.unprepared("DROP PROCEDURE openrust_query_rowsets")
        .await
        .unwrap();
    db.unprepared("DROP PROCEDURE openrust_query_no_rows")
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_reports_unsupported_rowset_traversal() {
    let db = connection(json!({"driver":"sqlite","database":":memory:"}));
    assert!(matches!(
        db.selectResultSets("SELECT 1", &[], true).await,
        Err(DatabaseError::UnsupportedOperation("selectResultSets"))
    ));
}

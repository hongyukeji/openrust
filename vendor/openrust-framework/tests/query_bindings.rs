use openrust::{
    config::Repository,
    database::{Connection, DatabaseManager, DatabaseResult, Value, query::Builder},
};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, sync::Arc};

fn connection(config: Json) -> Connection {
    DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"test","connections":{"test":config}}),
        )]))),
        ".",
    )
    .connection(None)
    .unwrap()
}
fn values(json: &Json) -> Vec<Value> {
    json.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap().into())
        .collect()
}
fn check(name: &str, query: &mut Builder<'_>, driver: &str, reference: &Json) {
    let expected = &reference[format!("{driver}_{name}")];
    assert!(!expected.is_null(), "缺少参考 {driver}_{name}");
    assert_eq!(
        query.to_sql().unwrap(),
        expected["sql"].as_str().unwrap(),
        "{driver}_{name}"
    );
    let raw = query.get_raw_bindings().unwrap();
    assert_eq!(
        raw.keys().copied().collect::<Vec<_>>(),
        [
            "select",
            "from",
            "join",
            "where",
            "groupBy",
            "having",
            "order",
            "union",
            "unionOrder"
        ]
    );
    for (kind, actual) in raw {
        assert_eq!(
            actual,
            values(&expected["raw"][kind]),
            "{driver}_{name}_{kind}"
        );
    }
    assert_eq!(
        query.get_bindings().unwrap(),
        values(&expected["flat"]),
        "{driver}_{name}_flat"
    );
}
#[tokio::test]
async fn binding_categories_and_mutation_sequences_match_fixed_upstream() {
    let reference: Json =
        serde_json::from_str(include_str!("reference/query-bindings.json")).unwrap();
    for driver in ["sqlite", "pgsql", "mysql"] {
        let db = connection(
            json!({"driver":driver,"database":if driver=="sqlite" {":memory:"} else {"fixture"}}),
        );
        let new = || db.table("binding_items");
        check("empty", &mut new(), driver, &reference);
        check(
            "nested_having",
            &mut new()
                .group_by(&["id"])
                .having("id", ">", 1)
                .having_nested(|q| {
                    q.having("id", "<", 2)
                        .set_bindings(vec![9.into()], Some("having"))
                        .unwrap()
                }),
            driver,
            &reference,
        );
        for kind in [
            "select",
            "from",
            "join",
            "where",
            "groupBy",
            "having",
            "order",
            "union",
            "unionOrder",
        ] {
            check(
                &format!("set_{kind}"),
                &mut new()
                    .set_bindings(vec![11.into(), 12.into()], Some(kind))
                    .unwrap(),
                driver,
                &reference,
            );
            check(
                &format!("add_{kind}"),
                &mut new()
                    .add_binding(11, Some(kind))
                    .unwrap()
                    .add_binding([12, 13], Some(kind))
                    .unwrap(),
                driver,
                &reference,
            );
        }
        check(
            "append",
            &mut new()
                .r#where("id", ">", 1)
                .set_bindings(vec![2.into()], None)
                .unwrap()
                .r#where("id", "<", 9),
            driver,
            &reference,
        );
        check(
            "add_append",
            &mut new()
                .where_raw("id > ?", vec![])
                .add_binding(1, None)
                .unwrap()
                .r#where("id", "<", 9),
            driver,
            &reference,
        );
        check(
            "clear_select",
            &mut new()
                .select_raw("? as marker", vec![1.into()])
                .set_bindings(vec![2.into()], Some("select"))
                .unwrap()
                .select(["id"])
                .select_raw("? as marker", vec![3.into()]),
            driver,
            &reference,
        );
        check(
            "clear_order",
            &mut new()
                .order_by_raw("?", vec![1.into()])
                .set_bindings(vec![2.into()], Some("order"))
                .unwrap()
                .reorder()
                .order_by_raw("?", vec![3.into()]),
            driver,
            &reference,
        );
        check(
            "source_replace",
            &mut new()
                .from_raw("(select ? as id) as one", vec![1.into()])
                .from_raw("(select ? as id) as two", vec![2.into()])
                .from("binding_items"),
            driver,
            &reference,
        );
        let other = new()
            .r#where("id", "<", 9)
            .having_raw("? = 1", vec![4.into()]);
        check(
            "merge",
            &mut new()
                .where_raw("id > ? and id < ?", vec![1.into()])
                .merge_bindings(&other)
                .unwrap()
                .order_by_raw("?", vec![5.into()]),
            driver,
            &reference,
        );
        let all = || {
            let mut part = new()
                .select_raw("? as marker", vec![8.into()])
                .r#where("id", "<", 9);
            new()
                .select_raw("? as marker", vec![1.into()])
                .from_raw("(select ? as id) as source", vec![2.into()])
                .join_where("other", "other.id", "=", 3)
                .r#where("source.id", ">", 4)
                .group_by_raw("?", vec![5.into()])
                .having_raw("? = 1", vec![6.into()])
                .order_by_raw("?", vec![7.into()])
                .union_all(&mut part)
                .unwrap()
                .order_by_raw("?", vec![10.into()])
        };
        check("all", &mut all(), driver, &reference);
        let changed = || {
            let mut q = all();
            for (i, kind) in [
                "select",
                "from",
                "join",
                "where",
                "groupBy",
                "having",
                "order",
                "union",
                "unionOrder",
            ]
            .iter()
            .enumerate()
            {
                q = q
                    .set_bindings(vec![(i as i64 + 101).into()], Some(kind))
                    .unwrap();
            }
            q.r#where("source.id", "<", 200)
                .select_raw("? as extra", vec![201.into()])
                .order_by_raw("?", vec![202.into()])
        };
        check("all_changed", &mut changed(), driver, &reference);
        check("original", &mut changed(), driver, &reference);
        // beforeQuery消费独立快照，绑定改动不会共享可变状态。
        let mut copied = changed().before_query(|q| q.add_binding(303, None));
        copied.apply_before_query_callbacks().unwrap();
        check("copy", &mut copied, driver, &reference);
        assert!(new().set_bindings(vec![1.into()], Some("invalid")).is_err());
        assert!(new().add_binding(1, Some("invalid")).is_err());
        assert_eq!(reference[format!("{driver}_invalid")], true);
    }
}

#[tokio::test]
async fn changed_bindings_execute_reads_aggregates_writes_and_pagination() -> DatabaseResult<()> {
    let config = match std::env::var("OPENRUST_TEST_BINDINGS_URL") {
        Ok(url) => json!({"url":url}),
        Err(_) => {
            assert!(std::env::var_os("OPENRUST_REQUIRE_BINDINGS").is_none());
            json!({"driver":"sqlite","database":":memory:"})
        }
    };
    let db = connection(config);
    db.statement("create table parity_binding_items (id integer primary key, group_id integer, score integer, note varchar(64))",&[]).await?;
    db.statement(
        "insert into parity_binding_items (id, group_id, score) values (1,1,10),(2,1,20),(3,2,30),(4,2,40)",
        &[],
    )
    .await?;
    let base = || db.table("parity_binding_items");
    let projected = base()
        .select_raw("coalesce(?, 0) as marker", vec![1.into()])
        .set_bindings(vec![44.into()], Some("select"))?
        .first()
        .await?
        .unwrap();
    assert_eq!(projected.get("marker"), Some(&44.into()));
    let payload = "x' OR 1=1 --";
    base()
        .r#where("id", "=", 1)
        .update(BTreeMap::from([("note".into(), payload.into())]))
        .await?;
    assert_eq!(
        base()
            .where_raw("note = ?", vec![])
            .add_binding(payload, None)?
            .count("*")
            .await?,
        1
    );
    let mut part = base().select(["id"]).r#where("id", "=", 2);
    let combined = base()
        .select(["id"])
        .r#where("id", "=", 1)
        .union_all(&mut part)?
        .set_bindings(vec![3.into()], Some("union"))?
        .order_by("id", "asc")
        .get()
        .await?;
    assert_eq!(
        combined
            .iter()
            .map(|row| row.get("id").unwrap().clone())
            .collect::<Vec<_>>(),
        [1.into(), 3.into()]
    );
    let having = base()
        .select(["group_id"])
        .group_by(&["group_id"])
        .having_raw("sum(score) > ?", vec![0.into()])
        .set_bindings(vec![50.into()], Some("having"))?
        .get()
        .await?;
    assert_eq!(having.len(), 1);
    assert_eq!(having[0].get("group_id"), Some(&2.into()));

    let ids = base()
        .where_raw("id > ? and id < ?", vec![0.into()])
        .add_binding(4, None)?
        .r#where("group_id", "=", 1)
        .order_by("id", "asc")
        .get()
        .await?;
    assert_eq!(
        ids.iter()
            .map(|r| r.get("id").unwrap().clone())
            .collect::<Vec<_>>(),
        [1.into(), 2.into()]
    );
    assert_eq!(
        base()
            .r#where("id", "=", 1)
            .set_bindings(vec![2.into()], None)?
            .value("score")
            .await?,
        20.into()
    );
    assert_eq!(
        base()
            .select_raw("? as unused", vec![777.into()])
            .set_bindings(vec![888.into()], Some("select"))?
            .r#where("group_id", "=", 1)
            .set_bindings(vec![2.into()], None)?
            .order_by_raw("case when id = ? then 0 else 1 end", vec![1.into()])
            .set_bindings(vec![4.into()], Some("order"))?
            .count("*")
            .await?,
        2
    );
    assert_eq!(
        base()
            .r#where("id", "=", 1)
            .set_bindings(vec![3.into()], None)?
            .update(BTreeMap::from([("score".into(), 99.into())]))
            .await?,
        1
    );
    assert_eq!(
        base().r#where("id", "=", 3).value("score").await?,
        99.into()
    );
    let mut callbacks = base()
        .r#where("id", "=", 1)
        .set_bindings(vec![3.into()], None)?
        .before_query(|q| Ok(q.r#where("score", "=", 99)));
    callbacks.apply_before_query_callbacks()?;
    assert_eq!(callbacks.get_bindings()?, [3.into(), 99.into()]);
    assert_eq!(callbacks.get().await?.len(), 1);
    let page = base()
        .r#where("group_id", "=", 1)
        .set_bindings(vec![2.into()], None)?
        .order_by_raw("case when id = ? then 0 else 1 end", vec![1.into()])
        .set_bindings(vec![4.into()], Some("order"))?
        .paginate(1)
        .await?;
    assert_eq!(page.total(), 2);
    assert_eq!(page.items()[0].get("id"), Some(&4.into()));
    let grouped = base()
        .r#where("score", ">", 1000)
        .set_bindings(vec![0.into()], None)?
        .order_by_raw("case when id = ? then 0 else 1 end", vec![1.into()])
        .set_bindings(vec![4.into()], Some("order"))?
        .order_by("id", "asc")
        .group_limit(1, "group_id")
        .get()
        .await?;
    let selected = grouped
        .iter()
        .map(|r| r.get("id").unwrap().clone())
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&1.into()) && selected.contains(&4.into()));
    assert_eq!(
        base()
            .r#where("id", "=", 1)
            .set_bindings(vec![4.into()], None)?
            .delete()
            .await?,
        1
    );
    assert_eq!(base().count("*").await?, 3);
    if db.get_driver_name() == "pgsql" {
        let mut source = base().select(["id"]).r#where("id", "=", 1);
        let changed = base()
            .join_sub(&mut source, "joined", |join| {
                join.on("parity_binding_items.id", "=", "joined.id")
                    .r#where("joined.id", "=", 1)
            })?
            .r#where("parity_binding_items.id", "=", 2)
            .set_bindings(vec![3.into(), 3.into()], Some("join"))?
            .set_bindings(vec![3.into()], None)?
            .update_from(BTreeMap::from([("score".into(), Value::from(101))]))
            .await?;
        assert_eq!(changed, 1);
        assert_eq!(
            base().r#where("id", "=", 3).value("score").await?,
            101.into()
        );
    }
    db.statement("drop table parity_binding_items", &[]).await?;
    Ok(())
}

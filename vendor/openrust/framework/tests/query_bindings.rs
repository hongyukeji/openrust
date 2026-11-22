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
        query.toSql().unwrap(),
        expected["sql"].as_str().unwrap(),
        "{driver}_{name}"
    );
    let raw = query.getRawBindings().unwrap();
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
        query.getBindings().unwrap(),
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
                .groupBy(&["id"])
                .having("id", ">", 1)
                .havingNested(|q| {
                    q.having("id", "<", 2)
                        .setBindings(vec![9.into()], Some("having"))
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
                    .setBindings(vec![11.into(), 12.into()], Some(kind))
                    .unwrap(),
                driver,
                &reference,
            );
            check(
                &format!("add_{kind}"),
                &mut new()
                    .addBinding(11, Some(kind))
                    .unwrap()
                    .addBinding([12, 13], Some(kind))
                    .unwrap(),
                driver,
                &reference,
            );
        }
        check(
            "append",
            &mut new()
                .r#where("id", ">", 1)
                .setBindings(vec![2.into()], None)
                .unwrap()
                .r#where("id", "<", 9),
            driver,
            &reference,
        );
        check(
            "add_append",
            &mut new()
                .whereRaw("id > ?", vec![])
                .addBinding(1, None)
                .unwrap()
                .r#where("id", "<", 9),
            driver,
            &reference,
        );
        check(
            "clear_select",
            &mut new()
                .selectRaw("? as marker", vec![1.into()])
                .setBindings(vec![2.into()], Some("select"))
                .unwrap()
                .select(["id"])
                .selectRaw("? as marker", vec![3.into()]),
            driver,
            &reference,
        );
        check(
            "clear_order",
            &mut new()
                .orderByRaw("?", vec![1.into()])
                .setBindings(vec![2.into()], Some("order"))
                .unwrap()
                .reorder()
                .orderByRaw("?", vec![3.into()]),
            driver,
            &reference,
        );
        check(
            "source_replace",
            &mut new()
                .fromRaw("(select ? as id) as one", vec![1.into()])
                .fromRaw("(select ? as id) as two", vec![2.into()])
                .from("binding_items"),
            driver,
            &reference,
        );
        let other = new()
            .r#where("id", "<", 9)
            .havingRaw("? = 1", vec![4.into()]);
        check(
            "merge",
            &mut new()
                .whereRaw("id > ? and id < ?", vec![1.into()])
                .mergeBindings(&other)
                .unwrap()
                .orderByRaw("?", vec![5.into()]),
            driver,
            &reference,
        );
        let all = || {
            let mut part = new()
                .selectRaw("? as marker", vec![8.into()])
                .r#where("id", "<", 9);
            new()
                .selectRaw("? as marker", vec![1.into()])
                .fromRaw("(select ? as id) as source", vec![2.into()])
                .joinWhere("other", "other.id", "=", 3)
                .r#where("source.id", ">", 4)
                .groupByRaw("?", vec![5.into()])
                .havingRaw("? = 1", vec![6.into()])
                .orderByRaw("?", vec![7.into()])
                .unionAll(&mut part)
                .unwrap()
                .orderByRaw("?", vec![10.into()])
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
                    .setBindings(vec![(i as i64 + 101).into()], Some(kind))
                    .unwrap();
            }
            q.r#where("source.id", "<", 200)
                .selectRaw("? as extra", vec![201.into()])
                .orderByRaw("?", vec![202.into()])
        };
        check("all_changed", &mut changed(), driver, &reference);
        check("original", &mut changed(), driver, &reference);
        // beforeQuery消费独立快照，绑定改动不会共享可变状态。
        let mut copied = changed().beforeQuery(|q| q.addBinding(303, None));
        copied.applyBeforeQueryCallbacks().unwrap();
        check("copy", &mut copied, driver, &reference);
        assert!(new().setBindings(vec![1.into()], Some("invalid")).is_err());
        assert!(new().addBinding(1, Some("invalid")).is_err());
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
        .selectRaw("coalesce(?, 0) as marker", vec![1.into()])
        .setBindings(vec![44.into()], Some("select"))?
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
            .whereRaw("note = ?", vec![])
            .addBinding(payload, None)?
            .count("*")
            .await?,
        1
    );
    let mut part = base().select(["id"]).r#where("id", "=", 2);
    let combined = base()
        .select(["id"])
        .r#where("id", "=", 1)
        .unionAll(&mut part)?
        .setBindings(vec![3.into()], Some("union"))?
        .orderBy("id", "asc")
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
        .groupBy(&["group_id"])
        .havingRaw("sum(score) > ?", vec![0.into()])
        .setBindings(vec![50.into()], Some("having"))?
        .get()
        .await?;
    assert_eq!(having.len(), 1);
    assert_eq!(having[0].get("group_id"), Some(&2.into()));

    let ids = base()
        .whereRaw("id > ? and id < ?", vec![0.into()])
        .addBinding(4, None)?
        .r#where("group_id", "=", 1)
        .orderBy("id", "asc")
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
            .setBindings(vec![2.into()], None)?
            .value("score")
            .await?,
        20.into()
    );
    assert_eq!(
        base()
            .selectRaw("? as unused", vec![777.into()])
            .setBindings(vec![888.into()], Some("select"))?
            .r#where("group_id", "=", 1)
            .setBindings(vec![2.into()], None)?
            .orderByRaw("case when id = ? then 0 else 1 end", vec![1.into()])
            .setBindings(vec![4.into()], Some("order"))?
            .count("*")
            .await?,
        2
    );
    assert_eq!(
        base()
            .r#where("id", "=", 1)
            .setBindings(vec![3.into()], None)?
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
        .setBindings(vec![3.into()], None)?
        .beforeQuery(|q| Ok(q.r#where("score", "=", 99)));
    callbacks.applyBeforeQueryCallbacks()?;
    assert_eq!(callbacks.getBindings()?, [3.into(), 99.into()]);
    assert_eq!(callbacks.get().await?.len(), 1);
    let page = base()
        .r#where("group_id", "=", 1)
        .setBindings(vec![2.into()], None)?
        .orderByRaw("case when id = ? then 0 else 1 end", vec![1.into()])
        .setBindings(vec![4.into()], Some("order"))?
        .paginate(1)
        .await?;
    assert_eq!(page.total(), 2);
    assert_eq!(page.items()[0].get("id"), Some(&4.into()));
    let grouped = base()
        .r#where("score", ">", 1000)
        .setBindings(vec![0.into()], None)?
        .orderByRaw("case when id = ? then 0 else 1 end", vec![1.into()])
        .setBindings(vec![4.into()], Some("order"))?
        .orderBy("id", "asc")
        .groupLimit(1, "group_id")
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
            .setBindings(vec![4.into()], None)?
            .delete()
            .await?,
        1
    );
    assert_eq!(base().count("*").await?, 3);
    if db.getDriverName() == "pgsql" {
        let mut source = base().select(["id"]).r#where("id", "=", 1);
        let changed = base()
            .joinSub(&mut source, "joined", |join| {
                join.on("parity_binding_items.id", "=", "joined.id")
                    .r#where("joined.id", "=", 1)
            })?
            .r#where("parity_binding_items.id", "=", 2)
            .setBindings(vec![3.into(), 3.into()], Some("join"))?
            .setBindings(vec![3.into()], None)?
            .updateFrom(BTreeMap::from([("score".into(), Value::from(101))]))
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

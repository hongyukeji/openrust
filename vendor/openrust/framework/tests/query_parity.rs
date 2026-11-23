use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseManager, Value,
        query::{Builder, Column, Expression},
    },
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
fn value(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::String(value) => value.clone().into(),
        Json::Number(value) => value.as_i64().unwrap().into(),
        Json::Bool(value) => (*value).into(),
        _ => panic!("参考值必须为 SQL 标量"),
    }
}
fn apply<'a>(
    mut query: Builder<'a>,
    operations: &Json,
) -> openrust::database::DatabaseResult<Builder<'a>> {
    for operation in operations.as_array().unwrap() {
        let name = operation[0].as_str().unwrap();
        let args = operation[1].as_array().unwrap();
        let text = |i: usize| args[i].as_str().unwrap();
        let values = |i: usize| {
            args[i]
                .as_array()
                .unwrap()
                .iter()
                .map(value)
                .collect::<Vec<_>>()
        };
        let columns = |i: usize| match &args[i] {
            Json::String(column) => vec![column.as_str()],
            Json::Array(columns) => columns
                .iter()
                .map(|column| column.as_str().unwrap())
                .collect(),
            _ => panic!("列必须为字符串或列表"),
        };
        query = match name {
            "orderBy" => query.orderBy(text(0), text(1)),
            "orderByRaw" => query.orderByRaw(text(0), values(1)),
            "reorderDesc" => query.reorderDesc(text(0)),
            "inRandomOrder" => query.inRandomOrder(args[0].as_i64()),
            "forPageBeforeId" => query.forPageBeforeId(
                args[0].as_u64().unwrap(),
                (!args[1].is_null()).then(|| value(&args[1])),
                text(2),
            ),
            "forPageAfterId" => query.forPageAfterId(
                args[0].as_u64().unwrap(),
                (!args[1].is_null()).then(|| value(&args[1])),
                text(2),
            ),
            "whereBinary" => query.whereBinary(text(0), value(&args[1])),
            "orWhereBinary" => query.orWhereBinary(text(0), value(&args[1])),
            "whereNotBinary" => query.whereNotBinary(text(0), value(&args[1])),
            "orWhereNotBinary" => query.orWhereNotBinary(text(0), value(&args[1])),
            "whereIntegerInRaw" => query.whereIntegerInRaw(
                text(0),
                args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_i64().unwrap())
                    .collect(),
            ),
            "orWhereIntegerInRaw" => query.orWhereIntegerInRaw(
                text(0),
                args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_i64().unwrap())
                    .collect(),
            ),
            "whereIntegerNotInRaw" => query.whereIntegerNotInRaw(
                text(0),
                args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_i64().unwrap())
                    .collect(),
            ),
            "orWhereIntegerNotInRaw" => query.orWhereIntegerNotInRaw(
                text(0),
                args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_i64().unwrap())
                    .collect(),
            ),
            "whereRowValues" => query.whereRowValues(&columns(0), text(1), values(2))?,
            "orWhereRowValues" => query.orWhereRowValues(&columns(0), text(1), values(2))?,
            "orWhereAny" => query.orWhereAny(&columns(0), text(1), value(&args[2])),
            "orWhereAll" => query.orWhereAll(&columns(0), text(1), value(&args[2])),
            "orWhereNone" => query.orWhereNone(&columns(0), text(1), value(&args[2])),
            "fromRaw" => query.fromRaw(text(0), values(1)),
            "selectRaw" => query.selectRaw(text(0), values(1)),
            "joinWhere" => query.joinWhere(text(0), text(1), text(2), value(&args[3])),
            "leftJoinWhere" => query.leftJoinWhere(text(0), text(1), text(2), value(&args[3])),
            "rightJoinWhere" => query.rightJoinWhere(text(0), text(1), text(2), value(&args[3])),
            "where" => query.r#where(text(0), text(1), value(&args[2])),
            "groupBy" => query.groupBy(&columns(0)),
            "having" => query.having(text(0), text(1), value(&args[2])),
            "orHaving" => query.orHaving(text(0), text(1), value(&args[2])),
            "havingNull" => query.havingNull(&columns(0)),
            "havingNotNull" => query.havingNotNull(&columns(0)),
            "orHavingNull" => query.orHavingNull(&columns(0)),
            "orHavingNotNull" => query.orHavingNotNull(&columns(0)),
            "havingNotBetween" => query.havingNotBetween(text(0), values(1).try_into().unwrap()),
            "orHavingBetween" => query.orHavingBetween(text(0), values(1).try_into().unwrap()),
            "orHavingNotBetween" => {
                query.orHavingNotBetween(text(0), values(1).try_into().unwrap())
            }
            "havingNested" if args.get(1).and_then(Json::as_str) == Some("or") => {
                query.orHavingNested(|query| apply(query, &args[0]).unwrap())
            }
            "havingNested" => query.havingNested(|query| apply(query, &args[0]).unwrap()),
            _ => panic!("未知参考方法 {name}"),
        };
    }
    Ok(query)
}

#[tokio::test]
async fn query_sources_joins_and_havings_match_fixed_upstream_sql_and_binding_order() {
    let reference: Json =
        serde_json::from_str(include_str!("reference/query-builder.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 156);
    for case in cases {
        let driver = case["driver"].as_str().unwrap();
        let db = connection(
            json!({"driver":driver,"database":if driver=="sqlite" {":memory:"} else {"fixture"}}),
        );
        let result = apply(db.table("items"), &case["operations"]);
        if case["error"] == "InvalidArgumentException" {
            assert!(result.is_err(), "{case}");
            continue;
        }
        let mut query = result.unwrap();
        if case["error"] == "RuntimeException" {
            assert!(query.toSql().is_err(), "{case}");
            continue;
        }
        if case.get("error").is_some() {
            // 固定参考对完全空的 HAVING 回调抛 TypeError；Rust 明确沿用空条件组不加入 SQL 的适配。
            assert_eq!(case["error"], "TypeError");
            assert_eq!(
                query.toSql().unwrap(),
                db.table("items")
                    .groupBy(&["id"])
                    .having("id", "=", 7)
                    .toSql()
                    .unwrap()
            );
            assert_eq!(query.get_bindings().unwrap(), vec![7.into()]);
        } else {
            assert_eq!(
                query.toSql().unwrap(),
                case["sql"].as_str().unwrap(),
                "{case}"
            );
            assert_eq!(
                query.get_bindings().unwrap(),
                case["bindings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(value)
                    .collect::<Vec<_>>(),
                "{case}"
            );
        }
    }
}

#[tokio::test]
async fn raw_sources_value_joins_and_nested_havings_execute_on_isolated_database() {
    let config = if let Ok(url) = std::env::var("OPENRUST_TEST_QUERY_URL") {
        json!({"url":url})
    } else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_QUERY").is_none(),
            "必须提供隔离数据库"
        );
        json!({"driver":"sqlite","database":":memory:"})
    };
    let db = connection(config);
    db.statement(
        "create table parity_items (id integer primary key, group_id integer, label varchar(64))",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "create table parity_groups (id integer primary key, flag varchar(64))",
        &[],
    )
    .await
    .unwrap();
    for (id, group, label) in [
        (1, 1, Value::from("a")),
        (2, 1, Value::from("b")),
        (3, 2, Value::Null),
        (4, 3, Value::from("x' OR 1=1 --")),
    ] {
        db.table("parity_items")
            .insert(&[BTreeMap::from([
                ("id".into(), id.into()),
                ("group_id".into(), group.into()),
                ("label".into(), label),
            ])])
            .await
            .unwrap();
    }
    db.table("parity_groups")
        .insert(&[
            BTreeMap::from([("id".into(), 1.into()), ("flag".into(), "active".into())]),
            BTreeMap::from([("id".into(), 2.into()), ("flag".into(), Value::Null)]),
        ])
        .await
        .unwrap();
    let rows = db
        .table("unused")
        .selectRaw("? as marker", vec![9.into()])
        .addSelect(["parity_items.id"])
        .fromRaw(
            "(select * from parity_items where id > ?) as parity_items",
            vec![2.into()],
        )
        .joinWhere("parity_groups", "parity_groups.flag", "=", "active")
        .r#where("parity_items.label", "=", "x' OR 1=1 --")
        .get()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "原始来源查询失败: {error}; source={:?}",
                std::error::Error::source(&error)
            )
        });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("id"), Some(&4.into()));
    let postgres =
        std::env::var("OPENRUST_TEST_QUERY_URL").is_ok_and(|url| url.starts_with("postgres"));
    assert_eq!(
        rows[0].get("marker"),
        Some(&if postgres { "9".into() } else { 9.into() })
    );
    assert_eq!(
        db.table("parity_items")
            .joinWhere("parity_groups", "parity_groups.flag", "=", Value::Null)
            .count("*")
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        db.table("parity_items")
            .leftJoinWhere("parity_groups", "parity_groups.flag", "=", "missing")
            .count("*")
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        db.table("parity_items")
            .rightJoinWhere("parity_groups", "parity_items.id", "=", 999)
            .count("*")
            .await
            .unwrap(),
        2
    );
    let grouped = || {
        db.table("parity_items")
            .select::<Column>([
                "group_id".into(),
                Expression::new("count(*) as tally").into(),
                Expression::new("min(label) as label").into(),
            ])
            .groupBy(&["group_id"])
    };
    assert_eq!(
        grouped().havingNull(&["group_id"]).get().await.unwrap(),
        Vec::new()
    );
    let mut ids = grouped()
        .havingNested(|q| {
            q.havingRaw("count(*) > ?", vec![1.into()])
                .orHaving("group_id", "=", 3)
        })
        .havingNotNull(&["group_id"])
        .pluck("group_id")
        .await
        .unwrap();
    ids.sort_by_key(|id| match id {
        Value::Integer(value) => *value,
        _ => panic!("主键应为整数"),
    });
    assert_eq!(ids, vec![1.into(), 3.into()]);
    let rows = grouped()
        .havingNotBetween("group_id", [1.into(), 2.into()])
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("group_id"), Some(&3.into()));
    assert!(
        db.table("unused")
            .fromRaw("(invalid sql)", vec![])
            .get()
            .await
            .is_err()
    );
    assert_eq!(db.table("parity_items").count("*").await.unwrap(), 4);
}

#[tokio::test]
async fn integer_row_and_grouped_or_conditions_execute_without_interpolating_values() {
    let config = if let Ok(url) = std::env::var("OPENRUST_TEST_QUERY_URL") {
        json!({"url":url})
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_QUERY").is_none());
        json!({"driver":"sqlite","database":":memory:"})
    };
    let db = connection(config);
    db.statement("create table parity_conditions (id integer primary key, other_id integer, label varchar(64))",&[]).await.unwrap();
    for (id, other, label) in [(1, 2, "x' OR 1=1 --"), (2, 2, "UPPER"), (3, 4, "upper")] {
        db.table("parity_conditions")
            .insert(&[BTreeMap::from([
                ("id".into(), id.into()),
                ("other_id".into(), other.into()),
                ("label".into(), label.into()),
            ])])
            .await
            .unwrap();
    }
    assert_eq!(
        db.table("parity_conditions")
            .whereIntegerInRaw("id", vec![-1, 1, 3])
            .orderBy("id", "asc")
            .pluck("id")
            .await
            .unwrap(),
        vec![1.into(), 3.into()]
    );
    assert_eq!(
        db.table("parity_conditions")
            .whereIntegerNotInRaw("id", vec![])
            .count("*")
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        db.table("parity_conditions")
            .whereRowValues(&["id", "other_id"], ">", vec![1.into(), 2.into()])
            .unwrap()
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db.table("parity_conditions")
            .r#where("id", "=", 1)
            .orWhereAll(&["id", "other_id"], "=", 2)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db.table("parity_conditions")
            .r#where("id", "=", 1)
            .orWhereAny(&["id", "other_id"], "=", 4)
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db.table("parity_conditions")
            .r#where("id", "=", 1)
            .orWhereNone(&["id", "other_id"], "=", 2)
            .count("*")
            .await
            .unwrap(),
        2
    );
    for use_or in [false, true] {
        let query = db.table("parity_conditions");
        let invalid = if use_or {
            query.orWhereRowValues(
                &["id"],
                "= ?; drop table parity_conditions; --",
                vec![1.into()],
            )
        } else {
            query.whereRowValues(
                &["id"],
                "= ?; drop table parity_conditions; --",
                vec![1.into()],
            )
        };
        assert!(invalid.is_err(), "非法运算符必须在配置条件时拒绝");
    }
    let mysql = std::env::var("OPENRUST_TEST_QUERY_URL").is_ok_and(|url| url.starts_with("mysql:"));
    if mysql {
        assert_eq!(
            db.table("parity_conditions")
                .whereBinary("label", "UPPER")
                .count("*")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            db.table("parity_conditions")
                .whereNotBinary("label", "UPPER")
                .count("*")
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            db.table("parity_conditions")
                .whereBinary("label", "x' OR 1=1 --")
                .count("*")
                .await
                .unwrap(),
            1
        );
    } else {
        assert!(
            db.table("parity_conditions")
                .whereBinary("label", "UPPER")
                .get()
                .await
                .is_err()
        );
    }
    assert_eq!(db.table("parity_conditions").count("*").await.unwrap(), 3);
}

#[tokio::test]
async fn id_pagination_random_raw_values_and_existence_callbacks_keep_query_lifecycle() {
    use openrust::database::query::ExistsOr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let config = if let Ok(url) = std::env::var("OPENRUST_TEST_QUERY_URL") {
        json!({"url":url})
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_QUERY").is_none());
        json!({"driver":"sqlite","database":":memory:"})
    };
    let db = connection(config);
    db.statement("create table parity_pages (id integer primary key)", &[])
        .await
        .unwrap();
    for id in 1..=7 {
        db.table("parity_pages")
            .insert(&[BTreeMap::from([("id".into(), id.into())])])
            .await
            .unwrap();
    }
    let mut after = 0;
    let mut seen = Vec::new();
    loop {
        let ids = db
            .table("parity_pages")
            .orderByDesc("id")
            .forPageAfterId(3, Some(after.into()), "id")
            .pluck("id")
            .await
            .unwrap();
        if ids.is_empty() {
            break;
        }
        after = match ids.last().unwrap() {
            Value::Integer(id) => *id,
            _ => panic!(),
        };
        seen.extend(ids);
    }
    assert_eq!(seen, (1..=7).map(Value::from).collect::<Vec<_>>());
    let mut before = 8;
    let mut seen = Vec::new();
    loop {
        let ids = db
            .table("parity_pages")
            .orderBy("id", "asc")
            .forPageBeforeId(3, Some(before.into()), "id")
            .pluck("id")
            .await
            .unwrap();
        if ids.is_empty() {
            break;
        }
        before = match ids.last().unwrap() {
            Value::Integer(id) => *id,
            _ => panic!(),
        };
        seen.extend(ids);
    }
    assert_eq!(seen, (1..=7).rev().map(Value::from).collect::<Vec<_>>());
    assert_eq!(
        db.table("parity_pages")
            .forPageBeforeId(1, None, "id")
            .value("id")
            .await
            .unwrap(),
        7.into()
    );
    assert_eq!(
        db.table("parity_pages")
            .forPageAfterId(1, None, "id")
            .value("id")
            .await
            .unwrap(),
        1.into()
    );
    assert_eq!(
        db.table("parity_pages")
            .orderBy("id", "asc")
            .reorderDesc("id")
            .value("id")
            .await
            .unwrap(),
        7.into()
    );
    let mut random = db
        .table("parity_pages")
        .inRandomOrder(Some(42))
        .pluck("id")
        .await
        .unwrap();
    random.sort_by_key(|value| match value {
        Value::Integer(id) => *id,
        _ => panic!(),
    });
    assert_eq!(random, (1..=7).map(Value::from).collect::<Vec<_>>());
    assert_eq!(
        db.table("parity_pages")
            .r#where("id", "=", 1)
            .rawValue("id + ?", vec![4.into()])
            .await
            .unwrap(),
        5.into()
    );
    assert_eq!(
        db.table("parity_pages")
            .r#where("id", "=", 99)
            .rawValue("id + ?", vec![4.into()])
            .await
            .unwrap(),
        Value::Null
    );
    assert_eq!(
        db.table("parity_pages")
            .select(["id"])
            .r#where("id", "=", 1)
            .rawValue("id + ?", vec![4.into()])
            .await
            .unwrap(),
        1.into()
    );
    let calls = AtomicUsize::new(0);
    for exists in [true, false] {
        let id = if exists { 1 } else { 99 };
        let result = db
            .table("parity_pages")
            .r#where("id", "=", id)
            .existsOr(|| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("fallback")
            })
            .await
            .unwrap();
        assert_eq!(
            result,
            if exists {
                ExistsOr::Satisfied
            } else {
                ExistsOr::Callback("fallback")
            }
        );
        let result = db
            .table("parity_pages")
            .r#where("id", "=", id)
            .doesntExistOr(|| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("fallback")
            })
            .await
            .unwrap();
        assert_eq!(
            result,
            if exists {
                ExistsOr::Callback("fallback")
            } else {
                ExistsOr::Satisfied
            }
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        db.table("missing_parity_table")
            .existsOr(|| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        db.table("parity_pages")
            .whereRaw("1=0", vec![])
            .existsOr(|| async {
                Err::<(), _>(openrust::database::DatabaseError::InvalidQuery("回调错误"))
            })
            .await
            .unwrap_err()
            .to_string(),
        "invalid query 回调错误"
    );
}

#[tokio::test]
#[ignore = "必须通过 PostgreSQL 隔离验收入口显式执行"]
async fn postgres_inferred_text_parameters_encode_scalars_without_binary_numeric_bytes() {
    let url = std::env::var("OPENRUST_TEST_QUERY_URL").expect("必须提供隔离 PostgreSQL 连接");
    assert!(url.starts_with("postgres"), "此专项必须使用 PostgreSQL");
    let db = connection(json!({"url":url}));
    for (value, expected) in [
        (Value::Integer(9), "9"),
        (Value::Unsigned(u64::MAX), "18446744073709551615"),
        (Value::Real(1.25), "1.25"),
        (Value::Bool(true), "1"),
        (Value::Bool(false), "0"),
        (Value::Bytes(b"hello".to_vec()), "hello"),
        (Value::Json(json!({"a":1})), "{\"a\":1}"),
    ] {
        assert_eq!(
            db.scalar("select ?", &[value]).await.unwrap(),
            Value::from(expected)
        );
    }
    assert_eq!(
        db.scalar("select cast(? as text)", &[Value::Null])
            .await
            .unwrap(),
        Value::Null
    );
    assert!(
        db.scalar("select cast(? as text)", &[Value::Bytes(vec![255])])
            .await
            .is_err()
    );
    assert_eq!(
        db.scalar("select cast(? as bigint)", &[7.into()])
            .await
            .unwrap(),
        7.into()
    );
}

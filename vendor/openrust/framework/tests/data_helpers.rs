use openrust::support::{Arr, Fluent, data_fill, data_forget, data_get, data_has, data_set};
use serde_json::{Value, json};

#[test]
fn data_helpers_preserve_null_wildcards_order_and_sparse_indices() {
    let mut value = json!({
        "teams": [
            {"users":[{"name":"第一","active":null}, {"name":"Deuxième"}]},
            {"users":[{"name":"🙂"}]}
        ],
        "a.b": "literal",
        "a": {"b": "nested"},
        "*": "literal wildcard"
    });
    assert_eq!(
        data_get(&value, Some("teams.*.users.*.name")),
        Some(json!(["第一", "Deuxième", "🙂"]))
    );
    assert_eq!(
        data_get(&value, Some("teams.{last}.users.{first}.name")),
        Some(json!("🙂"))
    );
    assert_eq!(
        data_get(&value, Some(r"\*")),
        Some(json!("literal wildcard"))
    );
    assert!(data_has(&value, "teams.0.users.0.active"));
    assert!(!data_has(&value, "teams.*.users"));
    assert_eq!(
        data_get(&value, Some("teams.0.users.0.active")),
        Some(Value::Null)
    );
    assert_eq!(data_get(&value, Some("missing")), None);

    assert_eq!(Arr::get(&value, "a.b"), Some(&json!("literal")));
    assert_eq!(data_get(&value, Some("a.b")), Some(json!("nested")));
    assert!(Arr::hasAll(&value, &["a.b", "teams.0"]));
    assert!(!Arr::has_any(&value, &["missing", "teams.5"]));
    data_fill(&mut value, "teams.*.users.*.active", json!(true)).unwrap();
    assert_eq!(
        data_get(&value, Some("teams.*.users.*.active")),
        Some(json!([null, true, true]))
    );
    data_set(&mut value, "teams.*.users.*.role", json!("member")).unwrap();
    data_forget(&mut value, "teams.*.users.*.role").unwrap();
    assert!(!data_has(&value, "teams.0.users.0.role"));
    data_forget(&mut value, "teams.0.users.0").unwrap();
    assert_eq!(
        data_get(&value, Some("teams.0.users")),
        Some(json!({"1":{"name":"Deuxième","active":true}}))
    );
}

#[test]
fn dotted_arrays_and_fluent_share_validation_and_preserve_literal_paths() {
    let original = json!({"group":[{"name":"中"},{}], "empty":[]});
    assert_eq!(Arr::undot(&Arr::dot(&original).unwrap()).unwrap(), original);
    let mut target = original.clone();
    Arr::set(&mut target, "group.*.flag", json!(false)).unwrap();
    assert_eq!(Arr::get(&target, "group.*.flag"), Some(&json!(false)));
    let too_deep = vec!["x"; 128].join(".");
    let previous = target.clone();
    assert!(data_set(&mut target, &too_deep, json!(true)).is_err());
    assert_eq!(target, previous);
    let mut fluent = Fluent::make(original).unwrap();
    fluent.set("group.*.active", json!(true)).unwrap();
    assert_eq!(fluent.get("group.*.active"), Some(json!([true, true])));
}

#[test]
fn typed_array_reads_mutations_and_query_encoding() {
    use openrust::support::ArrValueError;

    let mut value = serde_json::json!({
        "values": {"enabled": true,"count":2,"rate":2.0,"tag":"a+b","empty":null},
        "values.tag": "literal"
    });
    assert_eq!(Arr::boolean(&value, "values.enabled"), Ok(true));
    assert_eq!(Arr::integer(&value, "values.count"), Ok(2));
    assert_eq!(Arr::float(&value, "values.rate"), Ok(2.0));
    assert_eq!(
        Arr::float(&value, "values.count"),
        Err(ArrValueError::Expected("a float"))
    );
    assert_eq!(Arr::integer(&value, "absent"), Err(ArrValueError::Missing));
    assert_eq!(Arr::string(&value, "values.tag"), Ok("literal"));
    assert!(Arr::array(&value, "values").unwrap().is_object());
    Arr::add(&mut value, "values.enabled", false.into()).unwrap();
    Arr::add(&mut value, "values.empty", "filled".into()).unwrap();
    assert_eq!(
        Arr::pull(&mut value, "values.tag").unwrap(),
        Some("literal".into())
    );
    assert_eq!(Arr::string(&value, "values.tag"), Ok("a+b"));
    assert_eq!(Arr::string(&value, "values.empty"), Ok("filled"));
    assert_eq!(Arr::boolean(&value, "values.enabled"), Ok(true));
    assert_eq!(Arr::wrap(serde_json::Value::Null), serde_json::json!([]));
    assert_eq!(
        Arr::query(&serde_json::json!({"value":"a+b","rows":[true,false,null]})).unwrap(),
        "value=a%2Bb&rows%5B0%5D=1&rows%5B1%5D=0"
    );
}

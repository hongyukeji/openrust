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
    assert!(!Arr::hasAny(&value, &["missing", "teams.5"]));
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

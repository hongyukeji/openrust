use openrust_horizon::JobPayload;
use serde_json::{Map, Value, json};

#[test]
fn raw_payload_keeps_text_separate_from_offset_mutations() {
    let raw = r#"{ "id":"delivery", "uuid":"task", "data":{"commandName":"Command"}, "displayName":"Display", "retained":true }"#;
    let mut payload = JobPayload::new(raw).unwrap();

    assert_eq!(payload.value(), raw);
    assert_eq!(payload.id(), Some("task"));
    assert_eq!(payload.command_name(), Some("Command"));
    assert_eq!(payload.display_name(), Some("Display"));
    assert_eq!(payload.tags(), &json!([]));
    assert_eq!(payload.retry_of(), None);
    assert!(!payload.is_retry());
    assert!(!payload.is_silenced());

    payload.offset_set("uuid", Value::Null);
    payload.offset_set("retry_of", json!(""));
    payload.offset_set("tags", Value::Null);
    payload.offset_unset("retained");
    assert!(payload.offset_exists("uuid"));
    assert_eq!(payload.offset_get("uuid"), Some(&Value::Null));
    assert_eq!(payload.id(), Some("delivery"));
    assert_eq!(payload.retry_of(), Some(""));
    assert!(payload.is_retry());
    assert_eq!(payload.tags(), &Value::Null);
    assert!(!payload.offset_exists("retained"));
    assert_eq!(payload.value(), raw);

    payload.set(
        json!({"silenced": true, "tags": ["manual"]})
            .as_object()
            .unwrap()
            .clone(),
    );
    assert!(payload.is_silenced());
    assert_eq!(payload.tags(), &json!(["manual"]));
    assert_eq!(
        serde_json::from_str::<Value>(payload.value())
            .unwrap()
            .as_object(),
        Some(payload.decoded())
    );

    payload.offset_set("uuid", json!(""));
    payload.offset_set("retry_of", Value::Null);
    assert_eq!(payload.id(), Some(""));
    assert!(!payload.is_retry());
    assert!(payload.offset_exists("retry_of"));
    assert!(JobPayload::new("null").is_err());
    assert!(JobPayload::new("[").is_err());
}

#[test]
fn raw_retry_preparation_preserves_tags_and_clears_silence() {
    let mut payload = JobPayload::new(r#"{"id":"new","retry_of":"old","tags":["manual","manual"],"silenced":true,"type":"mail","attempts":0}"#).unwrap();
    payload.prepare(None).unwrap();
    assert_eq!(payload.id(), Some("new"));
    assert_eq!(payload.retry_of(), Some("old"));
    assert_eq!(payload.tags(), &json!(["manual", "manual"]));
    assert_eq!(payload.decoded()["type"], "job");
    assert_eq!(payload.decoded()["attempts"], 0);
    assert!(!payload.is_silenced());
    assert!(
        payload.decoded()["pushedAt"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
            > 0.0
    );
    assert_eq!(
        serde_json::from_str::<Value>(payload.value())
            .unwrap()
            .as_object(),
        Some(payload.decoded())
    );
}

#[test]
fn raw_payload_merges_integer_keys_and_preserves_associative_array_shapes() {
    let source = r#"{ "0": "zero", "8": "eight", "id": "old", "08": "text", "-2": "negative", "9223372036854775808": "large", "nested": {"0":"list"}, "empty": {} }"#;
    let mut payload = JobPayload::new(source).unwrap();
    *payload.value_mut() = r#"{"external":true}"#.to_owned();
    assert_eq!(payload.id(), Some("old"));
    payload.decoded_mut().insert("id".into(), json!("edited"));
    assert_eq!(payload.value(), r#"{"external":true}"#);

    payload.set(Map::from_iter([
        ("3".into(), json!("next")),
        ("updated".into(), json!(true)),
        ("existing".into(), json!({"a":1})),
    ]));
    assert_eq!(
        payload.value(),
        r#"{"0":"zero","1":"eight","id":"edited","08":"text","2":"negative","9223372036854775808":"large","nested":["list"],"empty":[],"3":"next","updated":true,"existing":{"a":1}}"#
    );
    assert_eq!(payload.offset_get("3"), Some(&json!("next")));
    assert_eq!(payload.offset_get("8"), None);

    let mut list = JobPayload::new(r#"["a","b"]"#).unwrap();
    assert_eq!(list.offset_get("0"), Some(&json!("a")));
    list.offset_unset("0");
    list.set(Map::from_iter([("7".into(), json!("c"))]));
    assert_eq!(list.value(), r#"["b","c"]"#);

    let mut empty = JobPayload::new("{}").unwrap();
    empty.set(Map::new());
    assert_eq!(empty.value(), "[]");
    empty.set(Map::from_iter([("id".into(), json!("added"))]));
    assert_eq!(empty.value(), r#"{"id":"added"}"#);
}

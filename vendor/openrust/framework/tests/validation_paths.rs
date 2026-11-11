use openrust::validation::{Data, ValidationError};
use serde_json::{Value, json};

#[test]
fn wildcard_paths_distinguish_missing_children_and_explicit_null() {
    let input = Data::new(json!({
        "users": [{"email": null}, {}, {"email": "test@example.test"}],
        "settings":{"example.test":{"*":"literal"}}
    }))
    .unwrap();
    let fields = input.attributes("users.*.email").unwrap();
    assert_eq!(fields[0], ("users.0.email".into(), Some(&Value::Null)));
    assert_eq!(fields[1], ("users.1.email".into(), None));
    assert_eq!(
        fields[2],
        ("users.2.email".into(), Some(&json!("test@example.test")))
    );
    assert_eq!(
        input.attributes(r"settings.example\.test.\*").unwrap(),
        vec![(r"settings.example\.test.\*".into(), Some(&json!("literal")))]
    );
    assert!(input.attributes("missing.*.email").unwrap().is_empty());
}

#[test]
fn validation_paths_reject_invalid_and_unbounded_inputs() {
    let input = Data::new(json!({"profile": {}})).unwrap();
    assert!(matches!(
        input.attributes(r"invalid\q"),
        Err(ValidationError::InvalidPath)
    ));
    assert!(matches!(
        input.attributes(&"x".repeat(1025)),
        Err(ValidationError::LimitsExceeded)
    ));
    assert!(matches!(
        Data::new(json!(vec![0; 1025])),
        Err(ValidationError::LimitsExceeded)
    ));
}

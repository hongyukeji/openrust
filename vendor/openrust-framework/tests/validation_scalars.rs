use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, RuleSet},
};
use serde_json::{Value, json};

async fn validate(value: Value, rule: &str) -> bool {
    Factory::default()
        .make(
            Data::new(json!({"value":value})).unwrap(),
            RuleSet::parse([("value", rule)]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .validate()
        .await
        .is_ok()
}

#[tokio::test]
async fn scalar_rules_follow_fixed_php_attribute_method_fixtures_and_empty_value_scheduling() {
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-scalars.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 199);
    for case in cases {
        let value = case["value"].clone();
        let rule = case["rule"].as_str().unwrap();
        // 非隐式规则跳过空白字符串；夹具记录的是上游属性方法本身的返回值。
        let skipped = value.as_str().is_some_and(|value| {
            value
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .is_empty()
        });
        let expected = skipped || case["passes"].as_bool().unwrap();
        assert_eq!(
            validate(value.clone(), rule).await,
            expected,
            "{rule}: {value}"
        );
    }
}

#[tokio::test]
async fn ascii_and_scalar_rule_absence_nullable_and_required_follow_validation_lifecycle() {
    for (value, expected) in [
        (json!("hello"), true),
        (json!("a\u{0}b"), true),
        (json!("中文"), false),
        (json!(12), false),
        (json!(false), false),
        (Value::Null, false),
    ] {
        assert_eq!(validate(value, "ascii").await, expected);
    }
    for rule in [
        "ascii",
        "lowercase",
        "uppercase",
        "alpha",
        "alpha_dash:ascii",
        "alpha_num",
        "list",
        "ip",
        "ipv4",
        "ipv6",
        "mac_address",
        "json",
        "hex_color",
        "not_regex:/^foo/",
        "starts_with:foo",
        "ends_with:foo",
        "doesnt_start_with:foo",
        "doesnt_end_with:foo",
    ] {
        let mut missing = Factory::default()
            .make(
                Data::new(json!({})).unwrap(),
                RuleSet::parse([("value", rule)]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        missing.validate().await.unwrap();
        assert!(
            validate(Value::Null, &format!("nullable|{rule}")).await,
            "{rule}"
        );
        assert!(
            !validate(json!(""), &format!("required|{rule}")).await,
            "{rule}"
        );
    }
}

#[tokio::test]
async fn digit_message_parameters_and_json_recursion_limits_are_observable() {
    for (rule, message) in [
        ("digits:3", "The value field must be 3 digits."),
        (
            "starts_with:foo,bar",
            "The value field must start with one of the following: foo, bar.",
        ),
        (
            "ends_with:foo,bar",
            "The value field must end with one of the following: foo, bar.",
        ),
        (
            "digits_between:2,4",
            "The value field must be between 2 and 4 digits.",
        ),
        (
            "max_digits:3",
            "The value field must not have more than 3 digits.",
        ),
        (
            "min_digits:3",
            "The value field must have at least 3 digits.",
        ),
        (
            "decimal:1,3",
            "The value field must have 1-3 decimal places.",
        ),
    ] {
        let mut validator = Factory::default()
            .make(
                Data::new(json!({"value":"wrong"})).unwrap(),
                RuleSet::parse([("value", rule)]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(validator.validate().await.is_err());
        assert_eq!(validator.errors().first(Some("value"), None), message);
    }
    assert!(
        validate(
            json!(format!("{}0{}", "[".repeat(127), "]".repeat(127))),
            "json"
        )
        .await
    );
    assert!(
        !validate(
            json!(format!("{}0{}", "[".repeat(128), "]".repeat(128))),
            "json"
        )
        .await
    );
    for rule in ["digits", "digits_between:1", "decimal:broken"] {
        assert!(RuleSet::parse([("value", rule)]).is_err());
    }
}

#[tokio::test]
async fn negative_pattern_and_affix_rules_preserve_messages_and_parameter_failures() {
    for (rule, input, message) in [
        (
            "not_regex:/foo/",
            "food",
            "The value field format is invalid.",
        ),
        (
            "doesnt_start_with:foo,bar",
            "food",
            "The value field must not start with one of the following: foo, bar.",
        ),
        (
            "doesnt_end_with:foo,bar",
            "afoo",
            "The value field must not end with one of the following: foo, bar.",
        ),
        ("lowercase", "FOO", "The value field must be lowercase."),
        ("uppercase", "foo", "The value field must be uppercase."),
    ] {
        let mut validator = Factory::default()
            .make(
                Data::new(json!({"value": input})).unwrap(),
                RuleSet::parse([("value", rule)]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(validator.validate().await.is_err());
        assert_eq!(validator.errors().first(Some("value"), None), message);
    }
    for rule in ["not_regex", "not_regex:/[/", "not_regex:/foo/z"] {
        assert!(RuleSet::parse([("value", rule)]).is_err(), "{rule}");
    }
    // 管道分隔属于规则字符串语法；正则内部的管道通过独立 Rule 传入。
    let mut validator = Factory::default()
        .make(
            Data::new(json!({"value":"bar"})).unwrap(),
            RuleSet::new()
                .field(
                    "value",
                    [openrust::validation::Rule::parse("not_regex:/foo|bar/").unwrap()],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_err());
}

#[tokio::test]
async fn numeric_divisor_guards_and_missing_parameters_fail_explicitly() {
    for rule in [
        "multiple_of",
        "date_format",
        "array_keys",
        "in_array",
        "same",
        "different",
        "before",
        "before_or_equal",
        "after",
        "after_or_equal",
        "date_equals",
        "gt",
        "lt",
        "gte",
        "lte",
        "timezone:invalid",
        "timezone:per_country",
        "timezone:per_country,USA",
    ] {
        assert!(RuleSet::parse([("value", rule)]).is_err(), "{rule}");
    }
    for (value, rule) in [("1e1001", "multiple_of:2"), ("2", "multiple_of:1e1001")] {
        let mut validator = Factory::default()
            .make(
                Data::new(json!({"value":value})).unwrap(),
                RuleSet::parse([("value", rule)]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(matches!(
            validator.validate().await,
            Err(openrust::validation::ValidationError::Execution { .. })
        ));
    }
}

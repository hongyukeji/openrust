use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, RuleSet},
};
use serde_json::{Value, json};

async fn passes(value: Value, rules: &str) -> bool {
    Factory::default()
        .make(
            Data::new(json!({"value":value})).unwrap(),
            RuleSet::parse([("value", rules)]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .validate()
        .await
        .is_ok()
}

#[tokio::test]
async fn integer_and_decimal_boundaries_match_php_filter_and_decimal_comparisons() {
    for value in [
        json!(true),
        json!(1.0),
        json!("+1"),
        json!("\u{c}1\u{c}"),
        json!(i64::MIN),
    ] {
        assert!(passes(value.clone(), "integer").await, "应接受 {value}");
    }
    for value in [
        json!(false),
        json!("01"),
        json!("\u{2003}1\u{2003}"),
        json!(9223372036854775808u64),
        json!(1.1),
    ] {
        assert!(!passes(value.clone(), "integer").await, "应拒绝 {value}");
    }
    assert!(passes(json!(1), "integer:strict").await);
    assert!(!passes(json!(1.0), "integer:strict").await);
    assert!(!passes(json!(true), "integer:strict").await);
    assert!(passes(json!("-1.25"), "numeric|between:-1.5,-1").await);
    assert!(!passes(json!("-1.75"), "numeric|between:-1.5,-1").await);
    assert!(passes(json!("9007199254740993"), "numeric|min:9007199254740993").await);
    assert!(!passes(json!("9007199254740992"), "numeric|min:9007199254740993").await);
}

#[tokio::test]
async fn standard_rules_validate_positive_and_negative_consumers() {
    for (rule, positive, negative) in [
        ("boolean:strict", json!(true), json!(1)),
        ("date", json!("November 21, 2026"), json!("2026-02-30")),
        ("date", json!(20261121), json!(20260230)),
        ("date", json!(20261121.0), json!(true)),
        ("in:1,2", json!(1.0), json!(1.5)),
        ("date", json!("2026-11-21T12:30:00Z"), json!("tomorrow")),
        ("accepted", json!("yes"), json!("")),
        ("url", json!("https://example.test/a"), json!("not a URL")),
        (
            "uuid",
            json!("550e8400-e29b-41d4-a716-446655440000"),
            json!("invalid"),
        ),
        ("in:a,b", json!("a"), json!("c")),
        ("regex:/^a+$/", json!("aaa"), json!("b")),
        ("array:name", json!({"name":"a"}), json!({"other":"a"})),
    ] {
        assert!(
            passes(positive.clone(), rule).await,
            "{rule} 应接受 {positive}"
        );
        assert!(
            !passes(negative.clone(), rule).await,
            "{rule} 应拒绝 {negative}"
        );
    }
}

#[tokio::test]
async fn after_runs_on_failure_and_stop_is_global_while_safe_excludes_unvalidated_data() {
    let mut validator = Factory::default()
        .make(
            Data::new(json!({"a":"wrong","b":"wrong","extra":1})).unwrap(),
            RuleSet::parse([("a", "integer"), ("b", "integer")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator.stop_on_first_failure(true).after(|validator| {
        validator.errors_mut().add("after", "额外检查失败");
        Ok(())
    });
    assert!(validator.validate().await.is_err());
    assert!(!validator.errors().get("a", None).into_messages().is_empty());
    assert!(validator.errors().get("b", None).into_messages().is_empty());
    assert!(
        !validator
            .errors()
            .get("after", None)
            .into_messages()
            .is_empty()
    );
    assert!(validator.safe().is_err());
}

#[tokio::test]
async fn exclusions_follow_rule_order_and_preserve_other_attributes() {
    use openrust::validation::Rule;
    let data = Data::new(json!({"mode":"01","value":"bad","extra":9})).unwrap();
    let mut validator = Factory::default()
        .make(
            data,
            RuleSet::parse([("value", "integer|exclude_if:mode,1"), ("mode", "required")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        validator.validate().await.unwrap().all(),
        &json!({"mode":"01"})
    );
    assert_eq!(
        validator.validate().await.unwrap().all(),
        &json!({"mode":"01"})
    );
    let mut validator = Factory::default()
        .make(
            Data::new(json!({"mode":true,"value":"bad"})).unwrap(),
            RuleSet::parse([("value", "bail|integer|exclude_if:mode,true")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_err());
    let mut validator = Factory::default()
        .make(
            Data::new(
                json!({"rows":[{"kind":"email","address":"x"},{"kind":"sms","address":"x"}]}),
            )
            .unwrap(),
            RuleSet::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator
        .sometimes("rows.*.address", vec![Rule::integer()], |_, item| {
            item.and_then(|value| value.get("kind")) == Some(&json!("email"))
        })
        .unwrap();
    assert!(validator.validate().await.is_err());
    assert!(
        !validator
            .errors()
            .get("rows.0.address", None)
            .into_messages()
            .is_empty()
    );
    assert!(
        validator
            .errors()
            .get("rows.1.address", None)
            .into_messages()
            .is_empty()
    );
    assert!(passes(json!(["a", "b"]), "array|in:a,b").await);
    assert!(!passes(json!(["a", ["b"]]), "array|in:a,b").await);
    assert!(!passes(json!(["a", "c"]), "array|in:a,b").await);
}

#[tokio::test]
async fn numeric_syntax_and_size_exponents_keep_php_boundaries() {
    for value in [json!("+1.2e3"), json!(".5"), json!("1."), json!(" 2\n")] {
        assert!(passes(value, "numeric").await);
    }
    for value in [
        json!("1_000"),
        json!("0x10"),
        json!("NaN"),
        json!("\u{2003}1"),
        json!(true),
    ] {
        assert!(!passes(value, "numeric").await);
    }
    assert!(passes(json!("1e1000"), "numeric|min:1").await);
    let error = Factory::default()
        .make(
            Data::new(json!({"value":"1e1001"})).unwrap(),
            RuleSet::parse([("value", "numeric|min:1")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .validate()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        openrust::validation::ValidationError::Execution { .. }
    ));
}

#[tokio::test]
async fn regex_array_rules_keep_pipe_alternatives_and_unicode_classes() {
    use openrust::validation::Rule;
    for (pattern, value, expected) in [
        (r"/^\w+$/u", "中文", true),
        (r"/^(a|b)$/", "a", true),
        (r"/^(a|b)$/", "c", false),
        (r"/^a$/", "a\n", true),
        (r"/^a$/D", "a\n", false),
        (r"/^a$/D", "a", true),
        (r"/^a$/mD", "a\nb", true),
    ] {
        let mut validator = Factory::default()
            .make(
                Data::new(json!({"value":value})).unwrap(),
                RuleSet::new()
                    .field("value", [Rule::parse(&format!("regex:{pattern}")).unwrap()])
                    .unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(validator.validate().await.is_ok(), expected);
    }
    assert!(Rule::parse("regex:/a/z").is_err());
}

#[tokio::test]
async fn standard_rules_preserve_missing_null_wildcards_and_translated_conditional_values() {
    use openrust::translation::{FileLoader, Translator};
    use std::{collections::BTreeMap, sync::Arc};
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("zh-Hans")).unwrap();
    std::fs::write(
        root.path().join("zh-Hans/validation.json"),
        serde_json::to_vec(&json!({
            "numeric": ":attribute 必须为数值",
            "required_if": ":other 为 :value 时必须填写 :attribute",
            "attributes": {"mode":"模式", "amount":"数量"},
            "values": {"mode":{"card":"银行卡"}}
        }))
        .unwrap(),
    )
    .unwrap();
    let factory = Factory::new(Arc::new(
        Translator::new(
            FileLoader::new(root.path().to_owned()),
            "zh-Hans".into(),
            "en".into(),
        )
        .unwrap(),
    ));
    let mut validator = factory
        .make(
            Data::new(json!({"mode":"card", "amount":null, "rows":[{"score":"bad"},{}]})).unwrap(),
            RuleSet::parse([
                ("amount", "nullable|required_if:mode,card,cash|numeric"),
                ("rows.*.score", "nullable|numeric"),
                ("missing", "numeric|date|url|uuid|in:a,b"),
            ])
            .unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
    assert!(validator.validate().await.is_err());
    assert_eq!(
        validator.errors().get("amount", None).into_messages(),
        ["模式 为 银行卡 时必须填写 数量"]
    );
    assert_eq!(
        validator.errors().get("rows.0.score", None).into_messages(),
        ["rows.0.score 必须为数值"]
    );
    assert!(
        validator
            .errors()
            .get("rows.1.score", None)
            .into_messages()
            .is_empty()
    );
    assert!(
        validator
            .errors()
            .get("missing", None)
            .into_messages()
            .is_empty()
    );
    let mut validator = factory
        .make(
            Data::new(json!({"mode":"other", "amount":null})).unwrap(),
            RuleSet::parse([("amount", "nullable|required_if:mode,card,cash|numeric")]).unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(
        validator.validate().await.unwrap().all(),
        &json!({"amount":null})
    );
    for (input, rules, expected) in [
        (json!({}), "accepted", false),
        (json!({"value":null}), "numeric", false),
        (json!({"value":null}), "nullable|numeric", true),
        (json!({"value":""}), "numeric", true),
        (json!({"mode":false}), "required_if:mode,true", true),
        (json!({"mode":true}), "required_if:mode,true", false),
    ] {
        let mut validator = factory
            .make(
                Data::new(input).unwrap(),
                RuleSet::parse([("value", rules)]).unwrap(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(validator.validate().await.is_ok(), expected, "规则 {rules}");
    }
}

#[tokio::test]
async fn integer_uses_php_float_precision_before_filtering() {
    for value in [
        json!(1.0),
        json!(1e13),
        json!(-1e13),
        json!(100000000000000i64),
        json!(-100000000000000i64),
    ] {
        assert!(passes(value.clone(), "integer").await, "应接受 {value}");
    }
    for value in [json!(1e14), json!(-1e14), json!(1.5), json!(1e20)] {
        assert!(!passes(value.clone(), "integer").await, "应拒绝 {value}");
    }
}

#[tokio::test]
async fn regex_rejects_boolean_and_null_but_nullable_skips_null() {
    for (value, rule) in [
        (json!(true), "regex:/^1$/"),
        (json!(false), "regex:/^$/"),
        (Value::Null, "regex:/^$/"),
    ] {
        assert!(!passes(value.clone(), rule).await, "应拒绝 {value}");
    }
    for value in [json!(1), json!(1.0), json!("1")] {
        assert!(passes(value.clone(), "regex:/^1$/").await, "应接受 {value}");
    }
    assert!(passes(Value::Null, "nullable|regex:/^$/").await);
    assert!(passes(json!(""), "regex:/^$/").await);
}

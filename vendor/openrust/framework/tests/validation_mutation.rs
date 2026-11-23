use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    translation::{FileLoader, Line, Translator},
    validation::{Data, Factory, RuleSet, ValidationError, Validator},
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

fn validator() -> Validator {
    Factory::default()
        .make(
            Data::new(json!({"users":[{"age":20}]})).unwrap(),
            RuleSet::parse([("users.*.age", "integer|min:18")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
}

#[tokio::test]
async fn replacing_values_and_rules_invalidates_results_and_preserves_atomic_failures() {
    let mut validator = validator();
    assert!(validator.validate().await.is_ok());
    assert_eq!(
        validator.get_value("users.0.age").unwrap(),
        Some(&json!(20))
    );
    assert_eq!(
        validator
            .get_rules()
            .unwrap()
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<Vec<_>>(),
        ["users.0.age"]
    );
    validator.set_value("users.0.age", json!(16)).unwrap();
    assert!(matches!(
        validator.validated(),
        Err(ValidationError::NotValidated)
    ));
    assert!(validator.validate().await.is_err());
    assert!(!validator.errors().is_empty());
    validator.set_value("users.0.age", json!(30)).unwrap();
    assert!(validator.errors().is_empty());
    assert!(validator.validate().await.is_ok());
    assert!(validator.set_value("users.*.age", json!(0)).is_err());
    assert_eq!(
        validator.get_value("users.0.age").unwrap(),
        Some(&json!(30))
    );
    assert!(validator.validated().is_ok());
    validator.set_rules(RuleSet::parse([("users.*.age", "integer|min:40")]).unwrap());
    assert!(validator.safe().is_err());
    assert!(validator.validate().await.is_err());
    validator.set_data(Data::new(json!({"users":[{"age":50},{"age":60}]})).unwrap());
    assert_eq!(validator.get_rules().unwrap().len(), 2);
    assert!(validator.validate().await.is_ok());
    assert_eq!(validator.get_data().as_value()["users"][1]["age"], 60);
}

#[tokio::test]
async fn runtime_additions_reset_on_set_data_but_append_rules_becomes_initial() {
    let mut validator = validator();
    validator
        .add_rules(RuleSet::parse([("users.*.age", "min:25"), ("token", "required")]).unwrap())
        .unwrap();
    assert_eq!(validator.get_rules().unwrap()[0].1.len(), 3);
    assert!(validator.validate().await.is_err());
    validator.set_data(Data::new(json!({"users":[{"age":20}]})).unwrap());
    assert_eq!(validator.get_rules().unwrap()[0].1.len(), 2);
    assert!(validator.validate().await.is_ok());
    validator
        .append_rules(RuleSet::parse([("token", "required")]).unwrap())
        .unwrap();
    validator.set_data(Data::new(json!({"users":[{"age":20}]})).unwrap());
    assert!(validator.validate().await.is_err());
    assert!(
        !validator
            .errors()
            .get("token", None)
            .intoMessages()
            .is_empty()
    );
    validator.set_value("token", json!("ok")).unwrap();
    assert!(validator.validate().await.is_ok());
    validator
        .sometimes("token", vec![openrust::validation::Rule::min(5)], |_, _| {
            true
        })
        .unwrap();
    assert!(validator.safe().is_err());
    assert!(validator.validate().await.is_err());
    let too_many = RuleSet::new()
        .field(
            "token",
            std::iter::repeat_n(openrust::validation::Rule::required(), 64),
        )
        .unwrap();
    let before = validator
        .get_rules()
        .unwrap()
        .iter()
        .map(|(_, rules)| rules.len())
        .collect::<Vec<_>>();
    assert!(validator.add_rules(too_many).is_err());
    assert_eq!(
        before,
        validator
            .get_rules()
            .unwrap()
            .iter()
            .map(|(_, rules)| rules.len())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn message_attribute_value_and_translator_changes_recompute_errors() {
    let mut validator = Factory::default()
        .make(
            Data::new(json!({"kind":"x","value":"bad"})).unwrap(),
            RuleSet::parse([("kind", "starts_with:a"), ("value", "integer")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator.set_custom_messages(BTreeMap::from([(
        "starts_with".into(),
        ":attribute :values".into(),
    )]));
    validator.set_attribute_names(BTreeMap::from([("kind".into(), "种类".into())]));
    validator.set_value_names(BTreeMap::from([(
        "kind".into(),
        BTreeMap::from([("a".into(), "甲".into())]),
    )]));
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(Some("kind"), None), "种类 甲");
    validator.set_custom_messages(BTreeMap::from([("integer".into(), "整数错误".into())]));
    validator.add_custom_attributes(BTreeMap::from([("value".into(), "数值".into())]));
    validator.add_custom_values(BTreeMap::from([(
        "kind".into(),
        BTreeMap::from([("a".into(), "A".into())]),
    )]));
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(Some("kind"), None), "种类 A");
    assert_eq!(validator.errors().first(Some("value"), None), "整数错误");
    validator
        .set_attribute_names(BTreeMap::new())
        .set_value_names(BTreeMap::new());
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(Some("kind"), None), "kind a");
    let resources = tempfile::tempdir().unwrap();
    let translator = Arc::new(
        Translator::new(
            FileLoader::new(resources.path().into()),
            "en".into(),
            "en".into(),
        )
        .unwrap(),
    );
    translator
        .addLines(
            BTreeMap::from([(
                "validation.required".into(),
                Line::Text("新的必填错误".into()),
            )]),
            "en",
            None,
        )
        .unwrap();
    validator.set_translator(translator.clone());
    assert!(Arc::ptr_eq(
        validator.get_translator().unwrap(),
        &translator
    ));
    validator.set_rules(RuleSet::parse([("missing", "required")]).unwrap());
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(None, None), "新的必填错误");
}

#[tokio::test]
async fn typed_exception_factory_controls_actual_validation_rejection_and_is_instance_local() {
    fn custom(
        errors: openrust::support::MessageBag,
        _: Option<&Translator>,
    ) -> openrust::validation::ValidationException {
        let mut error = openrust::validation::ValidationException::new(errors);
        error
            .status(openrust::http::StatusCode::BAD_REQUEST)
            .error_bag("custom")
            .redirect_to("/retry");
        error
    }
    let mut first = validator();
    let mut second = validator();
    let default = first.get_exception();
    first.set_exception(custom);
    first.set_value("users.0.age", json!(0)).unwrap();
    second.set_value("users.0.age", json!(0)).unwrap();
    for (validator, status) in [(&mut first, 400), (&mut second, 422)] {
        let Err(ValidationError::Invalid(error)) = validator.validate().await else {
            panic!("必须返回真实验证异常");
        };
        assert_eq!(error.status_code().as_u16(), status);
        assert!(!error.errors().is_empty());
    }
    first.set_exception(default);
    let Err(ValidationError::Invalid(error)) = first.validate().await else {
        panic!("必须返回真实验证异常");
    };
    assert_eq!(error.status_code().as_u16(), 422);
}

#[tokio::test]
async fn runtime_mutation_sequences_follow_complete_fixed_php_validator() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/validation-lifecycle.json")).unwrap();
    fn rules(value: &serde_json::Value) -> RuleSet {
        RuleSet::parse(
            value
                .as_object()
                .unwrap()
                .iter()
                .map(|(field, rule)| (field.as_str(), rule.as_str().unwrap())),
        )
        .unwrap()
    }
    let cases = reference["mutations"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for (index, case) in cases.iter().enumerate() {
        let mut validator = Factory::default()
            .make(
                Data::new(case["data"].clone()).unwrap(),
                rules(&case["rules"]),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        for step in case["steps"].as_array().unwrap() {
            let argument = &step["arguments"][0];
            match step["method"].as_str().unwrap() {
                "setValue" => validator
                    .set_value(argument.as_str().unwrap(), step["arguments"][1].clone())
                    .unwrap(),
                "setData" => {
                    validator.set_data(Data::new(argument.clone()).unwrap());
                }
                "setRules" => {
                    validator.set_rules(rules(argument));
                }
                "addRules" => validator.add_rules(rules(argument)).unwrap(),
                "appendRules" => {
                    validator.append_rules(rules(argument)).unwrap();
                }
                "setCustomMessages" => {
                    validator
                        .set_custom_messages(serde_json::from_value(argument.clone()).unwrap());
                }
                "setAttributeNames" => {
                    validator.set_attribute_names(if argument.is_array() {
                        BTreeMap::new()
                    } else {
                        serde_json::from_value(argument.clone()).unwrap()
                    });
                }
                "setValueNames" => {
                    validator.set_value_names(if argument.is_array() {
                        BTreeMap::new()
                    } else {
                        serde_json::from_value(argument.clone()).unwrap()
                    });
                }
                "addCustomAttributes" => {
                    validator
                        .add_custom_attributes(serde_json::from_value(argument.clone()).unwrap());
                }
                "addCustomValues" => {
                    validator.add_custom_values(serde_json::from_value(argument.clone()).unwrap());
                }
                "getRules" => {
                    let actual = validator
                        .get_rules()
                        .unwrap()
                        .into_iter()
                        .map(|(field, rules)| (field, json!(rules.len())))
                        .collect::<serde_json::Map<_, _>>();
                    assert_eq!(json!(actual), step["counts"], "序列 {index} {step}");
                }
                "passes" => {
                    let result = validator.validate().await;
                    assert_eq!(
                        result.is_ok(),
                        step["passes"].as_bool().unwrap(),
                        "序列 {index} {step}"
                    );
                    assert_eq!(
                        serde_json::to_value(validator.errors()).unwrap(),
                        step["errors"],
                        "序列 {index} {step}"
                    );
                    if let Ok(result) = result {
                        assert_eq!(result.all(), &step["validated"], "序列 {index} {step}");
                    }
                }
                method => panic!("未知操作 {method}"),
            }
        }
    }
}

#[tokio::test]
async fn rules_introspection_applies_precognitive_selection_to_expanded_fields() {
    let request = openrust::http::Request::from(
        ::http::Request::builder()
            .header("precognition-validate-only", "users.1.age")
            .body(axum::body::Body::empty())
            .unwrap(),
    );
    let rules = request
        .filter_precognitive_rules(
            RuleSet::parse([("users.*.age", "required|integer"), ("token", "required")]).unwrap(),
        )
        .unwrap();
    let mut validator = Factory::default()
        .make(
            Data::new(json!({"users":[{},{}]})).unwrap(),
            rules,
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        validator
            .get_rules()
            .unwrap()
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<Vec<_>>(),
        ["users.1.age"]
    );
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().keys(), ["users.1.age"]);
}

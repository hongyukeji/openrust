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
    assert_eq!(validator.getValue("users.0.age").unwrap(), Some(&json!(20)));
    assert_eq!(
        validator
            .getRules()
            .unwrap()
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<Vec<_>>(),
        ["users.0.age"]
    );
    validator.setValue("users.0.age", json!(16)).unwrap();
    assert!(matches!(
        validator.validated(),
        Err(ValidationError::NotValidated)
    ));
    assert!(validator.validate().await.is_err());
    assert!(!validator.errors().is_empty());
    validator.setValue("users.0.age", json!(30)).unwrap();
    assert!(validator.errors().is_empty());
    assert!(validator.validate().await.is_ok());
    assert!(validator.setValue("users.*.age", json!(0)).is_err());
    assert_eq!(validator.getValue("users.0.age").unwrap(), Some(&json!(30)));
    assert!(validator.validated().is_ok());
    validator.setRules(RuleSet::parse([("users.*.age", "integer|min:40")]).unwrap());
    assert!(validator.safe().is_err());
    assert!(validator.validate().await.is_err());
    validator.setData(Data::new(json!({"users":[{"age":50},{"age":60}]})).unwrap());
    assert_eq!(validator.getRules().unwrap().len(), 2);
    assert!(validator.validate().await.is_ok());
    assert_eq!(validator.getData().as_value()["users"][1]["age"], 60);
}

#[tokio::test]
async fn runtime_additions_reset_on_set_data_but_append_rules_becomes_initial() {
    let mut validator = validator();
    validator
        .addRules(RuleSet::parse([("users.*.age", "min:25"), ("token", "required")]).unwrap())
        .unwrap();
    assert_eq!(validator.getRules().unwrap()[0].1.len(), 3);
    assert!(validator.validate().await.is_err());
    validator.setData(Data::new(json!({"users":[{"age":20}]})).unwrap());
    assert_eq!(validator.getRules().unwrap()[0].1.len(), 2);
    assert!(validator.validate().await.is_ok());
    validator
        .appendRules(RuleSet::parse([("token", "required")]).unwrap())
        .unwrap();
    validator.setData(Data::new(json!({"users":[{"age":20}]})).unwrap());
    assert!(validator.validate().await.is_err());
    assert!(
        !validator
            .errors()
            .get("token", None)
            .intoMessages()
            .is_empty()
    );
    validator.setValue("token", json!("ok")).unwrap();
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
        .getRules()
        .unwrap()
        .iter()
        .map(|(_, rules)| rules.len())
        .collect::<Vec<_>>();
    assert!(validator.addRules(too_many).is_err());
    assert_eq!(
        before,
        validator
            .getRules()
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
    validator.setCustomMessages(BTreeMap::from([(
        "starts_with".into(),
        ":attribute :values".into(),
    )]));
    validator.setAttributeNames(BTreeMap::from([("kind".into(), "种类".into())]));
    validator.setValueNames(BTreeMap::from([(
        "kind".into(),
        BTreeMap::from([("a".into(), "甲".into())]),
    )]));
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(Some("kind"), None), "种类 甲");
    validator.setCustomMessages(BTreeMap::from([("integer".into(), "整数错误".into())]));
    validator.addCustomAttributes(BTreeMap::from([("value".into(), "数值".into())]));
    validator.addCustomValues(BTreeMap::from([(
        "kind".into(),
        BTreeMap::from([("a".into(), "A".into())]),
    )]));
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(Some("kind"), None), "种类 A");
    assert_eq!(validator.errors().first(Some("value"), None), "整数错误");
    validator
        .setAttributeNames(BTreeMap::new())
        .setValueNames(BTreeMap::new());
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
    validator.setTranslator(translator.clone());
    assert!(Arc::ptr_eq(validator.getTranslator().unwrap(), &translator));
    validator.setRules(RuleSet::parse([("missing", "required")]).unwrap());
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
            .errorBag("custom")
            .redirectTo("/retry");
        error
    }
    let mut first = validator();
    let mut second = validator();
    let default = first.getException();
    first.setException(custom);
    first.setValue("users.0.age", json!(0)).unwrap();
    second.setValue("users.0.age", json!(0)).unwrap();
    for (validator, status) in [(&mut first, 400), (&mut second, 422)] {
        let Err(ValidationError::Invalid(error)) = validator.validate().await else {
            panic!("必须返回真实验证异常");
        };
        assert_eq!(error.statusCode().as_u16(), status);
        assert!(!error.errors().is_empty());
    }
    first.setException(default);
    let Err(ValidationError::Invalid(error)) = first.validate().await else {
        panic!("必须返回真实验证异常");
    };
    assert_eq!(error.statusCode().as_u16(), 422);
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
                    .setValue(argument.as_str().unwrap(), step["arguments"][1].clone())
                    .unwrap(),
                "setData" => {
                    validator.setData(Data::new(argument.clone()).unwrap());
                }
                "setRules" => {
                    validator.setRules(rules(argument));
                }
                "addRules" => validator.addRules(rules(argument)).unwrap(),
                "appendRules" => {
                    validator.appendRules(rules(argument)).unwrap();
                }
                "setCustomMessages" => {
                    validator.setCustomMessages(serde_json::from_value(argument.clone()).unwrap());
                }
                "setAttributeNames" => {
                    validator.setAttributeNames(if argument.is_array() {
                        BTreeMap::new()
                    } else {
                        serde_json::from_value(argument.clone()).unwrap()
                    });
                }
                "setValueNames" => {
                    validator.setValueNames(if argument.is_array() {
                        BTreeMap::new()
                    } else {
                        serde_json::from_value(argument.clone()).unwrap()
                    });
                }
                "addCustomAttributes" => {
                    validator
                        .addCustomAttributes(serde_json::from_value(argument.clone()).unwrap());
                }
                "addCustomValues" => {
                    validator.addCustomValues(serde_json::from_value(argument.clone()).unwrap());
                }
                "getRules" => {
                    let actual = validator
                        .getRules()
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
        .filterPrecognitiveRules(
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
            .getRules()
            .unwrap()
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<Vec<_>>(),
        ["users.1.age"]
    );
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().keys(), ["users.1.age"]);
}

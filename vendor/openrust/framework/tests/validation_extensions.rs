use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, Rule, RuleSet, ValidationError, ValidationExtension},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn make(factory: &Factory, data: Value, spec: &str) -> openrust::validation::Validator {
    factory
        .make(
            Data::new(data).unwrap(),
            RuleSet::parse([("value", spec)]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
}

#[tokio::test]
async fn extensions_match_real_fixed_php_scheduling_parameters_and_messages() {
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-lifecycle.json")).unwrap();
    for (index, case) in fixture["extensions"].as_array().unwrap().iter().enumerate() {
        let factory = Factory::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let log = calls.clone();
        let callback = move |attribute: &str,
                             value: Option<&Value>,
                             parameters: &[String],
                             validator: &openrust::validation::Validator| {
            assert!(validator.get_value(attribute).is_ok());
            log.lock()
                .unwrap()
                .push(json!({"attribute":attribute,"value":value,"parameters":parameters}));
            Ok(value == Some(&json!("pass")))
        };
        match case["mode"].as_str().unwrap() {
            "extend" => {
                factory.extend("CustomRule", callback, Some("Invalid :attribute (:token)."))
            }
            "extendImplicit" => factory.extend_implicit(
                "CustomRule",
                callback,
                Some("Invalid :attribute (:token)."),
            ),
            "extendDependent" => factory.extend_dependent(
                "CustomRule",
                callback,
                Some("Invalid :attribute (:token)."),
            ),
            _ => unreachable!(),
        }
        factory.replacer("CustomRule", |message, _, _, parameters, _| {
            message.replace(":token", &parameters.join("/"))
        });
        let rules = RuleSet::parse(
            case["rules"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(field, spec)| (field.as_str(), spec.as_str().unwrap())),
        )
        .unwrap();
        let mut validator = factory
            .make(
                Data::new(case["data"].clone()).unwrap(),
                rules,
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            validator.validate().await.is_ok(),
            case["passes"].as_bool().unwrap(),
            "case {index}: {case}"
        );
        assert_eq!(
            serde_json::to_value(validator.errors().messages()).unwrap(),
            case["errors"],
            "case {index}: {case}"
        );
        assert_eq!(
            json!(*calls.lock().unwrap()),
            case["calls"],
            "case {index}: {case}"
        );
    }
}

#[tokio::test]
async fn factory_snapshots_and_resolver_callbacks_preserve_instance_isolation() {
    let factory = Arc::new(Factory::default());
    factory.extend("custom", |_, _, _, _| Ok(false), Some("old :attribute"));
    let mut old = make(&factory, json!({"value":"x"}), "custom");
    let reentrant = Arc::downgrade(&factory);
    let resolves = Arc::new(AtomicUsize::new(0));
    let copy = resolves.clone();
    factory.resolver(move |mut validator| {
        copy.fetch_add(1, Ordering::SeqCst);
        reentrant
            .upgrade()
            .unwrap()
            .extend("next", |_, _, _, _| Ok(true), None);
        validator.add_extension("local", |_, _, _, _| Ok(true));
        Ok(validator)
    });
    factory.extend("custom", |_, _, _, _| Ok(true), Some("new :attribute"));
    let mut current = make(&factory, json!({"value":"x"}), "custom|local");
    assert!(current.validate().await.is_ok());
    assert!(old.validate().await.is_err());
    assert_eq!(old.errors().first(None, None), "old value");
    assert_eq!(resolves.load(Ordering::SeqCst), 1);
    assert!(
        make(&factory, json!({"value":"x"}), "next")
            .validate()
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn bulk_and_single_registration_preserve_implicit_flags_and_factory_precedence() {
    let factory = Factory::default();
    factory.extend("custom", |_, _, _, _| Ok(true), None);
    factory.extend_implicit("custom", |_, _, _, _| Ok(false), Some("implicit"));
    factory.extend_dependent("custom", |_, _, _, _| Ok(true), None);
    let mut validator = make(&factory, json!({}), "nullable|custom");
    assert!(validator.validate().await.is_ok());
    let fails: ValidationExtension = Arc::new(|_, _, _, _| Ok(false));
    validator.add_implicit_extensions([("OtherRule".to_owned(), fails.clone())]);
    validator.add_dependent_extensions([("OtherRule".to_owned(), fails.clone())]);
    validator.add_extensions([("OtherRule".to_owned(), fails)]);
    validator.set_rules(RuleSet::parse([("value", "nullable|other_rule")]).unwrap());
    validator.set_custom_messages([("other_rule".into(), "missing :attribute".into())].into());
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(None, None), "missing value");
    validator.add_extension("OtherRule", |_, value, _, _| Ok(value.is_none()));
    assert!(validator.errors().is_empty());
    assert!(validator.validate().await.is_ok());
}

#[tokio::test]
async fn replacers_receive_builtin_parameters_and_follow_custom_message_selection() {
    let factory = Factory::default();
    factory.replacer("Min", |message, attribute, rule, parameters, _| {
        assert_eq!((attribute, rule), ("value", "min"));
        assert_eq!(parameters, ["3"]);
        format!("{message}/replaced")
    });
    let mut validator = make(&factory, json!({"value":1}), "Integer|min:3");
    validator
        .set_custom_messages([("value.min".into(), "minimum :min for :attribute".into())].into());
    assert!(validator.validate().await.is_err());
    assert_eq!(
        validator.errors().first(None, None),
        "minimum :min for value/replaced"
    );
    validator.set_rules(
        RuleSet::new()
            .field("value", [Rule::integer(), Rule::min(3)])
            .unwrap(),
    );
    assert!(validator.validate().await.is_err());
    assert_eq!(
        validator.errors().first(None, None),
        "minimum :min for value/replaced"
    );
    validator.add_replacer("Min", |_, _, _, parameters, _| parameters.join(","));
    assert!(validator.validate().await.is_err());
    assert_eq!(validator.errors().first(None, None), "3");
}

#[tokio::test]
async fn unknown_rules_fail_only_when_scheduled_and_callback_errors_propagate() {
    let factory = Factory::default();
    assert!(
        make(&factory, json!({}), "unregistered")
            .validate()
            .await
            .is_ok()
    );
    assert!(matches!(
        make(&factory, json!({"value":"x"}), "unregistered")
            .validate()
            .await,
        Err(ValidationError::InvalidRule)
    ));
    let mut validator = make(&factory, json!({"value":"x"}), "custom");
    validator.add_extension("custom", |_, _, _, _| Err(ValidationError::InvalidPath));
    assert!(matches!(
        validator.validate().await,
        Err(ValidationError::InvalidPath)
    ));
    validator.add_extension("custom", |_, _, _, _| Ok(true));
    assert!(validator.validate().await.is_ok());
    // 类型化异步应用规则仍沿原有入口执行。
    validator.set_rules(RuleSet::new().field("value", [Rule::required()]).unwrap());
    assert!(validator.validate().await.is_ok());
}

#[tokio::test]
async fn unvalidated_array_key_policy_matches_fixed_php_and_is_snapshotted_at_creation() {
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-lifecycle.json")).unwrap();
    for (index, case) in fixture["array_policy"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let factory = Factory::default();
        if case["exclude"].as_bool().unwrap() {
            factory.exclude_unvalidated_array_keys();
        } else {
            factory.include_unvalidated_array_keys();
        }
        let rules = RuleSet::parse(
            case["rules"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(field, spec)| (field.as_str(), spec.as_str().unwrap())),
        )
        .unwrap();
        let mut validator = factory
            .make(
                Data::new(case["data"].clone()).unwrap(),
                rules,
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            validator.exclude_unvalidated_array_keys,
            case["exclude"].as_bool().unwrap()
        );
        let result = validator.validate().await;
        assert_eq!(
            result.is_ok(),
            case["passes"].as_bool().unwrap(),
            "case {index}: {case}"
        );
        if let Ok(result) = result {
            assert_eq!(
                result.into_value(),
                case["validated"],
                "case {index}: {case}"
            );
        }
        assert_eq!(
            serde_json::to_value(validator.errors().messages()).unwrap(),
            case["errors"],
            "case {index}: {case}"
        );
    }
    let factory = Factory::default();
    let mut before = factory
        .make(
            Data::new(json!({"user":{"name":"Ada","secret":"hidden"}})).unwrap(),
            RuleSet::parse([("user", "array"), ("user.name", "string")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    factory.include_unvalidated_array_keys();
    assert_eq!(
        before.validate().await.unwrap().into_value(),
        json!({"user":{"name":"Ada"}})
    );
    before.exclude_unvalidated_array_keys = false;
    assert_eq!(
        before.validated().unwrap().into_value(),
        json!({"user":{"name":"Ada","secret":"hidden"}})
    );
    before.exclude_unvalidated_array_keys = true;
    assert_eq!(
        before.safe().unwrap().into_value(),
        json!({"user":{"name":"Ada"}})
    );
}

#[tokio::test]
async fn factory_contract_exposes_registration_to_dynamic_consumers() {
    let factory = Factory::default();
    let contract: &dyn openrust::contracts::validation::Factory = &factory;
    contract.extend(
        "custom",
        Arc::new(|_, _, _, _| Ok(false)),
        Some("bad :attribute"),
    );
    contract.extend_implicit(
        "mandatory",
        Arc::new(|_, value, _, _| Ok(value.is_some())),
        Some("missing :attribute"),
    );
    contract.replacer(
        "mandatory",
        Arc::new(|message, _, _, _, _| format!("{message}!")),
    );
    let mut validator = contract
        .make(
            Data::new(json!({"value":"x"})).unwrap(),
            RuleSet::parse([("value", "custom"), ("token", "mandatory")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_err());
    assert_eq!(
        validator.errors().all(None),
        ["bad value", "missing token!"]
    );
}

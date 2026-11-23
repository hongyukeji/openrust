use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, RuleSet},
};
use serde_json::Value;

#[tokio::test]
async fn presence_relation_and_exclusion_follow_complete_fixed_upstream_validator() {
    let reference: Value =
        serde_json::from_str(include_str!("reference/validation-lifecycle.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 25499);
    for (index, case) in cases.iter().enumerate() {
        if let Some(clock) = case.get("clock") {
            let timezone = clock["timezone"].as_str().unwrap().to_owned();
            let mut fixture = openrust::testing::TestApp::create(|builder| {
                builder.with_config(|_, _| {
                    Ok([("app".into(), serde_json::json!({"env":"testing"}))].into())
                })
            })
            .unwrap();
            let at = openrust::support::Date::fromUtc(
                clock["at"].as_str().unwrap().parse().unwrap(),
                openrust::support::DateTimezone::UTC,
            );
            fixture
                .travelToFor(at, async |_, _| {
                    openrust::support::facades::Config::set(
                        "app.timezone",
                        serde_json::json!(timezone),
                    )?;
                    assert_case(index, case).await;
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                })
                .await
                .unwrap();
        } else {
            assert_case(index, case).await;
        }
    }
}

async fn assert_case(index: usize, case: &Value) {
    let rules = case["rules"].as_object().unwrap();
    let mut validator = Factory::default()
        .make(
            Data::new(case["data"].clone()).unwrap(),
            RuleSet::parse(
                rules
                    .iter()
                    .map(|(name, rule)| (name.as_str(), rule.as_str().unwrap())),
            )
            .unwrap(),
            case.get("messages")
                .map(|value| serde_json::from_value(value.clone()).unwrap())
                .unwrap_or_default(),
            case.get("attributes")
                .map(|value| serde_json::from_value(value.clone()).unwrap())
                .unwrap_or_default(),
        )
        .unwrap();
    if case["dns_fake"].as_bool() == Some(true) {
        validator.fakeDnsLookups(true);
    }
    let result = validator.validate().await;
    assert_eq!(
        result.is_ok(),
        case["passes"].as_bool().unwrap(),
        "case {index}: {case}"
    );
    assert_eq!(
        serde_json::to_value(validator.errors()).unwrap(),
        case["errors"],
        "errors {index}: {case}"
    );
    if let Ok(result) = result {
        assert_eq!(
            result.all(),
            &case["validated"],
            "validated {index}: {case}"
        );
    }
}

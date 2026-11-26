use openrust::support::{DateFactory, facades::Date};
use serde_json::Value;
#[test]
fn original_php_format_matching_vectors_are_shared_by_facade_and_factory() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/date/format-matching-contract.json")).unwrap();
    assert_eq!(cases.len(), 45);
    let factory = DateFactory::default();
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let format = case["format"].as_str().unwrap();
        assert_eq!(
            Date::has_format(input, format).unwrap(),
            case["has"],
            "{case}"
        );
        assert_eq!(
            Date::has_format_with_modifiers(input, format).unwrap(),
            case["modifiers"],
            "{case}"
        );
        assert_eq!(factory.has_format(input, format), case["has"], "{case}");
        assert_eq!(
            factory.has_format_with_modifiers(input, format),
            case["modifiers"],
            "{case}"
        );
    }
}

#[test]
fn original_php_relative_keyword_probe_includes_null_invalid_and_wide_calendar_inputs() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/date/relative-keywords-contract.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 27);
    let factory = DateFactory::default();
    for case in cases {
        let input = case["input"].as_str();
        assert_eq!(
            Date::has_relative_keywords(input).unwrap(),
            case["relative"],
            "{case}"
        );
        assert_eq!(
            factory.has_relative_keywords(input),
            case["relative"],
            "{case}"
        );
    }
    assert!(Date::has_relative_keywords("tomorrow").unwrap());
    assert!(!Date::has_relative_keywords(None).unwrap());
}

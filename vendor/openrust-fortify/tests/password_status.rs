use openrust::support::php_integer_less_than_value;
use serde_json::Value;
#[test]
fn pinned_php_controller_mixed_seconds_comparisons() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/api-f01/fixtures/password-status.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 279);
    for case in cases {
        assert_eq!(
            php_integer_less_than_value(case["elapsed"].as_i64().unwrap(), &case["seconds"]),
            case["response"]["body"]["confirmed"].as_bool().unwrap(),
            "{case}"
        );
    }
}

use openrust_pulse::recorders::concerns::*;
use serde_json::json;
#[test]
fn php_pcre_groups_thresholds_and_deterministic_sampling_preserve_order() {
    assert!(matches(r"#^/api/(?<name>\w+)$#i", "/API/users").unwrap());
    assert!(matches(r"/中文/u", "中文").unwrap());
    assert!(matches("invalid", "value").is_err());
    assert!(matches("/ok/z", "ok").is_err());
    assert!(ignored(&json!([r"/^a/", r"/z$/"]), "xyz").unwrap());
    assert_eq!(
        group(
            &json!([[r"/users\/(\d+)/", "users:$1"], [r"/.*/", "all"]]),
            "users/12"
        )
        .unwrap(),
        "users:12"
    );
    assert_eq!(
        group(&json!([[r"/(\d+)/", r"${1}:\1"]]), "a12 b34").unwrap(),
        "a12:12 b34:34"
    );
    assert_eq!(
        threshold(&json!([["/^z/", 20], ["/.*/", 30], ["default", 40]]), "zzz").unwrap(),
        20.0
    );
    assert_eq!(threshold(&json!({"default":40}), "x").unwrap(), 40.0);
    assert_eq!(threshold(&json!(125), "x").unwrap(), 125.0);
    assert!(!sample(0.0, None).unwrap());
    assert!(sample(1.0, None).unwrap());
    assert!(sample(0.5, Some("hello")).unwrap());
    assert!(!sample(0.1, Some("hello")).unwrap());
    assert!(sample(1.1, None).is_err());
}

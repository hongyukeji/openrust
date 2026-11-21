use openrust_reverb::ReverbBroadcaster;
use serde_json::{Value, json};

fn configuration(verify: Value) -> Value {
    json!({
        "key":"test-key", "secret":"secret", "app_id":"test",
        "options":{"host":"localhost", "scheme":"https", "port":443, "path":""},
        "client_options":{"verify":verify},
    })
}

#[test]
fn verify_accepts_boolean_policy_and_rejects_invalid_or_empty_ca_files() {
    for policy in [json!(true), json!(false)] {
        assert!(ReverbBroadcaster::new(&configuration(policy)).is_ok());
    }
    for invalid in [json!(42), json!([]), json!({}), json!("")] {
        assert!(ReverbBroadcaster::new(&configuration(invalid)).is_err());
    }
    let path = std::env::temp_dir().join(format!("openrust-reverb-ca-{}", fastrand::u64(..)));
    for pem in ["", "this is not a PEM certificate"] {
        std::fs::write(&path, pem).unwrap();
        assert!(ReverbBroadcaster::new(&configuration(json!(path))).is_err());
    }
    std::fs::remove_file(path).unwrap();
}

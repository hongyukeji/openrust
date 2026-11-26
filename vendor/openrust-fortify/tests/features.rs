use openrust::{config::Repository, contracts::config::Repository as _};
use openrust_fortify::Features;
use serde_json::json;
use std::sync::Arc;
#[test]
fn feature_flags_options_and_profile_queries_follow_configuration() {
    let config = Arc::new(Repository::new(Default::default()));
    let features = Features::new(config.clone());
    assert!(!features.has_profile_features().unwrap());
    assert!(!features.has_security_features().unwrap());
    assert_eq!(
        features
            .two_factor_authentication_with_options(
                json!({"confirm":true,"confirmPassword":1})
                    .as_object()
                    .unwrap()
                    .clone()
            )
            .unwrap(),
        Features::two_factor_authentication()
    );
    assert!(
        !features
            .option_enabled(Features::two_factor_authentication(), "confirm")
            .unwrap()
    );
    config
        .set(
            "fortify.features",
            json!([Features::two_factor_authentication()]),
        )
        .unwrap();
    assert!(
        features
            .option_enabled(Features::two_factor_authentication(), "confirm")
            .unwrap()
    );
    assert!(
        !features
            .option_enabled(Features::two_factor_authentication(), "confirmPassword")
            .unwrap()
    );
    assert!(features.has_profile_features().unwrap());
    assert!(features.has_security_features().unwrap());
    features
        .two_factor_authentication_with_options(Default::default())
        .unwrap();
    assert!(
        features
            .option_enabled(Features::two_factor_authentication(), "confirm")
            .unwrap()
    );
    features
        .two_factor_authentication_with_options(json!({"window":2}).as_object().unwrap().clone())
        .unwrap();
    assert!(
        !features
            .option_enabled(Features::two_factor_authentication(), "confirm")
            .unwrap()
    );
    config
        .set(
            "fortify.features",
            json!([Features::update_profile_information()]),
        )
        .unwrap();
    assert!(features.can_update_profile_information().unwrap());
    assert!(features.has_profile_features().unwrap());
    assert!(!features.has_security_features().unwrap());
    config
        .set("fortify.features", json!([Features::passkeys()]))
        .unwrap();
    assert!(features.can_manage_passkeys().unwrap());
    assert!(features.has_security_features().unwrap());
}
#[test]
fn feature_membership_uses_php_loose_values_including_associative_arrays() {
    let config = Arc::new(Repository::new(Default::default()));
    let features = Features::new(config.clone());
    for values in [json!([true]), json!({"flag":true})] {
        config.set("fortify.features", values).unwrap();
        assert!(features.enabled("registration").unwrap());
        assert!(!features.enabled("0").unwrap());
    }
    config
        .set("fortify.features", json!([false, null, 0, [], {}]))
        .unwrap();
    assert!(!features.enabled("registration").unwrap());
    assert!(features.enabled("").unwrap());
    assert!(features.enabled("0").unwrap());
    config.set("fortify.features", json!(["0e10"])).unwrap();
    assert!(features.enabled("0e12").unwrap());
    config
        .set(
            "fortify.features",
            json!([
                Features::registration(),
                Features::reset_passwords(),
                Features::email_verification(),
                Features::update_passwords()
            ]),
        )
        .unwrap();
    assert!(features.enabled(Features::registration()).unwrap());
    assert!(features.enabled(Features::reset_passwords()).unwrap());
    assert!(features.enabled(Features::email_verification()).unwrap());
    assert!(features.can_update_passwords().unwrap());
}

#[test]
fn malformed_feature_lists_are_errors() {
    let config = Arc::new(Repository::new(Default::default()));
    let features = Features::new(config.clone());
    for value in [json!(null), json!(false), json!(1), json!("registration")] {
        config.set("fortify.features", value).unwrap();
        assert!(features.enabled("registration").is_err());
    }
}

#[test]
fn fixed_php_feature_queries() {
    fn outcome(value: Result<bool, openrust::config::ConfigError>) -> serde_json::Value {
        match value {
            Ok(value) => json!(value),
            Err(_) => json!("error"),
        }
    }
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/two_factor_oracle.json")).unwrap();
    for (index, case) in oracle["features"].as_array().unwrap().iter().enumerate() {
        let config = Arc::new(Repository::new(Default::default()));
        config
            .set("fortify.features", case["list"].clone())
            .unwrap();
        config
            .set(
                "fortify-options.two-factor-authentication.confirm",
                case["option"].clone(),
            )
            .unwrap();
        let features = Features::new(config);
        let actual = json!({
            "enabled":[outcome(features.enabled("registration")),outcome(features.enabled("0e12")),outcome(features.enabled("")),outcome(features.enabled("0"))],
            "hasProfileFeatures":outcome(features.has_profile_features()),
            "hasSecurityFeatures":outcome(features.has_security_features()),
            "canUpdateProfileInformation":outcome(features.can_update_profile_information()),
            "canUpdatePasswords":outcome(features.can_update_passwords()),
            "canManageTwoFactorAuthentication":outcome(features.can_manage_two_factor_authentication()),
            "canManagePasskeys":outcome(features.can_manage_passkeys()),
            "option":outcome(features.option_enabled(Features::two_factor_authentication(),"confirm"))
        });
        assert_eq!(actual, case["expected"], "PHP feature case {index}: {case}");
    }
}

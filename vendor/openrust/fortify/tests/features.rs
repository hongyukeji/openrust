use openrust::{config::Repository, contracts::config::Repository as _};
use openrust_fortify::Features;
use serde_json::json;
use std::sync::Arc;
#[test]
fn feature_flags_options_and_profile_queries_follow_configuration() {
    let config = Arc::new(Repository::new(Default::default()));
    let features = Features::new(config.clone());
    assert!(!features.hasProfileFeatures().unwrap());
    assert!(!features.hasSecurityFeatures().unwrap());
    assert_eq!(
        features
            .twoFactorAuthenticationWithOptions(
                json!({"confirm":true,"confirmPassword":1})
                    .as_object()
                    .unwrap()
                    .clone()
            )
            .unwrap(),
        Features::twoFactorAuthentication()
    );
    assert!(
        !features
            .optionEnabled(Features::twoFactorAuthentication(), "confirm")
            .unwrap()
    );
    config
        .set(
            "fortify.features",
            json!([Features::twoFactorAuthentication()]),
        )
        .unwrap();
    assert!(
        features
            .optionEnabled(Features::twoFactorAuthentication(), "confirm")
            .unwrap()
    );
    assert!(
        !features
            .optionEnabled(Features::twoFactorAuthentication(), "confirmPassword")
            .unwrap()
    );
    assert!(features.hasProfileFeatures().unwrap());
    assert!(features.hasSecurityFeatures().unwrap());
    features
        .twoFactorAuthenticationWithOptions(Default::default())
        .unwrap();
    assert!(
        features
            .optionEnabled(Features::twoFactorAuthentication(), "confirm")
            .unwrap()
    );
    features
        .twoFactorAuthenticationWithOptions(json!({"window":2}).as_object().unwrap().clone())
        .unwrap();
    assert!(
        !features
            .optionEnabled(Features::twoFactorAuthentication(), "confirm")
            .unwrap()
    );
    config
        .set(
            "fortify.features",
            json!([Features::updateProfileInformation()]),
        )
        .unwrap();
    assert!(features.canUpdateProfileInformation().unwrap());
    assert!(features.hasProfileFeatures().unwrap());
    assert!(!features.hasSecurityFeatures().unwrap());
    config
        .set("fortify.features", json!([Features::passkeys()]))
        .unwrap();
    assert!(features.canManagePasskeys().unwrap());
    assert!(features.hasSecurityFeatures().unwrap());
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
                Features::resetPasswords(),
                Features::emailVerification(),
                Features::updatePasswords()
            ]),
        )
        .unwrap();
    assert!(features.enabled(Features::registration()).unwrap());
    assert!(features.enabled(Features::resetPasswords()).unwrap());
    assert!(features.enabled(Features::emailVerification()).unwrap());
    assert!(features.canUpdatePasswords().unwrap());
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
            "hasProfileFeatures":outcome(features.hasProfileFeatures()),
            "hasSecurityFeatures":outcome(features.hasSecurityFeatures()),
            "canUpdateProfileInformation":outcome(features.canUpdateProfileInformation()),
            "canUpdatePasswords":outcome(features.canUpdatePasswords()),
            "canManageTwoFactorAuthentication":outcome(features.canManageTwoFactorAuthentication()),
            "canManagePasskeys":outcome(features.canManagePasskeys()),
            "option":outcome(features.optionEnabled(Features::twoFactorAuthentication(),"confirm"))
        });
        assert_eq!(actual, case["expected"], "PHP feature case {index}: {case}");
    }
}

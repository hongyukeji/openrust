use openrust::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
};
use serde_json::json;

#[tokio::test]
async fn profiles_control_views_and_features_without_changing_guard_or_prefix() {
    for (profile, views) in [("full", true), ("api", false)] {
        let mut app = Application::configure(std::env::temp_dir())
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let config = app.container().get::<dyn Repository>("config").unwrap();
        config
            .set("user", json!({"profile":profile,"registration":false}))
            .unwrap();
        config.set("fortify",json!({"guard":"admin","prefix":"auth","features":["custom","registration","passkeys"],"middleware":["web","custom-middleware"]})).unwrap();
        openrust_fortify::starter::configure(&app).await.unwrap();
        assert_eq!(config.get("fortify.views"), Some(json!(views)));
        assert_eq!(config.get("fortify.guard"), Some(json!("admin")));
        assert_eq!(config.get("fortify.prefix"), Some(json!("auth")));
        let features = config.get("fortify.features").unwrap();
        let features = features.as_array().unwrap();
        for expected in [
            "custom",
            "email-verification",
            "reset-passwords",
            "update-profile-information",
            "update-passwords",
        ] {
            assert!(features.contains(&json!(expected)), "{features:?}");
        }
        for absent in ["registration", "passkeys", "two-factor-authentication"] {
            assert!(!features.contains(&json!(absent)));
        }
        assert_eq!(
            config.get("fortify.middleware"),
            Some(json!(["web", "custom-middleware", "auth.session"]))
        );
        openrust_fortify::starter::configure(&app).await.unwrap();
        assert_eq!(
            config.get("fortify.middleware"),
            Some(json!(["web", "custom-middleware", "auth.session"]))
        );
    }
}
#[tokio::test]
async fn invalid_configuration_does_not_partially_change_fortify() {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    let original = json!({"views":false,"features":["custom"],"middleware":["web"]});
    for user in [
        json!({"profile":"other"}),
        json!({"registration":"false"}),
        json!({"token_expiration_days":0}),
        json!({"token_expiration_days":u64::MAX}),
    ] {
        config.set("fortify", original.clone()).unwrap();
        config.set("user", user).unwrap();
        assert!(openrust_fortify::starter::configure(&app).await.is_err());
        assert_eq!(config.get("fortify"), Some(original.clone()));
    }
}
#[tokio::test]
async fn extra_factors_are_only_enabled_by_explicit_configuration() {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config
        .set("user", json!({"two_factor":true,"passkeys":true}))
        .unwrap();
    openrust_fortify::starter::configure(&app).await.unwrap();
    let features = config.get("fortify.features").unwrap();
    for expected in ["two-factor-authentication", "passkeys", "registration"] {
        assert!(features.as_array().unwrap().contains(&json!(expected)));
    }
}

#[tokio::test]
async fn api_mail_redirects_follow_auth_prefix_and_preserve_explicit_destinations() {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config.set("user", json!({"profile":"api"})).unwrap();
    config
        .set("fortify", json!({"prefix":"auth","home":"/home"}))
        .unwrap();
    openrust_fortify::starter::configure(&app).await.unwrap();
    assert_eq!(
        config.get("fortify.home"),
        Some(json!("/auth/email/verify"))
    );
    assert_eq!(
        config.get("fortify.redirects.password-reset"),
        Some(json!("/auth/login"))
    );
    config
        .set(
            "fortify.home",
            json!("https://frontend.example.test/account"),
        )
        .unwrap();
    config
        .set(
            "fortify.redirects.password-reset",
            json!("https://frontend.example.test/login"),
        )
        .unwrap();
    openrust_fortify::starter::configure(&app).await.unwrap();
    assert_eq!(
        config.get("fortify.home"),
        Some(json!("https://frontend.example.test/account"))
    );
    assert_eq!(
        config.get("fortify.redirects.password-reset"),
        Some(json!("https://frontend.example.test/login"))
    );
}

#[tokio::test]
async fn disabling_verification_does_not_redirect_to_an_absent_notice() {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config
        .set("user", json!({"profile":"api","email_verification":false}))
        .unwrap();
    openrust_fortify::starter::configure(&app).await.unwrap();
    assert_eq!(config.get("fortify.home"), Some(json!("/")));
}

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use openrust_fortify::{Fortify, FortifyServiceProvider};
use std::sync::Arc;
struct Ignore;
impl ServiceProvider for Ignore {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Fortify>().await?.ignore_routes();
            Ok(())
        })
    }
}
#[tokio::test]
async fn ignored_routes_preserve_default_features_and_engine_binding() {
    let mut app = Application::configure(std::env::temp_dir())
        .with_providers(vec![Box::new(FortifyServiceProvider), Box::new(Ignore)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let fortify = app.make::<Fortify>().await.unwrap();
    assert!(!fortify.registers_routes());
    assert_eq!(fortify.username(), "email");
    let config = app
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    assert_eq!(config.get("fortify.guard"), Some(serde_json::json!("web")));
    assert_eq!(
        config
            .get("fortify.features")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        7
    );
    let engine = app
        .make::<dyn openrust_fortify::contracts::TwoFactorAuthenticationProvider>()
        .await
        .unwrap();
    assert_eq!(engine.generate_secret_key(16).unwrap().len(), 16);
    assert!(Arc::ptr_eq(&fortify, &app.make::<Fortify>().await.unwrap()));
}
#[tokio::test]
async fn missing_relying_party_configuration_refuses_passkey_routes() {
    let mut app = Application::configure(std::env::temp_dir())
        .with_providers(vec![Box::new(FortifyServiceProvider)])
        .create()
        .unwrap();
    let failure = app.boot().await.unwrap_err();
    let mut messages = vec![failure.to_string()];
    let mut source = std::error::Error::source(&failure);
    while let Some(error) = source {
        messages.push(error.to_string());
        source = error.source();
    }
    let error = messages.join(": ");
    assert!(error.contains("Invalid passkey configuration"), "{error}");
}

struct ConfirmationPolicy(bool);
impl ServiceProvider for ConfirmationPolicy {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .get::<dyn openrust::contracts::config::Repository>("config")?
                .set("fortify-options.passkeys.confirmPassword", self.0.into())?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn passkey_routes_use_fortify_prefix_guard_domain_and_confirmation_policy() {
    for confirm in [true, false] {
        let mut app = openrust::testing::TestApp::create(|builder| builder
            .with_config(|_, _| Ok([
                ("app".into(), serde_json::json!({"url":"https://example.test","key":"test-secret"})),
                ("fortify".into(), serde_json::json!({"features":["passkeys"],"prefix":"identity","domain":"example.test","guard":"members","limiters":{"passkeys":"keys"},"views":false,"passkeys":{"allowed_origins":["https://login.example.test:8443"],"user_handle_secret":"stable-secret","timeout":1234}})),
                ("fortify-options".into(), serde_json::json!({"passkeys":{"confirmPassword":true}})),
                ("passkeys".into(), serde_json::json!({"allowed_origins":["https://login.example.test:8443"],"user_handle_secret":"stable-secret","timeout":1234})),
            ].into()))
            .with_providers(vec![Box::new(FortifyServiceProvider), Box::new(ConfirmationPolicy(confirm))])
            ).unwrap();
        app.boot().await.unwrap();
        let service = app
            .app()
            .make::<openrust_fortify::passkeys::Passkeys>()
            .await
            .unwrap();
        assert_eq!(service.timeout().unwrap(), 1234);
        assert_eq!(
            service.allowed_origins().unwrap(),
            ["https://login.example.test:8443"]
        );
        assert!(!service.should_register_routes());
        let config = app
            .app()
            .container()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap();
        assert_eq!(
            config.get("passkeys.user_handle_secret"),
            Some("stable-secret".into())
        );
        let output = app
            .artisan(["route:list", "--json", "--name=passkey."])
            .await;
        output.assert_successful();
        let routes: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(routes.as_array().unwrap().len(), 7);
        for route in routes.as_array().unwrap() {
            let uri = route["uri"].as_str().unwrap();
            assert!(uri.starts_with("identity/"), "{uri}");
            assert_eq!(route["domain"], "example.test");
            let guest = route["name"].as_str().unwrap().starts_with("passkey.login");
            let middleware = route["middleware"].as_array().unwrap();
            assert!(
                middleware
                    .contains(&format!("{}:members", if guest { "guest" } else { "auth" }).into())
            );
            let management = uri.contains("/user/passkeys");
            assert_eq!(
                middleware.contains(&"password.confirm".into()),
                management && confirm
            );
            assert_eq!(
                middleware.contains(&"throttle:keys".into()),
                route["method"] != "DELETE"
            );
        }
    }
}

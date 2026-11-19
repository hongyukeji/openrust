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
            app.make::<Fortify>().await?.ignoreRoutes();
            Ok(())
        })
    }
}
#[tokio::test]
async fn ignored_routes_preserve_default_features_and_engine_binding() {
    let mut app = Application::configure(std::env::temp_dir())
        .withProviders(vec![Box::new(FortifyServiceProvider), Box::new(Ignore)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let fortify = app.make::<Fortify>().await.unwrap();
    assert!(!fortify.registersRoutes());
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
    assert_eq!(engine.generateSecretKey(16).unwrap().len(), 16);
    assert!(Arc::ptr_eq(&fortify, &app.make::<Fortify>().await.unwrap()));
}
#[tokio::test]
async fn unfinished_default_features_refuse_partial_route_registration() {
    let mut app = Application::configure(std::env::temp_dir())
        .withProviders(vec![Box::new(FortifyServiceProvider)])
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
    assert!(
        error.contains("Fortify feature reset-passwords HTTP integration is not implemented"),
        "{error}"
    );
}

mod support;
use openrust::contracts::config::Repository as _;
use openrust::{config::Value, foundation::Application};
use openrust_cashier::{CashierManager, CashierServiceProvider, Provider, facades::Cashier};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use support::*;
fn config(
    _: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([(
        "cashier".into(),
        json!({"default":"custom","drivers":{"custom":{"driver":"stripe","secret":"test","webhook_secret":"whsec_test","endpoint":"https://api.stripe.com","live":false}}}),
    )]))
}
#[tokio::test]
async fn provider_discovery_is_lazy_preserves_custom_profiles_and_facade_uses_application_scope() {
    let server = Server::new(Provider::Stripe).await;
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(config)
        .withRouting(openrust::foundation::configuration::Routing::default().api(routes))
        .withProviders(vec![Box::new(CashierServiceProvider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.make::<CashierManager>().await.unwrap();
    assert_eq!(manager.getDefaultDriver().unwrap(), "custom");
    let repo = app
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap();
    repo.set("cashier.drivers.custom.endpoint", json!(server.url))
        .unwrap();
    use openrust::contracts::http::Kernel as _;
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let response = std::sync::Arc::new(kernel)
        .handleTransport(
            http::Request::builder()
                .uri("/api/cashier-customer")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(server.requests().len(), 1);
    assert!(Cashier::defaultDriver().is_err());
    let mut other = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| Ok(BTreeMap::new()))
        .withProviders(vec![Box::new(CashierServiceProvider)])
        .create()
        .unwrap();
    other.boot().await.unwrap();
    let other = other.make::<CashierManager>().await.unwrap();
    assert_eq!(other.getDefaultDriver().unwrap(), "stripe");
    assert!(other.driver("stripe").is_err());
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/cashier-customer", customer)?;
    Ok(())
}
fn customer(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let client =
            Cashier::defaultDriver().map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        client
            .retrieve("customers", "cus_1")
            .await
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        Cashier::forgetDrivers().unwrap();
        Ok(openrust::http::Response::new(
            http::StatusCode::OK,
            openrust::http::Body::empty(),
        ))
    })
}

#[tokio::test]
async fn package_environment_defaults_load_only_after_provider_registration() {
    let path =
        std::env::temp_dir().join(format!("openrust-cashier-provider-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join(".env"),"CASHIER_DRIVER=paddle\nCASHIER_LIVE=true\nSTRIPE_SECRET=stripe_env_secret\nSTRIPE_WEBHOOK_SECRET=stripe_endpoint_secret\nPADDLE_API_KEY=lower_priority_secret\nPADDLE_AUTH_CODE=paddle_env_secret\nPADDLE_SANDBOX=true\nPADDLE_WEBHOOK_SECRET=paddle_endpoint_secret\n").unwrap();
    let mut app = Application::configure(path.clone())
        .withConfig(|_, _| Ok(BTreeMap::new()))
        .withProviders(vec![Box::new(CashierServiceProvider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap();
    let values = config.get("cashier").unwrap();
    assert_eq!(values["default"], "paddle");
    assert_eq!(values["secret"], "stripe_env_secret");
    assert_eq!(values["paddle"]["api_key"], "paddle_env_secret");
    assert_eq!(values["paddle"]["sandbox"], false);
    let manager = app.make::<CashierManager>().await.unwrap();
    assert_eq!(
        manager.driver("paddle").unwrap().provider(),
        Provider::Paddle
    );
    std::fs::write(path.join(".env"), "CASHIER_LIVE=not-a-bool\n").unwrap();
    let mut invalid = Application::configure(path.clone())
        .withConfig(|_, _| Ok(BTreeMap::new()))
        .withProviders(vec![Box::new(CashierServiceProvider)])
        .create()
        .unwrap();
    assert!(invalid.boot().await.is_err());
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn canonical_provider_configuration_and_named_account_overrides_are_distinct() {
    let server = Server::new(Provider::Stripe).await;
    let repo = Arc::new(openrust::config::Repository::new(BTreeMap::from([(
        "cashier".into(),
        json!({
            "default":"stripe", "secret":"canonical_secret", "webhook":{"secret":"whsec_canonical"},
            "paddle":{"api_key":"canonical_paddle","webhook_secret":"whsec_paddle","sandbox":true},
            "drivers":{"stripe":profile(Provider::Stripe,&server.url)}
        }),
    )])));
    let manager = CashierManager::new(
        repo.clone(),
        Arc::new(openrust::http::client::Factory::new().unwrap()),
    );
    assert_eq!(
        manager.driver("paddle").unwrap().provider(),
        Provider::Paddle
    );
    manager
        .driver("stripe")
        .unwrap()
        .retrieve("customers", "cus_1")
        .await
        .unwrap();
    assert_eq!(
        server.requests()[0].headers["authorization"],
        "Bearer api_secret_fixture"
    );
    repo.set("cashier.drivers", json!({})).unwrap();
    manager.forgetDrivers();
    assert_eq!(
        manager.driver("stripe").unwrap().provider(),
        Provider::Stripe
    );
}

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
        .with_config(config)
        .with_routing(openrust::foundation::configuration::Routing::default().api(routes))
        .with_providers(vec![Box::new(CashierServiceProvider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.make::<CashierManager>().await.unwrap();
    assert_eq!(manager.get_default_driver().unwrap(), "custom");
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
        .handle_transport(
            http::Request::builder()
                .uri("/api/cashier-customer")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(server.requests().len(), 1);
    assert!(Cashier::default_driver().is_err());
    let mut other = Application::configure(std::env::temp_dir())
        .with_config(|_, _| Ok(BTreeMap::new()))
        .with_providers(vec![Box::new(CashierServiceProvider)])
        .create()
        .unwrap();
    other.boot().await.unwrap();
    let other = other.make::<CashierManager>().await.unwrap();
    assert_eq!(other.get_default_driver().unwrap(), "stripe");
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
            Cashier::default_driver().map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        client
            .retrieve("customers", "cus_1")
            .await
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        Cashier::forget_drivers().unwrap();
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
        .with_config(|_, _| Ok(BTreeMap::new()))
        .with_providers(vec![Box::new(CashierServiceProvider)])
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
        .with_config(|_, _| Ok(BTreeMap::new()))
        .with_providers(vec![Box::new(CashierServiceProvider)])
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
    manager.forget_drivers();
    assert_eq!(
        manager.driver("stripe").unwrap().provider(),
        Provider::Stripe
    );
}

struct TransportProbe(openrust::console::CommandDefinition);

impl openrust::console::Command for TransportProbe {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            use openrust::console::ConsoleError;
            let client =
                Cashier::default_driver().map_err(|error| ConsoleError::new(error.to_string()))?;
            client
                .api(
                    http::Method::POST,
                    "/v1/customers",
                    &json!({"email":"probe@example.test"}),
                    Some("transport-artisan-probe"),
                )
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            Ok(0)
        })
    }
}

fn transport_commands() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(TransportProbe(
        openrust::console::CommandDefinition::new(clap::Command::new("billing:transport-probe")),
    )))
}

async fn framework_http(
    axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

#[tokio::test]
async fn named_http_factory_is_exercised_via_artisan_and_real_http_without_changing_other_accounts()
{
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::client::Factory,
    };
    let stripe = Server::new(Provider::Stripe).await;
    let paddle = Server::new(Provider::Paddle).await;
    let build = || {
        Application::configure(std::env::temp_dir())
            .with_config(|_, _| Ok(BTreeMap::new()))
            .with_routing(Routing::default().api(routes).commands(transport_commands))
            .with_providers(vec![Box::new(CashierServiceProvider)])
            .create()
            .unwrap()
    };
    let mut app = build();
    let mut second = build();
    let mut console = build();
    for app in [&mut app, &mut second, &mut console] {
        app.boot().await.unwrap();
        let repo = app
            .get::<dyn openrust::contracts::config::Repository>("config")
            .await
            .unwrap();
        repo.set(
            "cashier.drivers",
            json!({
                "stripe": profile(Provider::Stripe, &stripe.url),
                "paddle": profile(Provider::Paddle, &paddle.url),
            }),
        )
        .unwrap();
        repo.set("cashier.default", json!("stripe")).unwrap();
    }
    let manager = app.make::<CashierManager>().await.unwrap();
    let original = manager.driver("stripe").unwrap();
    let named = Arc::new(Factory::new().unwrap());
    named.global_request_middleware(|request| {
        request.headers_mut().insert(
            "x-cashier-probe",
            http::HeaderValue::from_static("selected"),
        );
        Ok(())
    });
    assert!(manager.set_http_factory("", Some(named.clone())).is_err());
    console
        .make::<CashierManager>()
        .await
        .unwrap()
        .set_http_factory("stripe", Some(named.clone()))
        .unwrap();
    manager.set_http_factory("stripe", Some(named)).unwrap();
    original.retrieve("customers", "cus_1").await.unwrap();
    assert_eq!(
        console
            .handle_command(&["billing:transport-probe".into()])
            .await
            .unwrap(),
        0
    );

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/api/cashier-customer",
        listener.local_addr().unwrap()
    );
    let shutdown = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .fallback(framework_http)
                .with_state(Arc::new(kernel)),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown.1.await;
        })
        .await
        .unwrap();
    });
    let http = Factory::new().unwrap();
    let request = || http.get(&url);
    assert_eq!(request().await.unwrap().status(), 200);
    assert_eq!(request().await.unwrap().status(), 200);

    manager
        .driver("paddle")
        .unwrap()
        .retrieve("customers", "cus_1")
        .await
        .unwrap();
    second
        .make::<CashierManager>()
        .await
        .unwrap()
        .driver("stripe")
        .unwrap()
        .retrieve("customers", "cus_1")
        .await
        .unwrap();
    manager.set_http_factory("stripe", None).unwrap();
    assert_eq!(request().await.unwrap().status(), 200);
    shutdown.0.send(()).unwrap();
    task.await.unwrap();

    let calls = stripe.requests();
    assert_eq!(calls.len(), 6);
    let marks: Vec<_> = calls
        .iter()
        .map(|call| call.headers.contains_key("x-cashier-probe"))
        .collect();
    assert_eq!(marks, [false, true, true, true, false, false]);
    assert_eq!(calls[1].path, "/v1/customers");
    assert_eq!(calls[1].method, http::Method::POST);
    assert_eq!(calls[1].params["email"], "probe@example.test");
    let expected = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA256,
        b"stripe:stripe\0transport-artisan-probe",
    );
    let expected = expected
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(calls[1].headers["idempotency-key"], expected);
    assert_eq!(
        calls[1].headers["stripe-version"],
        openrust_cashier::STRIPE_VERSION
    );
    let paddle = paddle.requests();
    assert_eq!(paddle.len(), 1);
    assert_eq!(paddle[0].headers["paddle-version"], "1");
    assert!(!paddle[0].headers.contains_key("x-cashier-probe"));
}

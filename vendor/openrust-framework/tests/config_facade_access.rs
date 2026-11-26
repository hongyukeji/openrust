use std::{error::Error, fs, sync::Arc};

use openrust::{
    config::{ConfigError, Repository as Settings},
    console::ConsoleError,
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::facades::{Artisan, Config, Route},
    testing::TestApp,
};
use serde_json::json;

fn routes() -> Result<(), RoutingError> {
    Route::get("/configuration", name)?;
    Route::get("/configuration/check", check)
}

fn name(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let name = Config::get("app.name").map_err(ConfigError::from)?;
        Response::json(StatusCode::OK, json!({"name":name}))
    })
}

fn check(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        assert!(Config::has("app.empty")?);
        assert_eq!(
            Config::get("app.empty").map_err(ConfigError::from)?,
            Some(json!(null))
        );
        assert_eq!(Config::get("app.missing").map_err(ConfigError::from)?, None);
        assert!(matches!(
            Config::string("app.number"),
            Err(ConfigError::InvalidType { .. })
        ));
        assert!(matches!(
            Config::float("app.number"),
            Err(ConfigError::InvalidType { .. })
        ));
        assert!(matches!(
            Config::integer("app.ratio"),
            Err(ConfigError::InvalidType { .. })
        ));
        assert_eq!(Config::integer("app.number")?, 7);
        assert_eq!(Config::float("app.ratio")?, 1.5);
        assert!(Config::boolean("app.active")?);
        assert_eq!(Config::array("app.map")?, json!({"mode":"test"}));
        assert_eq!(
            Config::collection("app.map")?.get("mode"),
            Some(&json!("test"))
        );
        Config::set("app.name", json!("Updated"))?;
        Config::push("app.list", json!("last"))?;
        Config::prepend("app.list", json!("first"))?;
        let all = Config::all()?;
        Response::json(StatusCode::OK, all["app"]["list"].clone())
    })
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command("fixture:config", |output| {
        output.comment(&Config::string("app.name").unwrap());
    })?
    .purpose("Read application configuration")
}

fn app(name: &'static str) -> Result<TestApp, Box<dyn Error + Send + Sync>> {
    let test = TestApp::create(|builder| {
        builder.with_config(|root, _| {
            let name = fs::read_to_string(root.join("name")).unwrap();
            Ok([(
                "app".into(),
                json!({"name":name,"empty":null,"number":7,"ratio":1.5,"active":true,"map":{"mode":"test"}}),
            )].into())
        }).with_routing(Routing::default().api(routes).commands(commands))
    })?;
    fs::write(test.root().join("name"), name)?;
    Ok(test)
}

#[tokio::test]
async fn config_facade_follows_the_canonical_repository_in_http_and_artisan()
-> Result<(), Box<dyn Error + Send + Sync>> {
    assert!(matches!(
        Config::get("app.name"),
        Err(BindingResolutionError::FacadeNotSet)
    ));
    let mut first = app("First")?;
    let mut second = app("Second")?;
    let (left, right) = tokio::join!(
        first.get("/api/configuration"),
        second.get("/api/configuration")
    );
    left?
        .assert_ok()
        .assert_exact_json(&json!({"name":"First"}));
    right?
        .assert_ok()
        .assert_exact_json(&json!({"name":"Second"}));
    first
        .get("/api/configuration/check")
        .await?
        .assert_ok()
        .assert_exact_json(&json!(["first", "last"]));
    assert_eq!(
        first.app().config()?.get("app.name"),
        Some(json!("Updated"))
    );
    let replacement = Arc::new(Settings::new(
        [("app".into(), json!({"name":"Replacement"}))].into(),
    ));
    first
        .app()
        .container()
        .instance_named::<dyn Repository>("config", replacement)?;
    first
        .get("/api/configuration")
        .await?
        .assert_ok()
        .assert_exact_json(&json!({"name":"Replacement"}));
    let (left, right) = tokio::join!(
        first.artisan(["fixture:config"]),
        second.artisan(["fixture:config"])
    );
    left.assert_successful().expects_output("Replacement");
    right.assert_successful().expects_output("Second");
    assert!(matches!(
        Config::get("app.name"),
        Err(BindingResolutionError::FacadeNotSet)
    ));
    Ok(())
}

#[tokio::test]
async fn uncached_config_resolves_after_swapping_and_rebinding_in_one_scope()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::{
        foundation::Application,
        support::{ProviderResult, ServiceProvider, facades::Facade},
    };

    struct Rebind;

    impl ServiceProvider for Rebind {
        fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                Config::swap(Arc::new(Settings::new(
                    [("app".into(), json!({"name":"Swapped"}))].into(),
                )))?;
                assert_eq!(Config::string("app.name")?, "Swapped");
                let replacement = Arc::new(Settings::new(
                    [("app".into(), json!({"name":"Rebound"}))].into(),
                ));
                app.container()
                    .instance_named::<dyn Repository>("config", replacement)?;
                assert_eq!(Config::string("app.name")?, "Rebound");
                assert_eq!(
                    Config::get_facade_root_async().await?.get("app.name"),
                    Some(json!("Rebound"))
                );
                Ok(())
            })
        }
    }

    let mut test = TestApp::create(|builder| builder.with_providers(vec![Box::new(Rebind)]))?;
    test.boot().await?;
    assert_eq!(test.app().config()?.get("app.name"), Some(json!("Rebound")));
    Ok(())
}

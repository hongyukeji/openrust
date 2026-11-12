use std::{error::Error, fs};

use openrust::{
    contracts::foundation::Application as _, foundation::configuration::ApplicationBuilder,
    testing::TestApp,
};
use serde_json::json;

#[tokio::test]
async fn boots_independent_apps_with_isolated_configuration_and_files()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let configure = |builder: ApplicationBuilder| {
        builder.withConfig(|root, _| {
            Ok([(
                "app".into(),
                json!({"env": "testing", "name": fs::read_to_string(root.join("name")).unwrap()}),
            )]
            .into())
        })
    };
    let mut first = TestApp::create(configure)?;
    let mut second = TestApp::create(configure)?;
    let first_root = first.root().to_path_buf();
    let second_root = second.root().to_path_buf();
    assert_ne!(first_root, second_root);
    fs::write(first_root.join("name"), "First")?;
    fs::write(second_root.join("name"), "Second")?;
    let (first_boot, second_boot) = tokio::join!(first.boot(), second.boot());
    first_boot?;
    second_boot?;

    let first_config = first.app().config()?;
    let second_config = second.app().config()?;
    assert_eq!(first_config.get("app.name"), Some(json!("First")));
    assert_eq!(second_config.get("app.name"), Some(json!("Second")));
    first_config.set("app.name", json!("Updated"))?;
    assert_eq!(second_config.get("app.name"), Some(json!("Second")));

    drop(first);
    assert!(!first_root.exists());
    assert!(second_root.join("name").is_file());
    drop(second);
    assert!(!second_root.exists());
    Ok(())
}

#[tokio::test]
async fn defaults_to_testing_environment() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| builder)?;
    test.boot().await?;
    assert_eq!(test.app().config()?.get("app.env"), Some(json!("testing")));
    Ok(())
}

#[tokio::test]
async fn dispatches_http_and_captures_artisan_output_without_leaking_state()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::{
        console::ConsoleError,
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response, StatusCode},
        routing::RoutingError,
        support::facades::{Artisan, Route},
    };

    fn answer(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { Response::json(StatusCode::OK, json!({"fixture": true})) })
    }
    fn routes() -> Result<(), RoutingError> {
        Route::get("/fixture", answer)
    }
    fn commands() -> Result<(), ConsoleError> {
        Artisan::command("fixture:probe", |output| output.comment("fixture output"))?
            .purpose("Verify the console fixture")
    }

    let mut fixture = TestApp::create(|builder| {
        builder.withRouting(Routing::default().web(routes).commands(commands))
    })?;
    fixture
        .get("/fixture")
        .await?
        .assertOk()
        .assertHeader("content-type", "application/json")
        .assertExactJson(&json!({"fixture": true}));
    fixture.get("/fixture").await?.assertOk();

    let command = fixture.artisan(["fixture:probe"]).await;
    command.assertSuccessful().expectsOutput("fixture output");
    assert!(command.stderr.is_empty());

    let mut other = TestApp::create(|builder| builder)?;
    other
        .get("/fixture")
        .await?
        .assertStatus(StatusCode::NOT_FOUND);
    let missing = other.artisan(["fixture:probe"]).await;
    missing.assertExitCode(1);
    assert!(missing.stdout.is_empty());
    assert!(missing.stderr.contains("fixture:probe"));
    Ok(())
}

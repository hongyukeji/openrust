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

#[tokio::test]
async fn artisan_publishes_registered_package_assets_and_migrations()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::{
        foundation::Application,
        http::HttpFuture,
        support::{ProviderResult, ServiceProvider},
    };

    struct Package<const ID: u8>;

    impl<const ID: u8> ServiceProvider for Package<ID> {
        fn name(&self) -> &'static str {
            match ID {
                1 => "FixturePackage",
                _ => "OtherPackage",
            }
        }

        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                if ID == 1 {
                    self.publishes(app, &[("config/fixture.rs", b"config")], "fixture")?;
                    self.publishesMigrations(
                        app,
                        &[(
                            "database/migrations/2000_01_01_000000_add_fixture.rs",
                            b"migration",
                        )],
                        "fixture-migrations",
                    )?;
                } else {
                    self.publishes(app, &[("public/fixture.css", b"stylesheet")], "fixture")?;
                }
                Ok(())
            })
        }
    }

    let mut test = TestApp::create(|builder| {
        builder.withProviders(vec![Box::new(Package::<1>), Box::new(Package::<2>)])
    })?;
    let root = test.root().to_owned();
    let config = root.join("config/fixture.rs");
    let stylesheet = root.join("public/fixture.css");
    let migrations = root.join("database/migrations");

    test.artisan(["vendor:publish", "--no-interaction"])
        .await
        .assertSuccessful();
    test.artisan(["vendor:publish", "--all", "--existing"])
        .await
        .assertSuccessful();
    assert!(!config.exists());
    assert!(!migrations.exists());

    let result = test
        .artisan([
            "vendor:publish",
            "--provider=FixturePackage",
            "--tag=fixture",
        ])
        .await;
    result.assertSuccessful();
    assert!(result.stdout.contains("Published config/fixture.rs."));
    assert_eq!(fs::read(&config)?, b"config");
    assert!(!stylesheet.exists());
    assert!(!migrations.exists());

    test.artisan(["vendor:publish", "--all"])
        .await
        .assertSuccessful();
    assert_eq!(fs::read(&stylesheet)?, b"stylesheet");
    let migration = fs::read_dir(&migrations)?.next().unwrap()?.path();
    let name = migration.file_name().unwrap().to_str().unwrap();
    assert!(name.ends_with("_add_fixture.rs"));
    assert_ne!(&name[..17], "2000_01_01_000000");
    chrono::NaiveDateTime::parse_from_str(&name[..17], "%Y_%m_%d_%H%M%S")?;
    assert_eq!(fs::read(&migration)?, b"migration");

    fs::write(&migration, "local edit")?;
    test.artisan(["vendor:publish", "--tag=fixture-migrations"])
        .await
        .assertSuccessful();
    assert_eq!(fs::read(&migration)?, b"local edit");
    test.artisan(["vendor:publish", "--tag=fixture-migrations", "--existing"])
        .await
        .assertSuccessful();
    assert_eq!(fs::read(&migration)?, b"migration");
    test.artisan(["vendor:publish", "--tag=fixture-migrations", "--force"])
        .await
        .assertSuccessful();
    assert_eq!(fs::read_dir(&migrations)?.count(), 1);

    let mut other = TestApp::create(|builder| builder)?;
    let result = other.artisan(["vendor:publish", "--tag=fixture"]).await;
    result.assertSuccessful();
    assert!(result.stdout.contains("No resources published."));
    assert!(!other.root().join("config/fixture.rs").exists());
    Ok(())
}

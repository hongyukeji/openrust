use std::{error::Error, fs};

use openrust::{
    contracts::foundation::Application as _, foundation::configuration::ApplicationBuilder,
    testing::TestApp,
};
use serde_json::json;

#[test]
fn test_app_does_not_change_an_inherited_route_cache() {
    let root = tempfile::tempdir().unwrap();
    let deployed_cache = root.path().join("deployed-routes.json");
    fs::write(&deployed_cache, b"unchanged deployment cache").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "test_app_route_cache_fixture"])
        .env("APP_ROUTES_CACHE", &deployed_cache)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(deployed_cache).unwrap(),
        b"unchanged deployment cache"
    );
}

#[tokio::test]
#[ignore = "Run with a separate deployment environment by the parent test"]
async fn test_app_route_cache_fixture() -> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::{foundation::configuration::Routing, support::facades::Route, view::view};

    let mut fixture = TestApp::create(|builder| {
        builder.with_routing(Routing::default().web(|| Route::get("/probe", || view("welcome"))))
    })?;
    fs::write(
        fixture.root().join(".env.testing"),
        "APP_ROUTES_CACHE=private/fixture.json\n",
    )?;
    let cache = fixture.root().canonicalize()?.join("private/fixture.json");
    fixture.artisan(["route:cache"]).await.assert_successful();
    assert_eq!(
        fixture.app().get_cached_routes_path(),
        Some(cache.as_path())
    );
    assert!(cache.is_file());
    fixture.artisan(["route:clear"]).await.assert_successful();
    assert!(!cache.exists());
    Ok(())
}

#[tokio::test]
async fn boots_independent_apps_with_isolated_configuration_and_files()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let configure = |builder: ApplicationBuilder| {
        builder.with_config(|root, _| {
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
async fn selects_local_testing_environment_on_http_and_artisan_first_boot()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::{
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::{Config, Route},
    };

    fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            Response::json(
                StatusCode::OK,
                json!({"label": Config::string_or("app.label", "")}),
            )
        })
    }

    let configure = |builder: ApplicationBuilder| {
        builder
            .with_routing(Routing::default().web(|| Route::get("/fixture", probe)))
            .with_config(|_, env| {
                Ok([(
                    "app".into(),
                    json!({
                        "env": env.get_utf8("APP_ENV")?,
                        "label": env.get_utf8("OPENRUST_T01_FIXTURE")?,
                    }),
                )]
                .into())
            })
    };
    let mut first = TestApp::create(configure)?;
    let mut second = TestApp::create(configure)?;
    let mut fallback = TestApp::create(configure)?;
    for fixture in [&first, &second, &fallback] {
        fs::write(
            fixture.root().join(".env"),
            "APP_ENV=production\nOPENRUST_T01_FIXTURE=fallback\n",
        )?;
    }
    fs::write(
        first.root().join(".env.testing"),
        "OPENRUST_T01_FIXTURE=first\n",
    )?;
    fs::write(
        second.root().join(".env.testing"),
        "OPENRUST_T01_FIXTURE=second\n",
    )?;

    let (http, artisan) = tokio::join!(
        first.get("/fixture"),
        second.artisan(["config:show", "app.label"])
    );
    http?
        .assert_ok()
        .assert_exact_json(&json!({"label": "first"}));
    artisan
        .assert_successful()
        .expects_output("  app.label  second");
    second
        .get("/fixture")
        .await?
        .assert_exact_json(&json!({"label": "second"}));
    fallback
        .get("/fixture")
        .await?
        .assert_exact_json(&json!({"label": "fallback"}));
    for fixture in [&first, &second, &fallback] {
        assert_eq!(
            fixture.app().config()?.get("app.env"),
            Some(json!("testing"))
        );
    }

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
        builder.with_routing(Routing::default().web(routes).commands(commands))
    })?;
    fixture
        .get("/fixture")
        .await?
        .assert_ok()
        .assert_header("content-type", "application/json")
        .assert_exact_json(&json!({"fixture": true}));
    fixture.get("/fixture").await?.assert_ok();

    let command = fixture.artisan(["fixture:probe"]).await;
    command.assert_successful().expects_output("fixture output");
    assert!(command.stderr.is_empty());

    let mut other = TestApp::create(|builder| builder)?;
    other
        .get("/fixture")
        .await?
        .assert_status(StatusCode::NOT_FOUND);
    let missing = other.artisan(["fixture:probe"]).await;
    missing.assert_exit_code(1);
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
                    self.publishes_migrations(
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
        builder
            .with_config(|_, _| {
                Ok([(
                    "database".into(),
                    serde_json::json!({
                        "migrations": {"update_date_on_publish": true}
                    }),
                )]
                .into())
            })
            .with_providers(vec![Box::new(Package::<1>), Box::new(Package::<2>)])
    })?;
    let root = test.root().to_owned();
    let config = root.join("config/fixture.rs");
    let stylesheet = root.join("public/fixture.css");
    let migrations = root.join("database/migrations");

    test.artisan(["vendor:publish", "--no-interaction"])
        .await
        .assert_successful();
    let absent = test
        .artisan(["vendor:publish", "--all", "--existing"])
        .await;
    absent.assert_successful();
    assert!(
        absent
            .stdout
            .contains("File [config/fixture.rs] does not exist SKIPPED")
    );
    assert!(!config.exists());
    assert!(!migrations.exists());

    let result = test
        .artisan([
            "vendor:publish",
            "--provider=FixturePackage",
            "--tag=fixture",
        ])
        .await;
    result.assert_successful();
    assert!(result.stdout.contains("Publishing [fixture] assets"));
    assert!(result.stdout.contains("Copying file [embedded"));
    assert!(result.stdout.contains("to [config/fixture.rs]"));
    let skipped = test
        .artisan([
            "vendor:publish",
            "--provider=FixturePackage",
            "--tag=fixture",
        ])
        .await;
    assert!(
        skipped
            .stdout
            .contains("File [config/fixture.rs] already exists SKIPPED")
    );
    assert_eq!(fs::read(&config)?, b"config");
    assert!(!stylesheet.exists());
    assert!(!migrations.exists());

    test.artisan(["vendor:publish", "--all"])
        .await
        .assert_successful();
    assert_eq!(fs::read(&stylesheet)?, b"stylesheet");
    let published = fs::read_dir(&migrations)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    // --all 同时发布已启用的 Sanctum 迁移，文件系统不保证目录枚举顺序。
    assert_eq!(
        published.len(),
        if cfg!(feature = "sanctum") { 2 } else { 1 }
    );
    let migration = published
        .iter()
        .find(|path| path.to_string_lossy().ends_with("_add_fixture.rs"))
        .cloned()
        .expect("应发布夹具迁移");
    #[cfg(feature = "sanctum")]
    assert!(published.iter().any(|path| {
        path.to_string_lossy()
            .ends_with("_create_personal_access_tokens_table.rs")
    }));
    let name = migration.file_name().unwrap().to_str().unwrap();
    assert!(name.ends_with("_add_fixture.rs"));
    assert_ne!(&name[..17], "2000_01_01_000000");
    jiff::civil::DateTime::strptime("%Y_%m_%d_%H%M%S", &name[..17])?;
    assert_eq!(fs::read(&migration)?, b"migration");

    fs::write(&migration, "local edit")?;
    test.artisan(["vendor:publish", "--tag=fixture-migrations"])
        .await
        .assert_successful();
    assert_eq!(fs::read(&migration)?, b"local edit");
    test.artisan(["vendor:publish", "--tag=fixture-migrations", "--existing"])
        .await
        .assert_successful();
    assert_eq!(fs::read(&migration)?, b"migration");
    test.artisan(["vendor:publish", "--tag=fixture-migrations", "--force"])
        .await
        .assert_successful();
    assert_eq!(fs::read_dir(&migrations)?.count(), published.len());

    let mut other = TestApp::create(|builder| builder)?;
    let result = other.artisan(["vendor:publish", "--tag=fixture"]).await;
    result.assert_successful();
    assert!(
        result
            .stdout
            .contains("No publishable resources for tag [fixture].")
    );
    assert!(!other.root().join("config/fixture.rs").exists());
    Ok(())
}

#[tokio::test]
async fn artisan_honors_migration_publish_settings_and_intersects_source_paths()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use std::path::Path;

    use openrust::{
        foundation::{Application, console::VendorPublishCommand},
        http::HttpFuture,
        support::{ProviderResult, ServiceProvider},
    };

    struct Package<const ID: u8>;
    impl<const ID: u8> ServiceProvider for Package<ID> {
        fn name(&self) -> &'static str {
            match ID {
                1 => "FirstPackage",
                2 => "SecondPackage",
                _ => "FixtureWithoutDateUpdates",
            }
        }

        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                if ID == 3 {
                    VendorPublishCommand::dont_update_migration_dates(app);
                    return Ok(());
                }
                self.publishes_migration_paths(
                    app,
                    &[(
                        Path::new("package/migrations"),
                        Path::new("database/migrations"),
                    )],
                    "migrations",
                )?;
                let (target, tag) = if ID == 1 {
                    ("public/first.txt", "first")
                } else {
                    ("public/second.txt", "shared")
                };
                self.publishes_paths(
                    app,
                    &[(Path::new("package/source.txt"), Path::new(target))],
                    tag,
                )
            })
        }
    }

    let source_name = "2000_01_01_000000_create_sample.rs";
    for (enabled, opt_out) in [(false, false), (true, true), (true, false)] {
        let mut app = TestApp::create(|builder| {
            let mut providers: Vec<Box<dyn ServiceProvider>> =
                vec![Box::new(Package::<1>), Box::new(Package::<2>)];
            if opt_out {
                providers.push(Box::new(Package::<3>));
            }
            builder
                .with_config(|root, _| {
                    Ok([(
                        "database".into(),
                        json!({"migrations":{"update_date_on_publish":root.join("update-dates").is_file()}}),
                    )]
                    .into())
                })
                .with_providers(providers)
        })?;
        let root = app.root().to_owned();
        if enabled {
            fs::write(root.join("update-dates"), "")?;
        }
        fs::create_dir_all(root.join("package/migrations"))?;
        fs::write(
            root.join("package/migrations").join(source_name),
            "migration",
        )?;
        fs::write(root.join("package/source.txt"), "asset")?;

        app.artisan(["vendor:publish", "--provider=FirstPackage", "--tag=shared"])
            .await
            .assert_successful();
        assert_eq!(fs::read_to_string(root.join("public/first.txt"))?, "asset");
        assert!(!root.join("public/second.txt").exists());

        app.artisan(["vendor:publish", "--tag=migrations"])
            .await
            .assert_successful();
        let directory = root.join("database/migrations");
        let name = fs::read_dir(&directory)?.next().unwrap()?.file_name();
        assert_eq!(name == source_name, !enabled || opt_out);
        assert_eq!(fs::read_to_string(directory.join(&name))?, "migration");
        app.artisan(["vendor:publish", "--tag=migrations", "--force"])
            .await
            .assert_successful();
        assert_eq!(fs::read_dir(&directory)?.count(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn artisan_publishes_directories_and_dispatches_registered_paths()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use std::path::Path;

    use openrust::{
        contracts::events::Event as _,
        foundation::{Application, events::VendorTagPublished},
        http::HttpFuture,
        support::{ProviderResult, ServiceProvider},
    };

    struct Assets;

    impl ServiceProvider for Assets {
        fn name(&self) -> &'static str {
            "ThirdPartyAssets"
        }

        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                let directory = (Path::new("package/assets"), Path::new("public/package"));
                self.publishes_paths(app, &[directory], "package-assets")?;
                self.publishes_paths(app, &[directory], "package-alias")?;
                self.publishes_paths(
                    app,
                    &[(Path::new("package/missing"), Path::new("public/missing"))],
                    "package-missing",
                )?;
                Ok(())
            })
        }
    }

    let mut app = TestApp::create(|builder| builder.with_providers(vec![Box::new(Assets)]))?;
    let source = app.root().join("package/assets");
    let target = app.root().join("public/package");
    fs::create_dir_all(source.join("nested/empty"))?;
    fs::write(source.join("nested/site.css"), "package version")?;
    fs::write(source.join("asset.bin"), [0, 255, 42])?;
    fs::write(source.join(".hidden"), "hidden asset")?;
    fs::create_dir_all(target.join("nested"))?;
    fs::write(target.join("nested/site.css"), "local edit")?;
    let name = "OpenRust\\Foundation\\Events\\VendorTagPublished";
    let events = app.fake_events(&[name]).await?;

    app.artisan(["vendor:publish", "--tag=unknown"])
        .await
        .assert_successful();
    app.artisan(["vendor:publish", "--no-interaction"])
        .await
        .assert_successful();
    events.assert_dispatched_times(name, 0);

    app.artisan([
        "vendor:publish",
        "--tag=package-assets",
        "--existing",
        "--force",
    ])
    .await
    .assert_successful();
    assert_eq!(
        fs::read(target.join("nested/site.css"))?,
        b"package version"
    );
    assert!(!target.join("asset.bin").exists());

    fs::write(target.join("nested/site.css"), "local edit")?;
    let directory = app
        .artisan(["vendor:publish", "--tag=package-assets"])
        .await;
    directory.assert_successful();
    assert!(directory.stdout.contains("Copying directory"));
    assert!(directory.stdout.contains("to [public"));
    assert_eq!(fs::read(target.join("nested/site.css"))?, b"local edit");
    assert_eq!(fs::read(target.join("asset.bin"))?, [0, 255, 42]);
    assert_eq!(fs::read(target.join(".hidden"))?, b"hidden asset");
    assert!(!target.join("nested/empty").exists());

    app.artisan(["vendor:publish", "--tag=package-assets"])
        .await
        .assert_successful();
    app.artisan([
        "vendor:publish",
        "--tag=package-assets",
        "--tag=package-alias",
        "--tag=unknown",
        "--force",
    ])
    .await
    .assert_successful();
    assert_eq!(
        fs::read(target.join("nested/site.css"))?,
        b"package version"
    );

    let missing = app
        .artisan(["vendor:publish", "--tag=package-missing"])
        .await;
    missing.assert_successful();
    assert!(missing.stderr.contains("Can't locate path"));
    app.artisan(["vendor:publish", "--all", "--tag=unknown"])
        .await
        .assert_successful();

    events.assert_dispatched_times(name, 7);
    let published = events.dispatched(name);
    for (event, expected) in published.iter().zip([
        Some("package-assets"),
        Some("package-assets"),
        Some("package-assets"),
        Some("package-assets"),
        Some("package-alias"),
        Some("package-missing"),
        None,
    ]) {
        let event = event.downcast_ref::<VendorTagPublished>().unwrap();
        assert_eq!(event.name(), name);
        assert_eq!(event.tag.as_deref(), expected);
        if expected != Some("package-missing") {
            assert_eq!(event.paths.get(&source), Some(&target));
        }
        if expected.is_none() || expected == Some("package-missing") {
            assert_eq!(
                event.paths.get(&app.root().join("package/missing")),
                Some(&app.root().join("public/missing")),
            );
        } else {
            assert_eq!(event.paths.len(), 1);
        }
    }
    Ok(())
}

#[tokio::test]
async fn publishes_pagination_templates_and_preserves_overrides()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    let destination = app.root().join("resources/views/vendor/pagination");
    app.artisan(["vendor:publish", "--tag=laravel-pagination"])
        .await
        .assert_successful();
    for name in [
        "tailwind",
        "simple-tailwind",
        "bootstrap-3",
        "simple-bootstrap-3",
        "bootstrap-4",
        "simple-bootstrap-4",
        "bootstrap-5",
        "simple-bootstrap-5",
        "semantic-ui",
    ] {
        let file = destination.join(format!("{name}.blade.html"));
        assert!(fs::read_to_string(file)?.contains("$paginator->has_pages"));
    }
    let file = destination.join("bootstrap-5.blade.html");
    let original = fs::read(&file)?;
    fs::write(&file, "my pagination view")?;
    app.artisan(["vendor:publish", "--tag=laravel-pagination"])
        .await
        .assert_successful();
    assert_eq!(fs::read_to_string(&file)?, "my pagination view");
    app.artisan(["vendor:publish", "--tag=laravel-pagination", "--force"])
        .await
        .assert_successful();
    assert_eq!(fs::read(file)?, original);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn artisan_rejects_publish_paths_through_application_symlinks()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use std::{os::unix::fs::symlink, path::Path};

    use openrust::{
        foundation::Application,
        http::HttpFuture,
        support::{ProviderResult, ServiceProvider},
    };

    struct Assets;
    impl ServiceProvider for Assets {
        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                self.publishes_paths(
                    app,
                    &[(
                        Path::new("package/link/source.txt"),
                        Path::new("public/unsafe.txt"),
                    )],
                    "source-link",
                )?;
                self.publishes(
                    app,
                    &[("public/link/target.txt", b"overwrite")],
                    "target-link",
                )?;
                self.publishes_migrations(
                    app,
                    &[(
                        "database/migrations/2000_01_01_000000_example.rs",
                        b"migration",
                    )],
                    "migration-link",
                )
            })
        }
    }

    let outside = TestApp::create(|builder| builder)?;
    let outside = outside.root();
    for (name, contents) in [
        ("source.txt", "outside source"),
        ("target.txt", "outside target"),
    ] {
        fs::write(outside.join(name), contents)?;
    }

    let mut app = TestApp::create(|builder| builder.with_providers(vec![Box::new(Assets)]))?;
    let root = app.root().to_owned();
    for directory in ["package", "public", "database"] {
        fs::create_dir_all(root.join(directory))?;
    }
    symlink(outside, root.join("package/link"))?;
    symlink(outside, root.join("public/link"))?;
    symlink(outside, root.join("database/migrations"))?;
    for tag in ["source-link", "target-link", "migration-link"] {
        let result = app
            .artisan(["vendor:publish", &format!("--tag={tag}"), "--force"])
            .await;
        result.assert_exit_code(1);
        assert!(
            result.stderr.contains("Unsupported publish path"),
            "{}",
            result.stderr
        );
    }
    assert!(!root.join("public/unsafe.txt").exists());
    assert_eq!(
        fs::read_to_string(outside.join("target.txt"))?,
        "outside target"
    );
    assert!(!outside.join("2000_01_01_000000_example.rs").exists());
    Ok(())
}

#[tokio::test]
async fn package_manifest_rebuilds_clears_and_uses_linked_providers()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::{
        foundation::{Application, DiscoveredPackage, PackageManifest, configuration::Routing},
        http::{HttpError, HttpFuture, Request, Response, StatusCode},
        routing::RoutingError,
        support::{ProviderResult, ServiceProvider, facades::Route},
    };

    struct Provider;
    impl ServiceProvider for Provider {
        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async { self.load_routes_from(app, package_routes) })
        }
    }
    fn providers() -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(Provider)]
    }
    fn package_routes() -> Result<(), RoutingError> {
        fn endpoint(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
            Box::pin(async { Ok(Response::new(StatusCode::OK, "linked provider".into())) })
        }
        Route::get("/discovered", endpoint)
    }

    static ENTRIES: &[DiscoveredPackage] = &[DiscoveredPackage::new(
        "fixture",
        "1.0.0",
        "renamed_dependency",
        &[],
        providers,
    )];

    let mut app = TestApp::create(|builder| {
        builder
            .with_packages(PackageManifest::new(ENTRIES))
            .with_routing(Routing::default())
    })?;
    let cache = app
        .root()
        .canonicalize()?
        .join("bootstrap/cache/packages.json");
    assert!(app.app().get_cached_packages_path().is_none());
    app.get("/discovered").await?.assert_ok();
    assert_eq!(app.app().get_cached_packages_path(), Some(cache.as_path()));
    assert!(!cache.exists());
    app.artisan(["package:discover"]).await.assert_successful();
    let original = fs::read(&cache)?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&original)?[0]["name"],
        "fixture"
    );
    fs::write(&cache, r#"[{"name":"unlinked"}]"#)?;
    app.get("/discovered").await?.assert_ok();
    app.artisan(["package:discover"]).await.assert_successful();
    assert_eq!(fs::read(&cache)?, original);
    fs::write(cache.with_file_name("unrelated.json"), "keep")?;
    app.artisan(["clear-compiled"]).await.assert_successful();
    assert!(!cache.exists());
    assert!(cache.with_file_name("unrelated.json").is_file());
    app.artisan(["clear-compiled"]).await.assert_successful();
    app.get("/discovered").await?.assert_ok();
    assert!(!cache.exists());

    let mut excluded = TestApp::create(|builder| builder.with_routing(Routing::default()))?;
    excluded
        .get("/discovered")
        .await?
        .assert_status(openrust::http::StatusCode::NOT_FOUND);
    assert!(
        !excluded
            .root()
            .join("bootstrap/cache/packages.json")
            .exists()
    );
    Ok(())
}

#[tokio::test]
async fn relative_test_requests_use_configured_origin_and_preserve_absolute_addresses() {
    use openrust::{
        foundation::configuration::Routing,
        http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::Route,
    };
    fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(
            async move { Response::json(StatusCode::OK, json!({"uri":request.uri().to_string()})) },
        )
    }
    for (url, relative, expected) in [
        (None, "/probe?x=1", "http://localhost/probe?x=1"),
        (
            Some("https://example.test"),
            "/probe?x=1",
            "https://example.test/probe?x=1",
        ),
        (
            Some("https://example.test/sub"),
            "/probe?x=1",
            "https://example.test/sub/probe?x=1",
        ),
    ] {
        let mut app = TestApp::create(|builder| {
            builder
                .with_config(|root, _| {
                    let url = fs::read_to_string(root.join("test-url")).ok();
                    Ok([("app".into(), json!({"env":"testing","url":url}))].into())
                })
                .with_routing(Routing::default().api_prefix("").api(|| {
                    Route::get("/probe", probe)?;
                    Route::get("/sub/probe", probe)
                }))
        })
        .unwrap();
        if let Some(url) = url {
            fs::write(app.root().join("test-url"), url).unwrap();
        }
        app.get(relative)
            .await
            .unwrap()
            .assert_json_path("uri", &json!(expected));
        app.call(
            http::Request::get("https://absolute.test/probe?x=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .assert_json_path("uri", &json!("https://absolute.test/probe?x=2"));
    }
}

use openrust::{
    contracts::{container::Container as _, foundation::Application as Contract},
    foundation::Application,
    support::{facades::App, lang_path},
    testing::TestApp,
    translation::Translator,
};
use std::{collections::BTreeMap, fs, path::PathBuf};

struct PackageProvider;
impl openrust::support::ServiceProvider for PackageProvider {
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            let source = app.base_path().join("package/en/messages.json");
            self.publishes_paths(
                app,
                &[(
                    source.as_path(),
                    app.lang_path("vendor/probe/en/messages.json").as_path(),
                )],
                "probe-lang",
            )?;
            self.load_translations_from(app, &app.base_path().join("package"), "probe")
                .await
        })
    }
}

#[test]
fn language_path_initialization_and_contract_preserve_legacy_priority() {
    for legacy in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if legacy {
            fs::create_dir_all(root.path().join("resources/lang")).unwrap();
        }
        let app = Application::configure(root.path().to_path_buf())
            .create()
            .unwrap();
        let expected = app
            .base_path()
            .join(if legacy { "resources/lang" } else { "lang" });
        assert_eq!(
            app.lang_path("en/messages.json"),
            expected.join("en/messages.json")
        );
        assert_eq!(Contract::lang_path(&app, ""), expected);
        assert_eq!(
            *app.container().get::<PathBuf>("path.lang").unwrap(),
            expected
        );
        assert!(!app.base_path().join("lang").exists());
        let custom = root.path().join("custom/languages");
        app.use_lang_path(custom.clone()).unwrap();
        assert_eq!(Contract::lang_path(&app, "en"), custom.join("en"));
        assert_eq!(
            *app.container().get::<PathBuf>("path.lang").unwrap(),
            custom
        );
    }
}

#[tokio::test]
async fn custom_language_path_connects_loader_publisher_facade_and_package_overrides() {
    for external in [false, true] {
        let external_root = tempfile::tempdir().unwrap();
        let mut app = TestApp::create(|b| {
            b.with_providers(vec![Box::new(PackageProvider)])
                .with_config(|_, _| {
                    Ok(BTreeMap::from([(
                        "app".into(),
                        serde_json::json!({"locale":"en","fallback_locale":"en"}),
                    )]))
                })
                .with_commands(|artisan| {
                    artisan.command("path:probe", |output| {
                        assert_eq!(App::lang_path("").unwrap(), lang_path("").unwrap());
                        assert_eq!(
                            openrust::support::facades::Lang::get(
                                "messages.greeting",
                                &BTreeMap::new(),
                                None
                            )
                            .unwrap(),
                            "custom"
                        );
                        assert_eq!(
                            openrust::translation::__("messages.greeting", &BTreeMap::new())
                                .unwrap(),
                            "custom"
                        );
                        let current = App::use_lang_path(lang_path("").unwrap()).unwrap();
                        assert_eq!(current.lang_path(""), App::lang_path("").unwrap());
                        output
                            .stdout
                            .push_str(&lang_path("").unwrap().to_string_lossy());
                    })?;
                    Ok(())
                })
        })
        .unwrap();
        let custom = if external {
            external_root.path().join("languages")
        } else {
            app.root().join("custom/languages")
        };
        fs::create_dir_all(custom.join("en")).unwrap();
        fs::write(custom.join("en/messages.json"), r#"{"greeting":"custom"}"#).unwrap();
        let package = app.root().join("package/en");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("messages.json"), r#"{"greeting":"package"}"#).unwrap();
        app.app().use_lang_path(custom.clone()).unwrap();
        let command = app.artisan(["path:probe"]).await;
        command.assert_successful();
        assert_eq!(command.stdout, custom.to_string_lossy());
        let translator = app.app().make::<Translator>().await.unwrap();
        assert_eq!(
            translator
                .string("messages.greeting", &BTreeMap::new(), None)
                .unwrap(),
            "custom"
        );
        let package = app.root().join("package/en");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("messages.json"), r#"{"greeting":"package"}"#).unwrap();
        let overrides = custom.join("vendor/probe/en");
        fs::create_dir_all(&overrides).unwrap();
        fs::write(
            overrides.join("messages.json"),
            r#"{"greeting":"override"}"#,
        )
        .unwrap();
        assert_eq!(
            translator
                .string("probe::messages.greeting", &BTreeMap::new(), None)
                .unwrap(),
            "override"
        );
        let package_override = overrides.join("messages.json");
        fs::remove_file(&package_override).unwrap();
        app.artisan(["vendor:publish", "--tag=probe-lang"])
            .await
            .assert_successful();
        assert!(
            fs::read_to_string(&package_override)
                .unwrap()
                .contains("package")
        );
        fs::write(&package_override, "custom package").unwrap();
        app.artisan(["vendor:publish", "--tag=probe-lang"])
            .await
            .assert_successful();
        assert_eq!(
            fs::read_to_string(&package_override).unwrap(),
            "custom package"
        );
        app.artisan(["vendor:publish", "--tag=probe-lang", "--force"])
            .await
            .assert_successful();
        assert!(
            fs::read_to_string(&package_override)
                .unwrap()
                .contains("package")
        );
        app.artisan(["lang:publish"]).await.assert_successful();
        assert!(custom.join("en/auth.json").is_file());
        assert!(!app.root().join("lang").exists());
        let auth = custom.join("en/auth.json");
        fs::write(&auth, r#"{"failed":"mine"}"#).unwrap();
        app.artisan(["lang:publish"]).await.assert_successful();
        assert!(fs::read_to_string(&auth).unwrap().contains("mine"));
        app.artisan(["lang:publish", "--force"])
            .await
            .assert_successful();
        assert!(!fs::read_to_string(&auth).unwrap().contains("mine"));
        fs::remove_file(&auth).unwrap();
        app.artisan(["lang:publish", "--existing"])
            .await
            .assert_successful();
        assert!(!auth.exists());
    }
}

#[tokio::test]
async fn publish_uses_default_and_legacy_roots_without_overwriting_custom_files() {
    for relative in ["lang", "resources/lang"] {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join(relative);
        fs::create_dir_all(destination.join("en")).unwrap();
        let auth = destination.join("en/auth.json");
        fs::write(&auth, r#"{"failed":"mine"}"#).unwrap();
        for arguments in [
            vec!["lang:publish"],
            vec!["lang:publish"],
            vec!["lang:publish", "--force"],
        ] {
            let app = Application::configure(root.path().to_path_buf())
                .create()
                .unwrap();
            let status = app
                .handle_command(
                    &arguments
                        .iter()
                        .map(std::ffi::OsString::from)
                        .collect::<Vec<_>>(),
                )
                .await
                .unwrap();
            assert_eq!(status, 0);
            assert_eq!(
                fs::read_to_string(&auth).unwrap().contains("mine"),
                !arguments.contains(&"--force")
            );
            assert!(destination.join("en/validation.json").is_file());
        }
        if relative == "resources/lang" {
            assert!(!root.path().join("lang").exists());
        }
    }
}

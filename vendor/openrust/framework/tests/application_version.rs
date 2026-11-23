use std::collections::BTreeMap;

use openrust::{
    console::Output,
    contracts::console::Kernel as _,
    foundation::{Application, console::Kernel},
    support::facades::{App, Config},
};

#[test]
fn version_is_available_before_boot_and_matches_the_framework_package() {
    let root = tempfile::tempdir().unwrap();
    let application = Application::configure(root.path().into()).create().unwrap();
    assert_eq!(Application::VERSION, env!("CARGO_PKG_VERSION"));
    assert_eq!(application.version(), Application::VERSION);
    assert!(App::version().is_err());
}

#[tokio::test]
async fn facade_and_artisan_keep_framework_version_when_application_version_changes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".env"), "APP_VERSION=9.8.7-app\n").unwrap();
    let application = Application::configure(root.path().into())
        .with_config(|_, environment| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"version": environment.get_utf8("APP_VERSION")?.unwrap_or("9.0.0")}),
            )]))
        })
        .with_commands(|artisan| {
            artisan.command("version:probe", |output| {
                assert_eq!(Config::get("app.version").unwrap(), Some("9.8.7-app".into()));
                Config::set("app.version", "10.0.0-app".into()).unwrap();
                assert_eq!(App::version().unwrap(), Application::VERSION);
                output.stdout.push_str(App::version().unwrap());
            })?;
            Ok(())
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application, openrust::console::Application::new());
    for (arguments, expected) in [
        (
            vec!["version:probe".into()],
            Application::VERSION.to_owned(),
        ),
        (
            vec!["--version".into()],
            format!("artisan {}\n", Application::VERSION),
        ),
        (
            vec!["-V".into()],
            format!("artisan {}\n", Application::VERSION),
        ),
        (
            vec!["version:probe".into(), "--version".into()],
            format!("artisan-version:probe {}\n", Application::VERSION),
        ),
    ] {
        let mut output = Output::default();
        assert_eq!(kernel.handle(&arguments, &mut output).await, 0);
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
        assert!(App::version().is_err());
    }
}

#[test]
fn repository_comes_from_the_framework_package_without_a_trailing_slash() {
    assert_eq!(Application::REPOSITORY, env!("CARGO_PKG_REPOSITORY"));
    assert!(Application::REPOSITORY.starts_with("https://"));
    assert!(!Application::REPOSITORY.ends_with('/'));
}

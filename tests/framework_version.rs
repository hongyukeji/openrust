use openrust::{contracts::foundation::Application as _, foundation::Application};

#[allow(dead_code)]
#[path = "../config/app.rs"]
mod app_config;
#[allow(dead_code)]
#[path = "../config/env.rs"]
mod env;

#[tokio::test]
async fn application_package_version_and_env_override_do_not_change_framework_version() {
    for override_version in [None, Some("9.8.7-app")] {
        let root = std::env::temp_dir().join(format!(
            "framework-version-{}-{}",
            std::process::id(),
            openrust::support::Str::random(8)
        ));
        std::fs::create_dir_all(&root).unwrap();
        if let Some(version) = override_version {
            std::fs::write(root.join(".env"), format!("APP_VERSION={version}\n")).unwrap();
        }
        let mut application = Application::configure(root.clone())
            .with_config(|_, environment| {
                Ok(std::collections::BTreeMap::from([(
                    "app".into(),
                    app_config::load(environment)?.config,
                )]))
            })
            .create()
            .unwrap();
        application.boot().await.unwrap();
        assert_eq!(
            application.config().unwrap().get("app.version"),
            Some(override_version.unwrap_or(env!("CARGO_PKG_VERSION")).into())
        );
        assert_eq!(application.version(), Application::VERSION);
        drop(application);
        std::fs::remove_dir_all(root).unwrap();
    }
}

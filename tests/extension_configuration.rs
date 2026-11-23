use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _},
    foundation::{Application, console::installation::SourceInstallation, http::Kernel},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "support/config_app.rs"]
mod app;

#[path = "../config/mod.rs"]
mod config;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "openrust-extension-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("storage/framework")).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Merge;
impl ServiceProvider for Merge {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.merge_config_from(app, json!({"from_package":true}), "probe")?;
            self.replace_config_recursively_from(app, json!({"nested":{"new":true}}), "probe")?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn cached_configuration_is_authoritative_for_both_merge_methods() {
    let root = Directory::new();
    fs::write(
        root.0.join("storage/framework/config.json"),
        r#"{"app":{"name":"cached"},"probe":{"nested":{"saved":true}}}"#,
    )
    .unwrap();
    let app = Application::configure(root.0.clone())
        .with_providers(vec![Box::new(Merge)])
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let app = kernel.get_application();
    assert!(app.configuration_is_cached());
    assert_eq!(
        app.config().unwrap().get("probe"),
        Some(json!({"nested":{"saved":true}}))
    );
}

#[test]
fn explicit_configuration_registration_preserves_customizations_and_rejects_ambiguous_loaders() {
    let root = Directory::new();
    fs::write(
        root.0.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.0.0'\n",
    )
    .unwrap();
    fs::create_dir(root.0.join("config")).unwrap();
    fs::write(root.0.join("config/oidc.rs"), "// published configuration").unwrap();
    let original =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("config/mod.rs")).unwrap();
    let path = root.0.join("config/mod.rs");
    fs::write(&path, &original).unwrap();
    let app = Application::configure(root.0.clone()).create().unwrap();
    let installation = SourceInstallation::new(&app).unwrap();
    assert!(installation.registerConfiguration("../escape").is_err());
    assert!(installation.registerConfiguration("missing").is_err());
    installation.registerConfiguration("oidc").unwrap();
    let configured = fs::read_to_string(&path).unwrap();
    assert!(configured.contains("mod oidc;"));
    assert!(configured.contains("oidc::load(env)?"));
    installation.registerConfiguration("oidc").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), configured);
    let custom = configured.replace("oidc::load(env)?", "oidc::custom(env)?");
    fs::write(&path, &custom).unwrap();
    installation.registerConfiguration("oidc").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), custom);
    fs::write(
        root.0.join("config/reverb.rs"),
        "// published configuration",
    )
    .unwrap();
    fs::write(&path, "// custom loader without standard anchors\n").unwrap();
    assert!(installation.registerConfiguration("reverb").is_err());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "// custom loader without standard anchors\n"
    );
    assert!(installation.registerConfiguration("../outside").is_err());
}

fn package_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![
        #[cfg(feature = "settings")]
        Box::new(openrust_settings::SettingsServiceProvider),
        #[cfg(feature = "oidc")]
        Box::new(openrust_oidc::OidcServiceProvider),
        #[cfg(feature = "reverb")]
        Box::new(openrust_reverb::ReverbServiceProvider),
        #[cfg(feature = "pay")]
        Box::new(openrust_pay::PayServiceProvider),
    ]
}

#[tokio::test]
async fn enabled_packages_supply_defaults_without_publishing_application_files() {
    let root = Directory::new();
    fs::write(root.0.join(".env"), "REVERB_ENABLED=false\nREVERB_SERVER_PORT=9123\nOIDC_ISSUER=https://example.test\nPAY_DRIVER=alipay\n").unwrap();
    let providers = package_providers();
    let app = Application::configure(root.0.clone())
        .with_config(config::load)
        .with_providers(providers)
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let app = kernel.get_application();
    assert!(!app.configuration_is_cached());
    let config = app.config().unwrap();
    for (name, enabled) in [
        ("settings", cfg!(feature = "settings")),
        ("oidc", cfg!(feature = "oidc")),
        ("reverb", cfg!(feature = "reverb")),
        ("pay", cfg!(feature = "pay")),
    ] {
        assert_eq!(config.get(name).is_some(), enabled, "{name}");
        assert!(!root.0.join(format!("config/{name}.rs")).exists());
    }
    #[cfg(feature = "settings")]
    assert_eq!(
        config.get("settings.repositories.database.table"),
        Some(json!("settings"))
    );
    #[cfg(feature = "oidc")]
    assert_eq!(
        config.get("oidc.providers"),
        Some(json!({})),
        "enabling OIDC must not create a business provider"
    );
    #[cfg(feature = "reverb")]
    assert_eq!(config.get("reverb.servers.reverb.port"), Some(json!(9123)));
    #[cfg(feature = "pay")]
    assert_eq!(config.get("pay.default"), Some(json!("alipay")));
}

#[tokio::test]
async fn package_defaults_preserve_application_overrides_and_do_not_fill_cached_groups() {
    fn overrides(
        base: &Path,
        env: &openrust::config::Environment,
    ) -> Result<
        std::collections::BTreeMap<String, openrust::config::Value>,
        openrust::config::ConfigError,
    > {
        let mut values = config::load(base, env)?;
        values.insert("pay".into(), json!({"default":"custom","drivers":{}}));
        values.insert(
            "settings".into(),
            json!({"repositories":{"database":{"table":"custom_settings","connection":null}}}),
        );
        values.insert(
            "oidc".into(),
            json!({"providers":{"custom":{"issuer":"https://custom.test"}}}),
        );
        values.insert("reverb".into(), json!({"enabled":false,"mode":"custom"}));
        Ok(values)
    }
    let root = Directory::new();
    let app = Application::configure(root.0.clone())
        .with_config(overrides)
        .with_providers(package_providers())
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let config = kernel.get_application().config().unwrap();
    assert_eq!(
        config.get("settings.repositories.database.table"),
        Some(json!("custom_settings"))
    );
    assert_eq!(
        config.get("oidc.providers"),
        Some(json!({"custom":{"issuer":"https://custom.test"}}))
    );
    assert_eq!(config.get("reverb.mode"), Some(json!("custom")));
    fs::write(
        root.0.join("storage/framework/config.json"),
        r#"{"app":{"name":"cache"},"reverb":{"enabled":false}}"#,
    )
    .unwrap();
    let app = Application::configure(root.0.clone())
        .with_config(overrides)
        .with_providers(package_providers())
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let app = kernel.get_application();
    assert!(app.configuration_is_cached());
    let config = app.config().unwrap();
    assert_eq!(config.get("pay"), None);
    assert_eq!(config.get("settings"), None);
    assert_eq!(config.get("oidc"), None);
    assert_eq!(config.get("reverb"), Some(json!({"enabled":false})));
}

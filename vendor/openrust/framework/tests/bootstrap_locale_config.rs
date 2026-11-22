use std::collections::BTreeMap;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Config},
    translation::Translator,
};

struct Settings;
impl ServiceProvider for Settings {
    fn register<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Config::set(
                "app",
                serde_json::json!({"key":"k".repeat(32), "locale":"en", "fallback_locale":"en"}),
            )?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.setLocale("zh-Hans").await?;
            app.setFallbackLocale("zh-Hant")?;
            Config::set("app.provider_boot", true.into())?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn bootstrap_configuration_survives_register_boot_and_booted_callbacks() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .withConfig(|_, _| Ok(BTreeMap::new()))
        .withProviders(vec![Box::new(Settings)])
        .booted(|app| {
            assert!(app.is_booted());
            assert_eq!(Config::string("app.locale")?, "zh-Hans");
            app.setFallbackLocale("en")?;
            Config::set("app.booted", true.into())?;
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    assert_eq!(config.get("app.key").unwrap(), "k".repeat(32));
    assert_eq!(config.get("app.provider_boot").unwrap(), true);
    assert_eq!(config.get("app.booted").unwrap(), true);
    assert_eq!(app.getLocale().unwrap(), "zh-Hans");
    assert_eq!(app.getFallbackLocale().unwrap(), "en");
    let translator = app.container().get::<Translator>("translator").unwrap();
    assert_eq!(translator.getLocale(), "zh-Hans");
    assert_eq!(translator.getFallback(), "en");

    // 启动结束后，显式任务作用域仍隔离父对象与译器语言。
    app.container()
        .scope(async {
            config
                .set(
                    "app",
                    serde_json::json!({"locale":"en", "fallback_locale":"zh-Hant"}),
                )
                .unwrap();
            app.setLocale("en").await.unwrap();
            app.setFallbackLocale("zh-Hant").unwrap();
            assert!(config.get("app.key").is_none());
            assert_eq!(translator.getLocale(), "en");
            assert_eq!(translator.getFallback(), "zh-Hant");
        })
        .await;
    assert_eq!(config.get("app.key").unwrap(), "k".repeat(32));
    assert_eq!(app.getLocale().unwrap(), "zh-Hans");
    assert_eq!(translator.getLocale(), "zh-Hans");
    assert_eq!(translator.getFallback(), "en");
}

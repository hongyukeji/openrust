//! 翻译 Provider 从同一应用配置快照读取默认和回退语言。

use super::*;
use crate::foundation::BootContext;
use crate::foundation::ProviderBuilder;
use crate::support::{BoxFuture, ServiceProvider};
use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use serde_json::json;

#[test]
fn module_namespace_published_messages_override_source() -> Result<()> {
    let temporary = tempfile::tempdir().map_err(Error::internal)?;
    let source = temporary.path().join("source");
    let published = temporary.path().join("published");
    std::fs::create_dir(&source).map_err(Error::internal)?;
    std::fs::create_dir(&published).map_err(Error::internal)?;
    std::fs::write(
        source.join("en.json"),
        r#"{"label":"source","fallback":"source-only"}"#,
    )
    .map_err(Error::internal)?;
    std::fs::write(source.join("zh-CN.json"), r#"{"only_zh":"仅源目录"}"#)
        .map_err(Error::internal)?;
    std::fs::write(published.join("en.json"), r#"{"label":"published"}"#)
        .map_err(Error::internal)?;
    let i18n = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    i18n.add_namespace("blog", &source, &published)?;
    let en = Locale::new("en")?;
    let args = MessageArgs::new();
    assert_eq!(i18n.translate(&en, "blog::label", &args), "published");
    assert_eq!(
        i18n.translate(&en, "blog::fallback", &args),
        "blog::fallback"
    );
    assert_eq!(i18n.translate(&en, "label", &args), "published");
    assert_eq!(
        i18n.translate(&Locale::new("zh-CN")?, "blog::only_zh", &args),
        "blog::only_zh"
    );
    assert_eq!(i18n.translate(&en, "other::label", &args), "other::label");
    assert_eq!(
        i18n.add_namespace("blog", &source, &published)
            .err()
            .map(|error| error.code().to_owned())
            .as_deref(),
        Some("i18n_namespace_conflict")
    );
    let published_only = temporary.path().join("published-only");
    std::fs::create_dir(&published_only).map_err(Error::internal)?;
    std::fs::write(
        published_only.join("en.json"),
        r#"{"global":"published-only"}"#,
    )
    .map_err(Error::internal)?;
    i18n.add_namespace(
        "published",
        temporary.path().join("missing"),
        &published_only,
    )?;
    assert_eq!(i18n.translate(&en, "global", &args), "published-only");
    let empty_published = temporary.path().join("empty-published");
    std::fs::create_dir(&empty_published).map_err(Error::internal)?;
    i18n.add_namespace("empty", &source, &empty_published)?;
    assert_eq!(i18n.translate(&en, "empty::label", &args), "empty::label");
    Ok(())
}

struct ChangeLocale;
impl ServiceProvider for ChangeLocale {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.overlay("app", json!({"locale": "zh-CN"})) })
    }
}

#[tokio::test]
async fn provider_factory_reads_current_overlay() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(ChangeLocale)
        .provider(TranslationServiceProvider::default());
    let runtime = builder.start(Figment::new()).await?;
    let service = runtime.container().resolve::<Translator>().await?;
    assert_eq!(service.locale_for_request(None).as_str(), "zh-CN");
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn provider_rejects_unsupported_configured_locale() {
    let mut builder = ProviderBuilder::new();
    builder.provider(TranslationServiceProvider::default());
    let sources = Figment::from(Serialized::defaults(
        json!({"app": {"fallback_locale": "fr"}}),
    ));
    assert!(matches!(
        builder.start(sources).await,
        Err(Error::ServiceInitialization { source, .. })
            if source.code() == "configuration_error"
                && source.to_string().contains("app.fallback_locale")
    ));
}

#[tokio::test]
async fn file_provider_uses_application_fallback_for_groups_without_falling_back_json() -> Result<()>
{
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let fallback = directory.path().join("zh-CN");
    std::fs::create_dir(&fallback).map_err(Error::internal)?;
    std::fs::write(fallback.join("messages.json"), r#"{"welcome":"欢迎"}"#)
        .map_err(Error::internal)?;
    std::fs::write(directory.path().join("zh-CN.json"), r#"{"Welcome":"欢迎"}"#)
        .map_err(Error::internal)?;
    let path = directory.path().to_path_buf();
    let mut builder = ProviderBuilder::new();
    builder.provider(TranslationServiceProvider::from_loader_factory(move |_| {
        let loader: Arc<dyn TranslationLoader> = Arc::new(FileTranslationLoader::new(&path));
        Box::pin(async move { Ok(loader) })
    }));
    let sources = Figment::from(Serialized::defaults(
        json!({"app": {"locale": "en", "fallback_locale": "zh-cn"}}),
    ));
    let runtime = builder.start(sources).await?;
    let service = runtime.container().resolve::<Translator>().await?;
    let locale = service.select(None);
    let args = MessageArgs::new();
    assert_eq!(locale.as_str(), "en");
    assert_eq!(
        service.translate(&locale, "messages.welcome", &args),
        "欢迎"
    );
    assert_eq!(service.translate(&locale, "Welcome", &args), "Welcome");
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}
#[test]
fn locales_deserialize_from_application_toml_section() -> Result<()> {
    let figment = Figment::new().merge(Toml::string(
        "[app]\nlocale = \"zh-CN\"\nfallback_locale = \"en\"",
    ));
    let config: AppConfig = figment.extract_inner("app").map_err(ConfigError::from)?;
    assert_eq!(config.locale.as_str(), "zh-CN");
    assert_eq!(config.fallback_locale.as_str(), "en");
    Ok(())
}

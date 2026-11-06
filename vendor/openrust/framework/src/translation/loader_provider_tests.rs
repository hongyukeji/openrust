//! Loader工厂经唯一Provider身份、配置仓储和单例生命周期装配。
use super::*;
use crate::foundation::BootContext;
use crate::foundation::ProviderBuilder;
use crate::foundation::ProviderId;
use crate::support::{BoxFuture, DefaultProviders, ServiceProvider};
use std::sync::atomic::{AtomicUsize, Ordering};

struct FixedLoader {
    snapshots: Arc<AtomicUsize>,
    fail: bool,
}
impl TranslationLoader for FixedLoader {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(Error::custom(
                    "private_loader_error",
                    "不应作为公开快照失败文本",
                ));
            }
            translation_session(BTreeMap::from([(
                TranslationResource {
                    locale: Locale::new("zh-CN")?,
                    group: "*".into(),
                    namespace: None,
                },
                BTreeMap::from([("factory.message".into(), serde_json::json!("工厂来源"))]),
            )]))
        })
    }
}
struct LateLocale;
impl ServiceProvider for LateLocale {
    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(
            async move { context.overlay("i18n", serde_json::json!({"default_locale":"zh-CN"})) },
        )
    }
}
#[tokio::test]
async fn loader_factory_preserves_provider_identity_reads_late_config_and_constructs_once()
-> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let snapshots = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let loaded = snapshots.clone();
    let provider = TranslationServiceProvider::from_loader_factory(move |resolver| {
        let seen = seen.clone();
        let loaded = loaded.clone();
        Box::pin(async move {
            seen.fetch_add(1, Ordering::SeqCst);
            let config = resolver.section::<TranslationConfig>("i18n")?;
            assert_eq!(config.default_locale.as_str(), "zh-CN");
            assert_eq!(config.fallback_locale.as_str(), "en");
            Ok(Arc::new(FixedLoader {
                snapshots: loaded,
                fail: false,
            }) as Arc<dyn TranslationLoader>)
        })
    });
    assert_eq!(provider.id(), TranslationServiceProvider::default().id());
    let defaults = DefaultProviders::empty()
        .merge([
            Box::new(LateLocale) as Box<dyn ServiceProvider>,
            Box::new(TranslationServiceProvider::default()),
        ])
        .replace(
            ProviderId::of::<TranslationServiceProvider>(),
            Box::new(provider),
        );
    let mut builder = ProviderBuilder::new();
    builder.default_providers(defaults);
    let runtime = builder.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        let first = runtime.container().resolve::<Translator>().await?;
        let second = runtime.container().resolve::<Translator>().await?;
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.select(None).as_str(), "zh-CN");
        assert_eq!(
            first.translate(
                &Locale::new("zh-CN")?,
                "factory.message",
                &MessageArgs::new()
            ),
            "工厂来源"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshots.load(Ordering::SeqCst), 1);
        Ok(())
    }
    .await;
    let cleanup = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await;
    outcome?;
    cleanup
}
#[tokio::test]
async fn factory_and_initial_capture_errors_fail_startup_without_static_catalog_fallback() {
    let mut builder = ProviderBuilder::new();
    builder.provider(TranslationServiceProvider::from_loader_factory(|_| {
        Box::pin(async { Err(Error::custom("i18n_fixture_factory_failed", "工厂失败")) })
    }));
    assert!(
        matches!(builder.start(figment::Figment::new()).await,Err(Error::ServiceInitialization {source,..}) if source.code()=="i18n_fixture_factory_failed")
    );
    let snapshots = Arc::new(AtomicUsize::new(0));
    let loaded = snapshots.clone();
    let mut builder = ProviderBuilder::new();
    builder.provider(TranslationServiceProvider::from_loader_factory(move |_| {
        let loaded = loaded.clone();
        Box::pin(async move {
            Ok(Arc::new(FixedLoader {
                snapshots: loaded,
                fail: true,
            }) as Arc<dyn TranslationLoader>)
        })
    }));
    assert!(
        matches!(builder.start(figment::Figment::new()).await,Err(Error::ServiceInitialization {source,..}) if source.code()=="i18n_source_refresh_failed")
    );
    assert_eq!(snapshots.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn normal_catalog_provider_preserves_static_constructor_and_default_lookup() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert("en", "static.message", "application catalog")?;
    let mut builder = ProviderBuilder::new();
    builder.provider(TranslationServiceProvider::new(catalog));
    let runtime = builder.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        let service = runtime.container().resolve::<Translator>().await?;
        assert!(!service.refresh_status()?.configured);
        assert_eq!(
            service.translate(&Locale::new("en")?, "static.message", &MessageArgs::new()),
            "application catalog"
        );
        assert_eq!(
            service.translate(&Locale::new("fr")?, "static.message", &MessageArgs::new()),
            "static.message"
        );
        Ok(())
    }
    .await;
    let cleanup = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn later_boot_overlay_changes_repository_but_does_not_rebuild_existing_i18n_singleton()
-> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let snapshots = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let loaded = snapshots.clone();
    let provider = TranslationServiceProvider::from_loader_factory(move |resolver| {
        let seen = seen.clone();
        let loaded = loaded.clone();
        Box::pin(async move {
            seen.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                resolver
                    .section::<TranslationConfig>("i18n")?
                    .default_locale
                    .as_str(),
                "en"
            );
            Ok(Arc::new(FixedLoader {
                snapshots: loaded,
                fail: false,
            }) as Arc<dyn TranslationLoader>)
        })
    });
    let mut builder = ProviderBuilder::new();
    builder.provider(provider).provider(LateLocale);
    let runtime = builder.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        assert_eq!(
            runtime
                .config()
                .section::<TranslationConfig>("i18n")?
                .default_locale
                .as_str(),
            "zh-CN"
        );
        let first = runtime.container().resolve::<Translator>().await?;
        let second = runtime.container().resolve::<Translator>().await?;
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.select(None).as_str(), "en");
        assert_eq!(second.locale_for_request(None).as_str(), "en");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshots.load(Ordering::SeqCst), 1);
        Ok(())
    }
    .await;
    let cleanup = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await;
    outcome?;
    cleanup
}

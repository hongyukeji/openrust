//! 应用选择翻译来源；文件默认不要求数据库、共享缓存或后台任务。
use openrust::foundation::{ProviderId, RegisterContext};
use openrust::support::{BoxFuture, ServiceProvider};
#[cfg(feature = "translation-database")]
use openrust::translation::CompositeTranslationLoader;
use openrust::translation::{FileLoader, TranslationLoader, TranslationServiceProvider};
use openrust::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceMode {
    #[default]
    Files,
    Database,
    Composite,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SourceSettings {
    pub mode: SourceMode,
    pub install_database: bool,
    pub file_root: String,
    pub paths: Vec<String>,
    pub json_paths: Vec<String>,
    pub namespaces: BTreeMap<String, String>,
}
impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            mode: SourceMode::Files,
            install_database: false,
            file_root: "lang".into(),
            paths: Vec::new(),
            json_paths: Vec::new(),
            namespaces: BTreeMap::new(),
        }
    }
}
impl SourceSettings {
    fn uses_database(&self) -> bool {
        self.install_database || self.mode != SourceMode::Files
    }
    fn validate(&self) -> Result<()> {
        if self.mode != SourceMode::Database
            && std::iter::once(&self.file_root)
                .chain(&self.paths)
                .chain(&self.json_paths)
                .chain(self.namespaces.values())
                .any(|path| path.trim().is_empty())
        {
            return Err(Error::custom(
                "translation_source_config_invalid",
                "文件来源需要非空目录",
            ));
        }
        Ok(())
    }
    fn files(
        &self,
        resolver: &openrust::container::Resolver,
    ) -> Result<Arc<dyn TranslationLoader>> {
        let root = match resolver.section::<Option<PathBuf>>("application_root") {
            Ok(root) => root.unwrap_or_else(|| PathBuf::from(".")),
            Err(openrust::config::ConfigError::Source(error)) if error.missing() => {
                PathBuf::from(".")
            }
            Err(error) => return Err(error.into()),
        };
        let mut loader = if self.file_root == "lang" {
            FileLoader::for_application(&root)
        } else {
            FileLoader::new(root.join(&self.file_root))
        };
        for path in &self.paths {
            loader = loader.with_path(root.join(path));
        }
        for path in &self.json_paths {
            loader = loader.with_json_path(root.join(path));
        }
        for (namespace, path) in &self.namespaces {
            loader = loader.with_namespace(namespace, root.join(path));
        }
        Ok(Arc::new(loader))
    }
}
/// App register阶段贡献配置及显式扩展；文件模式不登记/解析数据库。
pub(crate) struct TranslationSourcesProvider;
impl ServiceProvider for TranslationSourcesProvider {
    fn nested_providers(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<TranslationServiceProvider>()]
    }
    fn possible_nested_providers(&self) -> Vec<ProviderId> {
        #[cfg(feature = "translation-database")]
        {
            vec![ProviderId::of::<
                openrust_translation_database::DatabaseTranslationServiceProvider,
            >()]
        }
        #[cfg(not(feature = "translation-database"))]
        {
            Vec::new()
        }
    }
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use openrust::config::repository::SectionSchema as S;
            app.config_section(
                "translation_sources",
                SourceSettings::default(),
                S::object([
                    ("mode", S::String),
                    ("install_database", S::Boolean),
                    ("file_root", S::String),
                    ("paths", S::array(S::String)),
                    ("json_paths", S::array(S::String)),
                    ("namespaces", S::map(S::String)),
                ]),
                SourceSettings::validate,
            )?;
            let settings = app
                .config_repository()
                .section::<SourceSettings>("translation_sources")?;
            settings.validate()?;
            if settings.uses_database() {
                #[cfg(feature = "translation-database")]
                app.register_provider(
                    openrust_translation_database::DatabaseTranslationServiceProvider,
                )
                .await?;
                #[cfg(not(feature = "translation-database"))]
                return Err(Error::custom(
                    "translation_database_feature_required",
                    "数据库来源需显式编译translation-database",
                ));
            }
            app.register_provider(configured_translator()).await?;
            Ok(())
        })
    }
}
/// 默认槽安装器在公开绑定前登记最终工厂；首次解析读取同一配置视图。
pub(crate) fn configured_translator() -> TranslationServiceProvider {
    TranslationServiceProvider::from_loader_factory(|resolver| {
        Box::pin(async move {
            let settings = resolver.section::<SourceSettings>("translation_sources")?;
            settings.validate()?;
            if settings.mode == SourceMode::Files {
                return settings.files(&resolver);
            }
            #[cfg(feature = "translation-database")]
            {
                let database: Arc<dyn TranslationLoader> = resolver
                    .resolve::<openrust_translation_database::DatabaseTranslationLoader>()
                    .await?;
                if settings.mode == SourceMode::Database {
                    return Ok(database);
                }
                Ok(Arc::new(CompositeTranslationLoader::new(vec![
                    settings.files(&resolver)?,
                    database,
                ])) as Arc<dyn TranslationLoader>)
            }
            #[cfg(not(feature = "translation-database"))]
            {
                Err(Error::custom(
                    "translation_database_feature_required",
                    "数据库来源需显式编译translation-database",
                ))
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrust::foundation::ProviderBuilder;
    use openrust::translation::{Locale, MessageArgs, TranslationLookup, Translator};
    #[tokio::test]
    async fn file_default_installs_no_database_and_preserves_empty_translation_in_unique_service()
    -> Result<()> {
        let files = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::write(
            files.path().join("en.json"),
            r#"{"label":"","zero":"0","only_en":"English"}"#,
        )
        .map_err(Error::internal)?;
        let mut builder = ProviderBuilder::new();
        builder.provider(TranslationSourcesProvider);
        let runtime = builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(
                    serde_json::json!({"translation_sources":{"file_root":files.path()}}),
                ),
            ))
            .await?;
        let outcome: Result<()> = async {
            assert!(!runtime.container().contains::<openrust::database::Db>());
            let first = runtime.container().resolve::<Translator>().await?;
            let second = runtime.container().resolve::<Translator>().await?;
            assert!(Arc::ptr_eq(&first, &second));
            // 普通共享服务保持官方JSON falsey与请求locale规则。
            assert_eq!(
                first.translate(&Locale::new("en")?, "label", &MessageArgs::new()),
                "label"
            );
            assert_eq!(
                first.translate(&Locale::new("en")?, "zero", &MessageArgs::new()),
                "zero"
            );
            assert_eq!(
                first.translate(&Locale::new("zh-CN")?, "only_en", &MessageArgs::new()),
                "only_en"
            );
            // 明确增强视图共享Source目录，保原空值能力，不改变全局singleton。
            let catalog_view = first
                .as_ref()
                .clone()
                .with_lookup(TranslationLookup::Catalog);
            assert_eq!(
                catalog_view.translate(&Locale::new("en")?, "label", &MessageArgs::new()),
                ""
            );
            assert_eq!(
                catalog_view.translate(&Locale::new("en")?, "zero", &MessageArgs::new()),
                "0"
            );
            assert_eq!(
                catalog_view.translate(&Locale::new("zh-CN")?, "only_en", &MessageArgs::new()),
                "English"
            );
            assert_eq!(
                second.translate(&Locale::new("en")?, "label", &MessageArgs::new()),
                "label"
            );
            Ok(())
        }
        .await;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        outcome
    }
    #[tokio::test]
    async fn explicit_missing_file_source_is_not_silently_replaced_by_static_catalog() -> Result<()>
    {
        let files = tempfile::tempdir().map_err(Error::internal)?;
        let mut builder = ProviderBuilder::new();
        builder.provider(TranslationSourcesProvider);
        let outcome = builder.start(figment::Figment::from(figment::providers::Serialized::defaults(
            serde_json::json!({"translation_sources":{"file_root":files.path().join("missing")}})
        ))).await;
        assert!(
            matches!(outcome, Err(Error::ServiceInitialization { source, .. }) if source.code() == "i18n_source_refresh_failed")
        );
        Ok(())
    }
}

#[cfg(test)]
mod early_package_tests {
    use super::*;
    use openrust::foundation::ProviderBuilder;
    use openrust::translation::Translator;
    struct EarlyReader;
    struct Early(Arc<Translator>);
    impl ServiceProvider for EarlyReader {
        fn register<'a>(
            &'a self,
            context: &'a mut RegisterContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let translator = context.resolve::<Translator>().await?;
                assert_eq!(
                    translator.translate(
                        &openrust::translation::Locale::new("en")?,
                        "source_unique",
                        &openrust::translation::MessageArgs::new()
                    ),
                    "file-owned"
                );
                context.instance(Arc::new(Early(translator))).await
            })
        }
    }
    #[tokio::test]
    async fn package_register_early_resolution_and_boot_share_one_source_translator() -> Result<()>
    {
        let files = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::write(
            files.path().join("en.json"),
            r#"{"source_unique":"file-owned"}"#,
        )
        .map_err(Error::internal)?;
        let defaults = openrust::support::DefaultProviders::empty()
            .merge(vec![
                Box::new(TranslationServiceProvider::default()) as Box<dyn ServiceProvider>
            ])
            .except([ProviderId::of::<TranslationServiceProvider>()])
            .merge([Box::new(TranslationSourcesProvider) as Box<dyn ServiceProvider>]);
        let mut builder = ProviderBuilder::new();
        builder.default_providers(defaults).provider(EarlyReader);
        let runtime = builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(
                    serde_json::json!({"translation_sources":{"file_root":files.path()}}),
                ),
            ))
            .await?;
        let outcome: Result<()> = async {
            let early = runtime.container().resolve::<Early>().await?;
            let final_service = runtime.container().resolve::<Translator>().await?;
            assert!(Arc::ptr_eq(&early.0, &final_service));
            assert_eq!(
                final_service.translate(
                    &openrust::translation::Locale::new("en")?,
                    "source_unique",
                    &openrust::translation::MessageArgs::new()
                ),
                "file-owned"
            );
            Ok(())
        }
        .await;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        outcome
    }
}

#[cfg(all(test, feature = "translation-database"))]
mod database_selection_tests {
    use super::*;
    use openrust::foundation::ProviderBuilder;
    struct Replacement;
    impl ServiceProvider for Replacement {}
    #[tokio::test]
    async fn source_does_not_reintroduce_removed_or_replaced_database_provider() -> Result<()> {
        for replaced in [false, true] {
            let defaults = openrust::support::DefaultProviders::empty()
                .merge([Box::new(openrust::database::DatabaseServiceProvider)
                    as Box<dyn ServiceProvider>]);
            let defaults = if replaced {
                defaults.replace(
                    ProviderId::of::<openrust::database::DatabaseServiceProvider>(),
                    Box::new(Replacement),
                )
            } else {
                defaults.except([ProviderId::of::<openrust::database::DatabaseServiceProvider>()])
            }
            .merge([Box::new(TranslationSourcesProvider) as Box<dyn ServiceProvider>]);
            let mut builder = ProviderBuilder::new();
            builder.default_providers(defaults);
            let outcome = builder
                .start(figment::Figment::from(
                    figment::providers::Serialized::defaults(
                        serde_json::json!({"translation_sources":{"mode":"database"}}),
                    ),
                ))
                .await;
            // 条件嵌套登记检查已就绪依赖；静态启动清单缺失使用另一诊断代码。
            assert!(
                matches!(&outcome, Err(error)
                    if error.code() == "provider_order_invalid"
                    && error.to_string().contains(std::any::type_name::<
                        openrust_translation_database::DatabaseTranslationServiceProvider,
                    >())
                    && error.to_string().contains(std::any::type_name::<
                        openrust::database::DatabaseServiceProvider,
                    >())),
                "数据库被替换={replaced}；实际启动错误={:?}",
                outcome.as_ref().err()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod full_default_chain_tests {
    use super::*;
    use openrust::foundation::ProviderBuilder;
    use openrust::translation::Translator;
    struct EarlyReader;
    struct Early(Arc<Translator>);
    impl ServiceProvider for EarlyReader {
        fn register<'a>(
            &'a self,
            context: &'a mut RegisterContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let translator = context.resolve::<Translator>().await?;
                context.instance(Arc::new(Early(translator))).await
            })
        }
    }
    #[tokio::test]
    async fn complete_no_database_default_chain_then_package_reads_final_file_source() -> Result<()>
    {
        let files = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::write(
            files.path().join("en.json"),
            r#"{"source_unique":"file-owned"}"#,
        )
        .map_err(Error::internal)?;
        let defaults = openrust::support::DefaultProviders::auto()
            .except([ProviderId::of::<TranslationServiceProvider>()])
            .merge([Box::new(TranslationSourcesProvider) as Box<dyn ServiceProvider>]);
        let mut builder = ProviderBuilder::new();
        builder.default_providers(defaults).provider(EarlyReader);
        let source = figment::Figment::from(figment::providers::Serialized::defaults(openrust::config::FrameworkConfig::default()))
            .merge(figment::providers::Serialized::defaults(serde_json::json!({
                "database":{"url":null},
                "cache":{"default":"memory","stores":{"memory":{"driver":"memory"}}},
                "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
                "translation_sources":{"file_root":files.path()}
            })));
        let runtime = builder.start(source).await?;
        let outcome: Result<()> = async {
            let early = runtime.container().resolve::<Early>().await?;
            let final_service = runtime.container().resolve::<Translator>().await?;
            assert!(Arc::ptr_eq(&early.0, &final_service));
            assert_eq!(
                final_service.translate(
                    &openrust::translation::Locale::new("en")?,
                    "source_unique",
                    &openrust::translation::MessageArgs::new()
                ),
                "file-owned"
            );
            Ok(())
        }
        .await;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        outcome
    }
}

//! 可选Provider只贡献具体数据库Loader与标准迁移，不改变核心默认来源。
use crate::{DatabaseTranslationLoader, TranslationSourceMigration};
use openrust::Error;
use openrust::Result;
use openrust::database::DatabaseServiceProvider;
use openrust::database::Databases;
use openrust::database::MigrationServiceProvider;
use openrust::database::sea_orm_migration::MigrationTrait;
use openrust::database::sea_orm_migration::MigratorTrait;
use openrust::foundation::ProviderId;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, ServiceProvider};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

/// 独立来源配置；连接名称来自同一Databases，不接受额外URL或凭据。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TranslationDatabaseConfig {
    /// None使用框架默认连接；Some按配置连接名选择。
    pub connection: Option<String>,
    /// 一次完整快照允许的最大启用消息行数。
    pub max_rows: usize,
    /// 无通知时核对数据库修订的间隔秒数。
    pub revision_check_interval_secs: u64,
}
impl Default for TranslationDatabaseConfig {
    fn default() -> Self {
        Self {
            connection: None,
            max_rows: 10_000,
            revision_check_interval_secs: 30,
        }
    }
}
fn validate(config: &TranslationDatabaseConfig) -> Result<()> {
    if config
        .connection
        .as_ref()
        .is_some_and(|name| name.is_empty() || name.chars().any(char::is_control))
        || config.max_rows == 0
        || config
            .max_rows
            .checked_add(1)
            .and_then(|n| i64::try_from(n).ok())
            .is_none()
        || config.revision_check_interval_secs == 0
    {
        return Err(Error::custom(
            "i18n_database_config_invalid",
            "数据库翻译配置无效",
        ));
    }
    Ok(())
}
/// 独立来源迁移集合，复用应用唯一迁移命令、锁与表名作用域。
pub struct TranslationSourceMigrator;
impl MigratorTrait for TranslationSourceMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(TranslationSourceMigration)]
    }
}
/// 显式登记数据库来源；不启动守护、不替换文件Loader或Translator singleton。
#[derive(Debug, Default, Clone, Copy)]
pub struct DatabaseTranslationServiceProvider;
impl ServiceProvider for DatabaseTranslationServiceProvider {
    fn dependencies(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<DatabaseServiceProvider>()]
    }
    fn nested_providers(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<
            MigrationServiceProvider<TranslationSourceMigrator>,
        >()]
    }
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use openrust::config::repository::SectionSchema as S;
            context.config_section(
                "translation_database",
                TranslationDatabaseConfig::default(),
                S::object([
                    ("connection", S::String.nullable()),
                    ("max_rows", S::Integer),
                    ("revision_check_interval_secs", S::Integer),
                ]),
                validate,
            )?;
            context
                .singleton::<DatabaseTranslationLoader, _>(|resolver| {
                    Box::pin(async move {
                        let config = resolver
                            .section::<TranslationDatabaseConfig>("translation_database")?;
                        validate(&config)?;
                        let databases = resolver.resolve::<Databases>().await?;
                        let db = match config.connection.as_deref() {
                            Some(name) => databases.get(name)?,
                            None => databases.default_database(),
                        };
                        Ok(Arc::new(DatabaseTranslationLoader::new(db).with_limits(
                            config.max_rows,
                            Duration::from_secs(config.revision_check_interval_secs),
                        )?))
                    })
                })
                .await?;
            context
                .register_provider(
                    MigrationServiceProvider::<TranslationSourceMigrator>::new()
                        .for_module("openrust-translation-database"),
                )
                .await?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

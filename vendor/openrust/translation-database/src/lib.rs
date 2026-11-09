//! 显式选择的数据库翻译来源；复用框架唯一Loader与翻译服务。
//!
//! 数据库存储、修订与提交后提示复用统一来源契约。
//! 翻译、语言回退及格式化由框架 Translator 执行。
//! 应用显式注册本Provider后，再选择该具体Loader或核心CompositeTranslationLoader；
//! 仅登记本Provider不会改变文件默认来源，也不会启动后台守护。
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod database;
mod migration;
mod provider;
pub use database::{
    DatabaseTranslationLoader, TranslationMutation, TranslationSourceChanged,
    TranslationWriteOutcome,
};
pub use migration::TranslationSourceMigration;
pub use provider::{
    DatabaseTranslationServiceProvider, TranslationDatabaseConfig, TranslationSourceMigrator,
};

//! 显式选择的数据库翻译来源；复用框架唯一Loader与翻译服务。
//!
//! 对应固定Laravel的Loader公开替换机制，数据库存储、修订与提交后提示是已授权来源增强，
//! 不宣称Laravel内置相同数据库schema。翻译、语言回退及格式化仍由框架Translator执行。
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

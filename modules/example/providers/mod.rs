//! 业务模块装配入口；后继生命周期统一遵循框架 `ServiceProvider` contract。

#[cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
mod attachments;
mod example;
mod module;
pub(crate) mod module_state;
#[cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) use attachments::ArticleAttachmentsProvider;
pub(crate) use example::ExampleServiceProvider;
pub(crate) use module::ExampleModuleServiceProvider;

#[cfg(feature = "session")]
#[allow(dead_code)] // 显式装配示例；默认宿主有意不覆盖调用方既有 store。
pub(crate) mod session;

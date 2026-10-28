//! 业务模块装配入口；后继生命周期统一遵循框架 `ServiceProvider` contract。

#[cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
mod attachments;
mod example;
#[cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) use attachments::ArticleAttachmentsProvider;
pub(crate) use example::ExampleServiceProvider;

//! 示例业务依赖；由模块 Provider 登记，通过框架容器注入控制器。

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod account_security;
#[cfg(all(
    feature = "auth",
    feature = "data-exchange",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod article_transfer;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod article_write;
#[cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod attachments;
mod message;
#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod oidc;
#[cfg(any(
    test,
    all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))
))]
pub(crate) mod telemetry;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod tenant;
#[cfg(feature = "http-client")]
pub(crate) mod upstream;
#[cfg(any(
    test,
    all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))
))]
pub(crate) mod welcome;

pub(crate) use message::{ExampleConfig, ExampleMessage};

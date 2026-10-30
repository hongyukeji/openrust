//! 业务 HTTP 处理器。

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod account_security;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod article;
#[cfg(all(
    feature = "auth",
    feature = "queue",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod article_cache;
#[cfg(all(
    feature = "auth",
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod article_socket;
#[cfg(all(
    feature = "auth",
    feature = "data-exchange",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod article_transfer;
#[cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod attachments;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod auth;
pub(crate) mod config_reading;
pub(crate) mod cookie;
pub(crate) mod example;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod example_record;
#[cfg(feature = "queue")]
pub(crate) mod jobs;
pub(crate) mod localization;
#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod oidc;
#[cfg(feature = "view")]
pub(crate) mod pages;
#[cfg(all(
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod realtime;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod tenant;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod welcome;

#[cfg(feature = "session")]
pub(crate) mod session;

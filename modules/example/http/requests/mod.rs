//! 业务请求 DTO。

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod account_security;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod article;
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
#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) mod oidc;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod welcome;

//! 业务请求 DTO。

#[cfg(feature = "auth")]
pub(crate) mod account_security;
#[cfg(feature = "auth")]
pub(crate) mod article;
#[cfg(all(feature = "auth", feature = "data-exchange"))]
pub(crate) mod article_transfer;
#[cfg(all(feature = "auth", feature = "storage-local"))]
pub(crate) mod attachments;
#[cfg(feature = "auth")]
pub(crate) mod auth;
#[cfg(feature = "auth")]
pub(crate) mod example_record;
#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
#[cfg(feature = "auth")]
pub(crate) mod welcome;

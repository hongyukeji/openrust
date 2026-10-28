//! 类型化业务响应与网页上下文。

#[cfg(feature = "view")]
pub(crate) mod about;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod article;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod credentials;
pub(crate) mod example;

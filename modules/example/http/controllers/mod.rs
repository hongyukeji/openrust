//! 业务 HTTP 处理器。

#[cfg(feature = "auth")]
pub(crate) mod auth;
pub(crate) mod example;
#[cfg(feature = "queue")]
pub(crate) mod jobs;
#[cfg(feature = "view")]
pub(crate) mod pages;

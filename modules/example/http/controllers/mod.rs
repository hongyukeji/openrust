//! 业务 HTTP 处理器。

#[cfg(feature = "auth")]
pub(crate) mod account_security;
#[cfg(feature = "auth")]
pub(crate) mod article;
#[cfg(all(feature = "auth", feature = "queue"))]
pub(crate) mod article_cache;
#[cfg(all(feature = "auth", feature = "realtime"))]
pub(crate) mod article_socket;
#[cfg(all(feature = "auth", feature = "data-exchange"))]
pub(crate) mod article_transfer;
#[cfg(all(feature = "auth", feature = "storage-local"))]
pub(crate) mod attachments;
#[cfg(feature = "auth")]
pub(crate) mod auth;
pub(crate) mod config_reading;
pub(crate) mod cookie;
pub(crate) mod database_connection;
pub(crate) mod draft_redirect;
pub(crate) mod example;
#[cfg(feature = "auth")]
pub(crate) mod example_record;
#[cfg(feature = "auth-fortify")]
pub(crate) mod fortify;
#[cfg(feature = "auth")]
pub(crate) mod import_timeline;
#[cfg(feature = "queue")]
pub(crate) mod jobs;
pub(crate) mod localization;
pub(crate) mod logging;
#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
#[cfg(feature = "view")]
pub(crate) mod pages;
#[cfg(feature = "realtime")]
pub(crate) mod realtime;
#[cfg(feature = "auth")]
pub(crate) mod tenant;
#[cfg(feature = "auth")]
pub(crate) mod welcome;

#[cfg(feature = "session")]
pub(crate) mod session;

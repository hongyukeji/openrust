//! 示例业务依赖；由模块 Provider 登记，通过框架容器注入控制器。

#[cfg(feature = "auth")]
pub(crate) mod account_security;
#[cfg(all(feature = "auth", feature = "data-exchange"))]
pub(crate) mod article_transfer;
#[cfg(feature = "auth")]
pub(crate) mod article_write;
#[cfg(all(feature = "auth", feature = "storage-local"))]
pub(crate) mod attachments;
pub(crate) mod config_reading;
#[cfg(feature = "auth")]
pub(crate) mod import_timeline;
mod message;
#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
#[cfg(any(test, feature = "auth"))]
pub(crate) mod telemetry;
#[cfg(feature = "auth")]
pub(crate) mod tenant;
#[cfg(feature = "http-client")]
pub(crate) mod upstream;
#[cfg(any(test, feature = "auth"))]
pub(crate) mod welcome;

pub(crate) use message::{ExampleConfig, ExampleMessage};
#[cfg(feature = "session")]
pub(crate) mod draft_errors;

#[cfg(feature = "translation-database")]
pub(crate) mod translation_source;

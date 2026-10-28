//! 普通业务模块，通过应用注册清单接入框架；全部业务资源在本目录内。

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub(crate) mod database;
#[cfg(test)]
mod extensions;
#[cfg(feature = "grpc")]
#[allow(dead_code)]
mod grpc;
mod http;
#[cfg(feature = "queue")]
pub(crate) mod jobs;
pub(crate) mod providers;
mod routes;
mod services;
#[cfg(test)]
#[path = "tests/registration.rs"]
mod tests;

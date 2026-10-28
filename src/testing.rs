//! 使用生产 Provider 与 Router 流程的应用测试工具。

mod app;
mod response;

pub use app::{TestApp, TestAppBuilder};
pub use response::TestResponse;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
mod database;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use database::TestDatabase;

//! 使用生产 Provider 与 Router 流程的应用测试工具。

mod app;
mod response;

pub use app::{TestApp, TestAppBuilder};
pub use response::TestResponse;

#[cfg(any(feature = "db-postgres", feature = "db-mysql", feature = "db-sqlite"))]
mod database;

#[cfg(any(feature = "db-postgres", feature = "db-mysql", feature = "db-sqlite"))]
pub use database::TestDatabase;

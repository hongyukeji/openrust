//! 数据库集成测试工具。

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
mod database;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use database::TestDatabase;

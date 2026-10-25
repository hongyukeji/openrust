#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
mod database;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use database::TestDatabase;

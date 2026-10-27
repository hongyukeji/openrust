#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use openrust::testing::TestDatabase;

//! Disk resolution, storage, and cloud URL contracts.

mod cloud;
mod factory;
mod file_stream;
mod filesystem_contract;

pub use cloud::Cloud;
pub use factory::Factory;
pub use file_stream::FileStream;
pub use filesystem_contract::Filesystem;

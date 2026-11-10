//! Named filesystem disks and file operations.

mod aws_s3_v3_adapter;
mod filesystem_adapter;
mod filesystem_manager;
mod filesystem_service_provider;
mod local_filesystem_adapter;
mod receive_file;
mod serve_file;

use std::{error::Error, future::Future, pin::Pin};

pub use aws_s3_v3_adapter::AwsS3V3Adapter;
pub use filesystem_adapter::FilesystemAdapter;
pub use filesystem_manager::FilesystemManager;
pub use filesystem_service_provider::FilesystemServiceProvider;
pub use local_filesystem_adapter::LocalFilesystemAdapter;
pub use receive_file::ReceiveFile;
pub use serve_file::ServeFile;

/// An asynchronous filesystem operation.
pub type FilesystemFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a filesystem operation.
pub type FilesystemResult<T> = Result<T, FilesystemError>;

/// A disk configuration, path, or storage failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum FilesystemError {
    UnknownDisk {
        name: String,
    },
    InvalidPath,
    NotFound,
    UnsupportedOperation,
    PublishOutcomeUnknown,
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// The requested visibility of an object.
pub enum Visibility {
    Public,
    Private,
}

/// Options applied to a single filesystem write.
pub struct WriteOptions {
    _private: (),
}

/// A signed upload URL and the headers required to use it.
pub struct TemporaryUploadUrl {
    _private: (),
}

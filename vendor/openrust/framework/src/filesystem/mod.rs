//! Named filesystem disks and file operations.

mod aws_s3_v3_adapter;
mod download;
mod facade;
mod filesystem_adapter;
mod filesystem_manager;
mod filesystem_service_provider;
mod local_filesystem_adapter;
mod receive_file;
mod serve_file;

use std::{error::Error, future::Future, pin::Pin};

pub use aws_s3_v3_adapter::{AwsS3V3Adapter, S3DiskConfig};
pub use filesystem_adapter::FilesystemAdapter;
pub use filesystem_manager::{FilesystemManager, LocalDiskConfig};
pub use filesystem_service_provider::FilesystemServiceProvider;
pub use local_filesystem_adapter::LocalFilesystemAdapter;
pub use receive_file::ReceiveFile;
pub use serve_file::ServeFile;

/// An asynchronous filesystem operation.
pub type FilesystemFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A readable object stream from any configured disk.
pub type FilesystemReader = Box<dyn tokio::io::AsyncRead + Send + Unpin>;

/// The result of a filesystem operation.
pub type FilesystemResult<T> = Result<T, FilesystemError>;

/// A disk configuration, path, or storage failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum FilesystemError {
    UnknownDisk {
        name: String,
    },
    InvalidConfiguration,
    Unavailable(crate::contracts::container::BindingResolutionError),
    InvalidPath,
    NotFound,
    UnsupportedOperation,
    PublishOutcomeUnknown,
    Backend {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// The requested visibility of an object.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    #[default]
    Private,
}

/// Options applied to a single filesystem write.
#[derive(Clone, Copy, Debug, Default)]
pub struct WriteOptions {
    pub visibility: Option<Visibility>,
}

/// A signed upload URL and the headers required to use it.
pub struct TemporaryUploadUrl {
    pub url: String,
    pub headers: crate::http::HeaderMap,
}

impl std::fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownDisk { name } => {
                write!(formatter, "filesystem disk [{name}] is not configured")
            }
            Self::InvalidConfiguration => {
                formatter.write_str("invalid filesystem disk configuration")
            }
            Self::Unavailable(_) => formatter.write_str("filesystem manager is unavailable"),
            Self::InvalidPath => formatter.write_str("invalid filesystem path"),
            Self::NotFound => formatter.write_str("filesystem entry not found"),
            Self::UnsupportedOperation => formatter.write_str("unsupported filesystem operation"),
            Self::PublishOutcomeUnknown => {
                formatter.write_str("filesystem publication outcome is unknown")
            }
            Self::Backend { .. } => formatter.write_str("filesystem operation failed"),
        }
    }
}

impl Error for FilesystemError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(source) => Some(source),
            Self::Backend { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FilesystemError {
    fn from(source: std::io::Error) -> Self {
        match source.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::Unsupported => Self::UnsupportedOperation,
            _ => Self::Backend {
                source: Box::new(source),
            },
        }
    }
}

#[cfg(unix)]
impl Visibility {
    fn file_mode(self) -> u32 {
        match self {
            Self::Private => 0o600,
            Self::Public => 0o644,
        }
    }

    fn directory_mode(self) -> u32 {
        match self {
            Self::Private => 0o700,
            Self::Public => 0o755,
        }
    }
}

pub(crate) use local_filesystem_adapter::{is_staging_name, open_public_file};

pub use facade::Storage;

mod signed_routes;
pub(crate) use signed_routes::register as register_signed_routes;

use std::sync::Arc;

use crate::filesystem::{FilesystemAdapter, FilesystemFuture, FilesystemResult};

/// Resolves the default or a named filesystem disk.
pub trait Factory: Send + Sync {
    fn disk<'a>(
        &'a self,
        name: Option<&'a str>,
    ) -> FilesystemFuture<'a, FilesystemResult<Arc<FilesystemAdapter>>>;
}

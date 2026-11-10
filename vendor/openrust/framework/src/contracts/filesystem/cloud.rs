use crate::filesystem::FilesystemResult;

use super::Filesystem;

/// A filesystem disk that can produce object URLs.
pub trait Cloud: Filesystem {
    fn url(&self, path: &str) -> FilesystemResult<String>;
}

use std::path::Path;

use crate::view::{ViewData, ViewError, ViewResult};

/// Reads a UTF-8 HTML view inside its configured root.
pub struct FileEngine;

impl FileEngine {
    pub fn get(&self, root: &Path, path: &Path) -> ViewResult<String> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ViewError::OutsideRoot)?;
        let directory = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(ViewError::ReadFailed)?;
        directory
            .read_to_string(relative)
            .map_err(ViewError::ReadFailed)
    }
}

impl crate::contracts::view::Engine for FileEngine {
    fn get(&self, root: &Path, path: &Path, _: &ViewData) -> ViewResult<String> {
        self.get(root, path)
    }
}

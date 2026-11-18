use crate::File;
use std::{fs, io, path::PathBuf};

/// Finds active sessions belonging to one application's storage directory.
pub struct Files(PathBuf);

impl Files {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
    pub fn all(&self) -> io::Result<Vec<File>> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "pail")
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths.into_iter().map(File::new).collect())
    }
}

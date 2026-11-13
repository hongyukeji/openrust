use std::{fs::File, path::PathBuf};

use crate::{
    console::ConsoleError, contracts::foundation::Application as _, foundation::Application,
};

use super::error;

pub(super) struct Destination {
    pub path: PathBuf,
    pub append: bool,
}

impl Destination {
    pub fn open(&self, app: &Application) -> Result<File, ConsoleError> {
        let path = app.base_path().join(&self.path);
        File::options()
            .create(true)
            .write(true)
            .append(self.append)
            .truncate(!self.append)
            .open(&path)
            .map_err(|source| {
                error(format!(
                    "Cannot open scheduled output {}: {source}",
                    path.display()
                ))
            })
    }
}

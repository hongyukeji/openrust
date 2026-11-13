use std::{
    io::{self, Write as _},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::{contracts::foundation::Application as _, foundation::Application};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub(super) struct EventCache {
    version: u32,
    decisions: Vec<(String, bool)>,
}

impl EventCache {
    pub(super) fn new(decisions: Vec<(String, bool)>) -> Self {
        Self {
            version: VERSION,
            decisions,
        }
    }

    pub(super) fn decisions(&self, names: impl Iterator<Item = &'static str>) -> Option<Vec<bool>> {
        if self.version != VERSION
            || !self
                .decisions
                .iter()
                .map(|(name, _)| name.as_str())
                .eq(names)
        {
            return None;
        }
        Some(self.decisions.iter().map(|(_, enabled)| *enabled).collect())
    }

    pub(super) fn load(app: &Application) -> io::Result<Option<Self>> {
        match std::fs::read(path(app)) {
            Ok(contents) => Ok(serde_json::from_slice(&contents).ok()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) fn save(&self, app: &Application) -> io::Result<()> {
        let path = path(app);
        let directory = path.parent().expect("event cache path has a parent");
        std::fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut file, self)?;
        file.flush()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

pub(in crate::foundation) fn clear(app: &Application) -> io::Result<()> {
    match std::fs::remove_file(path(app)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn path(app: &Application) -> PathBuf {
    app.base_path().join("storage/framework/cache/events.json")
}

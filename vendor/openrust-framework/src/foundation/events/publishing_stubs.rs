use std::{
    fs, io,
    path::Path,
    sync::{Arc, Mutex},
};

use indexmap::IndexMap;

use crate::contracts::events::Event;

/// Allows listeners to modify the source templates about to be published.
#[derive(Clone, Default)]
pub struct PublishingStubs {
    stubs: Arc<Mutex<IndexMap<String, String>>>,
}

impl PublishingStubs {
    /// Adds a source file to the current publication.
    pub fn add(&self, path: impl AsRef<Path>, name: &str) -> io::Result<()> {
        self.insert(name, fs::read_to_string(path)?)
    }

    /// Adds or replaces a template by its filename under the application stubs directory.
    pub fn insert(&self, name: &str, contents: impl Into<String>) -> io::Result<()> {
        if !name.ends_with(".stub")
            || !name
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expected a stub filename.",
            ));
        }
        self.stubs
            .lock()
            .expect("publishing stubs lock poisoned")
            .insert(name.to_owned(), contents.into());
        Ok(())
    }

    /// Excludes a template from this publication.
    pub fn remove(&self, name: &str) -> Option<String> {
        self.stubs
            .lock()
            .expect("publishing stubs lock poisoned")
            .shift_remove(name)
    }

    pub(crate) fn snapshot(&self) -> IndexMap<String, String> {
        self.stubs
            .lock()
            .expect("publishing stubs lock poisoned")
            .clone()
    }
}

impl Event for PublishingStubs {
    fn name(&self) -> &str {
        "foundation.publishing_stubs"
    }
}

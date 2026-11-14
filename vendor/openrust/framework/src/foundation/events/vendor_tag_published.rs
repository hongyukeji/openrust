use std::path::PathBuf;

use indexmap::IndexMap;

use crate::contracts::events::Event;

/// Dispatched after processing a selection with registered publishable paths.
#[derive(Clone, Debug)]
pub struct VendorTagPublished {
    pub tag: Option<String>,
    pub paths: IndexMap<PathBuf, PathBuf>,
}

impl Event for VendorTagPublished {
    fn name(&self) -> &str {
        "OpenRust\\Foundation\\Events\\VendorTagPublished"
    }
}

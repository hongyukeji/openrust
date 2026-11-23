//! Fixed the framework Passkeys 0.3.0 display resources (MIT); never authentication evidence.
use crate::{PasskeyError, PasskeyResult};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

type Labels = BTreeMap<String, String>;
type Icons = BTreeMap<String, BTreeMap<String, String>>;
static LABELS: RwLock<Option<Arc<Labels>>> = RwLock::new(None);
static ICONS: RwLock<Option<Arc<Icons>>> = RwLock::new(None);

pub struct Aaguids;
#[allow(non_snake_case, reason = "Matches the upstream display resource API")]
impl Aaguids {
    pub fn unknown() -> &'static str {
        "00000000-0000-0000-0000-000000000000"
    }
    pub fn all() -> Arc<Labels> {
        let mut labels = LABELS.write().expect("AAGUID label cache lock");
        labels
            .get_or_insert_with(|| {
                Arc::new(
                    serde_json::from_str(include_str!("../resources/aaguids.json"))
                        .expect("fixed AAGUID label resource"),
                )
            })
            .clone()
    }
    pub fn labelFor(aaguid: &str) -> Option<String> {
        Self::all().get(aaguid).cloned()
    }
    pub fn iconFor(aaguid: &str, theme: &str) -> PasskeyResult<Option<String>> {
        if !matches!(theme, "light" | "dark") {
            return Err(PasskeyError::UnsupportedIconTheme(theme.into()));
        }
        let mut icons = ICONS.write().expect("AAGUID icon cache lock");
        let icons = icons.get_or_insert_with(|| {
            Arc::new(
                serde_json::from_str(include_str!("../resources/aaguid-icons.json"))
                    .expect("fixed AAGUID icon resource"),
            )
        });
        Ok(icons
            .get(aaguid)
            .and_then(|themes| themes.get(theme))
            .cloned())
    }
    pub fn flush() {
        *LABELS.write().expect("AAGUID label cache lock") = None;
        *ICONS.write().expect("AAGUID icon cache lock") = None;
    }
}

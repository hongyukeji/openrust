use std::{collections::BTreeMap, fmt};

use openrust::database::Transaction;
use serde_json::Value;

use crate::SettingsFuture;

/// Raw persisted property. Encrypted properties contain ciphertext, never decoded values.
#[derive(Clone, PartialEq)]
pub struct StoredProperty {
    pub payload: Value,
    pub locked: bool,
}
impl fmt::Debug for StoredProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredProperty")
            .field("locked", &self.locked)
            .finish_non_exhaustive()
    }
}

/// One complete database group read, including an opaque concurrency token.
#[derive(Clone, Default, PartialEq)]
pub struct SettingsSnapshot {
    pub properties: BTreeMap<String, StoredProperty>,
    pub(crate) version: Option<String>,
}
impl SettingsSnapshot {
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}
impl fmt::Debug for SettingsSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsSnapshot")
            .field("properties", &self.properties.keys())
            .finish_non_exhaustive()
    }
}

/// Trusted application operation. Payloads must already be validated and encrypted.
#[derive(Clone)]
pub enum SettingsUpdate {
    Set(Value),
    Reset,
    Lock,
    Unlock,
}
impl fmt::Debug for SettingsUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Set(_) => "Set([redacted])",
            Self::Reset => "Reset",
            Self::Lock => "Lock",
            Self::Unlock => "Unlock",
        })
    }
}

/// Repository operations use complete snapshots for compare-and-save, including empty groups.
pub trait SettingsRepository: Send + Sync {
    fn load<'a>(&'a self, group: &'a str) -> SettingsFuture<'a, SettingsSnapshot>;
    fn save<'a>(
        &'a self,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot>;
    /// The supplied transaction selects the target database. Its caller owns the outer commit.
    /// The returned snapshot is uncommitted and must not be published to other operations.
    fn saveIn<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot>;
}

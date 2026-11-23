use std::{collections::BTreeMap, fmt};

use openrust::database::Transaction;
use serde_json::Value;

use crate::SettingsFuture;

/// Raw persisted property. Encrypted properties contain ciphertext, never decoded values.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SettingsSnapshot {
    pub properties: BTreeMap<String, StoredProperty>,
    pub(crate) version: Option<String>,
}
impl SettingsSnapshot {
    /// 自定义仓储每次提交时提供新的不透明版本；不要用明文配置值充当版本。
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
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
    /// 显式刷新必须绕过本地快照；无缓存仓储默认等同于 load。
    fn refresh<'a>(&'a self, group: &'a str) -> SettingsFuture<'a, SettingsSnapshot> {
        self.load(group)
    }
    /// 自定义仓储默认回退为完整读取；数据库实现只批量查询持久版本。
    fn versions<'a>(
        &'a self,
        groups: &'a [String],
    ) -> SettingsFuture<'a, BTreeMap<String, Option<String>>> {
        Box::pin(async move {
            let mut versions = BTreeMap::new();
            for group in groups {
                versions.insert(group.clone(), self.load(group).await?.version);
            }
            Ok(versions)
        })
    }
    fn save<'a>(
        &'a self,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot>;
    /// The supplied transaction selects the target database. Its caller owns the outer commit.
    /// The returned snapshot is uncommitted and must not be published to other operations.
    fn save_in<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        group: &'a str,
        expected: &'a SettingsSnapshot,
        changes: &'a BTreeMap<String, SettingsUpdate>,
    ) -> SettingsFuture<'a, SettingsSnapshot>;
}

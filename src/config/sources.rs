//! 同一次加载形成的有序原始配置与类型化来源投影。

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};

use figment::{
    Figment, Metadata, Profile, Provider,
    value::{Dict, Map},
};
use serde_json::{Map as JsonMap, Value};

use super::ConfigError;
use super::repository::{normalize_php_source, php_value, reorder_like};

/// 不可变配置来源；原始键序和 Figment 来源元数据来自同一输入。
///
/// 从已有 [`Figment`] 转换时原始键序已经丢失，仅能使用有损入口。
#[derive(Clone)]
pub struct ConfigSources {
    figment: Figment,
    ordered: Option<Value>,
}

impl ConfigSources {
    /// 按根键的迭代顺序建立显式来源；重复键覆盖原位置。
    ///
    /// # Errors
    ///
    /// 来源无法投影为 Figment 配置时返回脱敏的配置错误。
    pub fn from_ordered_entries<K: Into<String>>(
        entries: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<Self, ConfigError> {
        let mut root = JsonMap::new();
        for (key, value) in entries {
            root.insert(key.into(), value);
        }
        let ordered = php_value(
            root.into_iter()
                .map(|(key, value)| (key, normalize_php_source(value, false)))
                .collect(),
            true,
        );
        // Figment 的根必须是 Dict；列表根仅在投影内展开成数字键。
        let projected = project_root(&ordered);
        let figment = Figment::from(figment::providers::Serialized::defaults(projected));
        Self::checked(figment, ordered)
    }

    /// 借用保留 profile 与来源元数据的 Figment 投影。
    pub fn figment(&self) -> &Figment {
        &self.figment
    }

    pub(crate) fn into_parts(self) -> (Figment, Option<Value>) {
        (self.figment, self.ordered)
    }

    pub(crate) fn checked(figment: Figment, ordered: Value) -> Result<Self, ConfigError> {
        let resolved: Value = figment.extract()?;
        if resolved != project_root(&ordered) {
            return Err(ConfigError::Invalid(
                "configuration: 有序原始来源与类型化投影不一致".into(),
            ));
        }
        Ok(Self {
            figment,
            ordered: Some(ordered),
        })
    }

    pub(crate) fn merge<P: Provider>(
        &mut self,
        provider: P,
        layer: Value,
    ) -> Result<(), ConfigError> {
        let candidate = self.figment.clone().merge(provider);
        let resolved: Value = candidate.extract()?;
        let mut order = self
            .ordered
            .clone()
            .unwrap_or_else(|| Value::Object(JsonMap::new()));
        merge_order(&mut order, normalize_php_source(layer, false));
        let ordered = reorder_like(&order, resolved);
        if candidate.extract::<Value>()? != project_root(&ordered) {
            return Err(ConfigError::Invalid(
                "configuration: 来源合并后的值与有序投影不一致".into(),
            ));
        }
        self.figment = candidate;
        self.ordered = Some(ordered);
        Ok(())
    }
}

impl From<Figment> for ConfigSources {
    fn from(figment: Figment) -> Self {
        Self {
            figment,
            ordered: None,
        }
    }
}

pub(crate) fn merge_order(before: &mut Value, layer: Value) {
    match (before, layer) {
        (Value::Object(existing), Value::Object(incoming)) => {
            for (key, value) in incoming {
                if let Some(old) = existing.get_mut(&key) {
                    merge_order(old, value);
                } else {
                    existing.insert(key, value);
                }
            }
        }
        (before, layer) => *before = layer,
    }
}

fn project_root(ordered: &Value) -> Value {
    match ordered {
        Value::Array(items) => Value::Object(
            items
                .iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value.clone()))
                .collect(),
        ),
        value => value.clone(),
    }
}

/// 冻结一次解析后的 provider 数据，并保留原始来源元数据。
#[derive(Clone)]
pub(crate) struct CapturedProvider {
    data: Map<Profile, Dict>,
    metadata: Metadata,
    profile: Option<Profile>,
}

impl CapturedProvider {
    pub(crate) fn new<P: Provider>(provider: &P) -> Result<Self, ConfigError> {
        Ok(Self {
            data: provider.data()?,
            metadata: provider.metadata(),
            profile: provider.profile(),
        })
    }

    pub(crate) fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }
}

impl Provider for CapturedProvider {
    fn metadata(&self) -> Metadata {
        self.metadata.clone()
    }
    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        Ok(self.data.clone())
    }
    fn profile(&self) -> Option<Profile> {
        self.profile.clone()
    }
}

/// 在一次迭代中冻结进程环境；保留非 UTF-8 值供安全的按键查询。
pub(crate) struct EnvironmentSnapshot {
    values: Vec<(OsString, OsString)>,
}

impl EnvironmentSnapshot {
    pub(crate) fn capture() -> Self {
        Self {
            values: std::env::vars_os().collect(),
        }
    }

    pub(crate) fn present(&self, key: &str) -> bool {
        self.values.iter().any(|(name, _)| name == OsStr::new(key))
    }

    pub(crate) fn get(&self, key: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(name, _)| name == OsStr::new(key))
            .and_then(|(_, value)| value.to_str().map(str::to_owned))
    }

    pub(crate) fn value(&self, key: &str) -> Option<String> {
        self.get(key).filter(|value| !value.is_empty())
    }

    pub(crate) fn layer(&self, map: impl Fn(&str) -> Option<String>) -> Result<Value, ConfigError> {
        let mut mapped = Vec::new();
        for (name, value) in &self.values {
            let key = name.to_string_lossy();
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            if let Some(path) = map(key) {
                if path.split('.').any(str::is_empty) {
                    continue;
                }
                mapped.push((path, key.to_owned(), value.to_string_lossy().into_owned()));
            }
        }
        mapped.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let mut root = Value::Object(JsonMap::new());
        for (path, _, text) in mapped {
            let value: figment::value::Value = text.parse().map_err(|_| {
                ConfigError::Invalid("environment: 配置值无法解析（源值未公开）".into())
            })?;
            let nested: Value = figment::util::nest(&path, value)
                .deserialize()
                .map_err(ConfigError::from)?;
            merge_order(&mut root, nested);
        }
        Ok(root)
    }
}

/// 对已发现的平面 TOML 文件名使用 PHP `SORT_NATURAL` 的数字段顺序。
pub(crate) fn natural_name_cmp(left: &str, right: &str) -> Ordering {
    let mut a = left.as_bytes();
    let mut b = right.as_bytes();
    while !a.is_empty() && !b.is_empty() {
        if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
            let a_end = a
                .iter()
                .position(|c| !c.is_ascii_digit())
                .unwrap_or(a.len());
            let b_end = b
                .iter()
                .position(|c| !c.is_ascii_digit())
                .unwrap_or(b.len());
            let aa = &a[..a_end];
            let bb = &b[..b_end];
            let cmp = if aa[0] == b'0' || bb[0] == b'0' {
                aa.cmp(bb)
            } else {
                aa.len().cmp(&bb.len()).then_with(|| aa.cmp(bb))
            };
            if cmp != Ordering::Equal {
                return cmp;
            }
            a = &a[a_end..];
            b = &b[b_end..];
        } else {
            let cmp = a[0].cmp(&b[0]);
            if cmp != Ordering::Equal {
                return cmp;
            }
            a = &a[1..];
            b = &b[1..];
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{ConfigSources, EnvironmentSnapshot, natural_name_cmp};
    use crate::config::repository::ConfigRepository;
    use crate::core::Result;

    // 显式输入的覆盖原位、空根和规范数字键必须保留 PHP 数组形态，且 Figment 仍可读取。
    #[test]
    fn ordered_entries_keep_root_and_nested_php_shapes() -> Result<()> {
        let root = ConfigSources::from_ordered_entries(
            (0..=12).map(|index| (index.to_string(), json!(index))),
        )?;
        let repository = ConfigRepository::new(root.clone());
        assert_eq!(repository.all()?, json!((0..=12).collect::<Vec<_>>()));
        assert_eq!(repository.get("10")?, Some(json!(10)));
        assert_eq!(
            root.figment()
                .extract_inner::<u32>("10")
                .map_err(crate::config::ConfigError::from)?,
            10
        );
        let projected: Value = root
            .figment()
            .extract()
            .map_err(crate::config::ConfigError::from)?;
        assert!(projected.is_object());
        assert_eq!(projected["12"], 12);

        let empty = ConfigRepository::new(ConfigSources::from_ordered_entries(Vec::<(
            String,
            Value,
        )>::new())?);
        assert_eq!(empty.all()?, json!([]));

        let sparse = ConfigRepository::new(ConfigSources::from_ordered_entries([
            ("12", json!("last")),
            ("2", json!("middle")),
            ("0", json!("first")),
        ])?);
        let sparse_raw = sparse.all()?;
        let sparse_keys = sparse_raw
            .as_object()
            .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(sparse_keys, Some(vec!["12", "2", "0"]));

        let mixed = ConfigRepository::new(ConfigSources::from_ordered_entries([
            ("section", json!({"0": "zero", "1": "one"})),
            ("named", json!({"2": "two", "0": "zero", "01": "named"})),
            ("section", json!({"0": "changed", "1": "one"})),
        ])?);
        assert_eq!(mixed.get("section")?, Some(json!(["changed", "one"])));
        let named = mixed.get("named")?.unwrap_or(Value::Null);
        assert!(named.is_object());
        assert_eq!(
            named
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["2", "0", "01"])
        );
        assert_eq!(named["01"], "named");
        Ok(())
    }

    // 来源与投影有差异时构造失败，不以 raw 成功读取掩盖类型化来源不一致。
    #[test]
    fn mismatched_ordered_source_is_rejected_without_exposing_value() {
        let figment = figment::Figment::from(figment::providers::Serialized::defaults(json!({
            "secret": "source-secret"
        })));
        let result = ConfigSources::checked(figment, json!({"secret": "different-secret"}));
        let error = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert_ne!(error, "");
        assert!(!error.contains("source-secret"));
        assert!(!error.contains("different-secret"));
    }

    // 固定 PHP 8.5 `SORT_NATURAL` 探针：数字段不按字典序，也不裁掉前导零。
    #[test]
    fn natural_sort_matches_fixed_php_numeric_segments() {
        let mut names = vec!["foo10", "foo2", "foo001", "foo1", "foo01", "foo0", "foo00"];
        names.sort_by(|left, right| natural_name_cmp(left, right));
        assert_eq!(
            names,
            ["foo0", "foo00", "foo001", "foo01", "foo1", "foo2", "foo10"]
        );
    }

    // 非 UTF-8 环境项沿 Figment 的 vars_os/有损字符策略处理，不能在一次捕获时 panic。
    #[cfg(unix)]
    #[test]
    fn environment_snapshot_handles_non_utf8_without_panicking() -> Result<()> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let snapshot = EnvironmentSnapshot {
            values: vec![
                (
                    OsString::from_vec(b"APP_BROKEN\xff".to_vec()),
                    OsString::from("one"),
                ),
                (
                    OsString::from("APP_NAME"),
                    OsString::from_vec(b"x\xffy".to_vec()),
                ),
            ],
        };
        assert_eq!(snapshot.get("APP_NAME"), None);
        let layer = snapshot.layer(|key| key.strip_prefix("APP_").map(str::to_owned))?;
        assert_eq!(layer["NAME"], "x�y");
        Ok(())
    }
}

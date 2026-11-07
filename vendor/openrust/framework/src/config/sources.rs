//! 同一次加载形成的有序原始配置与类型化来源投影。

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::Path;

use figment::{
    Figment, Metadata, Profile, Provider,
    value::{Dict, Map},
};
use serde_json::{Map as JsonMap, Value};

use super::ConfigError;
use super::code::{ConfigContext, ReadView, TypedValue};
use super::repository::{
    merge_shallow, normalize_php_source, php_value, read_dot_value, reorder_like, replace_section,
    replace_section_overlay, set_dot_value,
};

/// 不可变配置来源；原始键序和 Figment 来源元数据来自同一输入。
///
/// 从已有 [`Figment`] 转换时原始键序已经丢失，仅能使用有损入口。
#[derive(Clone)]
pub struct ConfigSources {
    figment: Figment,
    ordered: Option<Value>,
    file_domains: HashSet<String>,
    pre_file_base: Option<Value>,
    after_file_layers: Vec<Value>,
    cached: bool,
    typed: BTreeMap<String, TypedValue>,
    code_context: Option<ConfigContext>,
}

type SourceParts = (
    Figment,
    Option<Value>,
    HashSet<String>,
    Option<Value>,
    Vec<Value>,
    bool,
    BTreeMap<String, TypedValue>,
    Option<ConfigContext>,
);

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

    /// 按显式根键顺序覆盖来源，并保留同次代码配置环境、typed 值与 profile。
    ///
    /// 提供的路径作为高优先级 Global 层对全部 profile 生效，未覆盖路径保持原值。
    /// 此方法不恢复此前经 `Figment` 丢失的声明顺序。
    /// 已编译缓存来源拒绝再次覆盖，避免绕过缓存冻结语义。
    ///
    /// # Errors
    ///
    /// 缓存来源、原始值与 typed 路径冲突，或来源投影校验失败时返回错误。
    pub fn merge_ordered_entries<K: Into<String>>(
        mut self,
        entries: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<Self, ConfigError> {
        if self.cached {
            return Err(ConfigError::Invalid(
                "configuration: 已编译缓存来源不允许追加覆盖".to_owned(),
            ));
        }
        let overlay = Self::from_ordered_entries(entries)?;
        let ordered = overlay.ordered.ok_or_else(|| {
            ConfigError::Invalid("configuration: 显式覆盖缺少有序原始值".to_owned())
        })?;
        let provider = CapturedProvider::new(&figment::providers::Serialized::globals(
            project_root(&ordered),
        ))?;
        let candidate = self.figment.clone().merge(provider);
        let mut order = self
            .ordered
            .clone()
            .unwrap_or_else(|| Value::Object(JsonMap::new()));
        merge_order(&mut order, ordered.clone());
        let resolved: Value = candidate.extract()?;
        let raw = reorder_like(&order, resolved);
        if candidate.extract::<Value>()? != project_root(&raw) {
            return Err(ConfigError::Invalid(
                "configuration: 显式覆盖的原始值与投影不一致".to_owned(),
            ));
        }
        self.figment = candidate;
        self.ordered = Some(raw);
        if self.pre_file_base.is_some() {
            self.after_file_layers.push(ordered);
        }
        self.validate_typed_paths()?;
        Ok(self)
    }

    /// 借用保留 profile 与来源元数据的 Figment 投影。
    pub fn figment(&self) -> &Figment {
        &self.figment
    }

    pub(crate) fn into_parts(self) -> SourceParts {
        (
            self.figment,
            self.ordered,
            self.file_domains,
            self.pre_file_base,
            self.after_file_layers,
            self.cached,
            self.typed,
            self.code_context,
        )
    }

    pub(crate) fn from_cached(ordered: Value) -> Result<Self, ConfigError> {
        let projected = project_root(&ordered);
        let figment = Figment::from(figment::providers::Serialized::defaults(projected));
        let mut sources = Self::checked(figment, ordered)?;
        sources.cached = true;
        Ok(sources)
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
            file_domains: HashSet::new(),
            pre_file_base: None,
            after_file_layers: Vec::new(),
            cached: false,
            typed: BTreeMap::new(),
            code_context: None,
        })
    }

    pub(crate) fn merge<P: Provider>(
        &mut self,
        provider: P,
        layer: Value,
    ) -> Result<(), ConfigError> {
        let captured = CapturedProvider::new(&provider)?;
        let overlay = Figment::from(captured.clone());
        let mut candidate = self.figment.clone().merge(captured);
        let mut order = self
            .ordered
            .clone()
            .unwrap_or_else(|| Value::Object(JsonMap::new()));
        let layer = normalize_php_source(layer, false);
        merge_order(&mut order, layer.clone());
        if self.pre_file_base.is_some()
            && let Some(entries) = layer.as_object()
        {
            for key in entries.keys() {
                let value = order.get(key).cloned().ok_or_else(|| {
                    ConfigError::Invalid("configuration: 环境投影缺少根键".into())
                })?;
                let old = candidate.find_value(key).ok();
                let source = overlay.find_value(key).ok();
                candidate =
                    replace_section_overlay(&candidate, key, value, old.as_ref(), source.as_ref())
                        .map_err(|_| {
                            ConfigError::Invalid("configuration: 环境投影失败（源值未公开）".into())
                        })?;
            }
        }
        let resolved: Value = candidate.extract()?;
        let ordered = reorder_like(&order, resolved);
        if candidate.extract::<Value>()? != project_root(&ordered) {
            return Err(ConfigError::Invalid(
                "configuration: 来源合并后的值与有序投影不一致".into(),
            ));
        }
        self.figment = candidate;
        self.ordered = Some(ordered);
        if self.pre_file_base.is_some() {
            self.after_file_layers.push(layer);
        }
        Ok(())
    }

    pub(crate) fn merge_file(
        &mut self,
        name: &str,
        file: Value,
        metadata: Metadata,
    ) -> Result<(), ConfigError> {
        let mut ordered = self
            .ordered
            .clone()
            .ok_or_else(|| ConfigError::Invalid("configuration: 文件来源需要有序配置根".into()))?;
        let pre_file_base = self
            .pre_file_base
            .clone()
            .unwrap_or_else(|| ordered.clone());
        let file = normalize_php_source(file, false);
        let mut incoming = file.clone();
        if let Some(base) = read_dot_value(&ordered, name).filter(|value| {
            (value.is_object() || value.is_array()) && (file.is_object() || file.is_array())
        }) {
            let mut merged = base.clone();
            merge_shallow(&mut merged, file.clone(), name);
            for option in mergeable_options(name) {
                if let (Some(old), Some(new)) = (base.get(*option), file.get(*option))
                    && (old.is_object() || old.is_array())
                    && (new.is_object() || new.is_array())
                {
                    let mut combined = old.clone();
                    merge_shallow(&mut combined, new.clone(), option);
                    if let Some(section) = merged.as_object_mut() {
                        section.insert((*option).to_owned(), combined);
                    }
                }
            }
            incoming = merged;
        }
        set_dot_value(&mut ordered, name, incoming.clone());
        let mut file_layer = Value::Object(JsonMap::new());
        set_dot_value(&mut file_layer, name, file);
        let captured =
            CapturedProvider::new(&figment::providers::Serialized::defaults(file_layer))?
                .with_metadata(metadata);
        let merged = self.figment.clone().merge(captured);
        let original = merged.find_value(name).ok();
        let candidate =
            replace_section(&merged, name, incoming, original.as_ref()).map_err(|_| {
                ConfigError::Invalid("configuration: 文件层无法投影（源值未公开）".into())
            })?;
        let checked = Self::checked(candidate, ordered)?;
        self.figment = checked.figment;
        self.ordered = checked.ordered;
        self.file_domains.insert(name.to_owned());
        self.pre_file_base = Some(pre_file_base);
        Ok(())
    }

    pub(crate) fn merge_code_section(
        &mut self,
        name: &str,
        value: Value,
    ) -> Result<(), ConfigError> {
        self.merge_file(name, value, Metadata::named("Rust configuration"))
    }

    pub(crate) fn read_view(&self) -> Result<ReadView, ConfigError> {
        Ok(ReadView {
            raw: self
                .ordered
                .clone()
                .ok_or_else(|| ConfigError::Invalid("代码配置缺少原始视图".to_owned()))?,
            typed: self.typed.keys().cloned().collect(),
        })
    }

    pub(crate) fn set_code_context(&mut self, context: ConfigContext) {
        self.code_context = Some(context);
    }

    pub(crate) fn insert_typed(&mut self, key: &str, value: TypedValue) -> Result<(), ConfigError> {
        if self
            .typed
            .keys()
            .any(|existing| paths_overlap(existing, key))
        {
            return Err(ConfigError::Invalid(
                "代码配置类型路径重复（源值未公开）".to_owned(),
            ));
        }
        self.typed.insert(key.to_owned(), value);
        Ok(())
    }

    pub(crate) fn validate_typed_paths(&self) -> Result<(), ConfigError> {
        let raw = self
            .ordered
            .as_ref()
            .ok_or_else(|| ConfigError::Invalid("代码配置缺少原始视图".to_owned()))?;
        for key in self.typed.keys() {
            if raw_path_conflicts(raw, key) {
                return Err(ConfigError::Invalid(
                    "代码配置类型路径与原始值冲突（源值未公开）".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}."))
        || right.starts_with(&format!("{left}."))
}

pub(crate) fn raw_path_conflicts(raw: &Value, typed_key: &str) -> bool {
    // 空来源根按 PHP 数组投影成 []，尚未占用任何命名路径；嵌套数组仍会遮蔽子路径。
    if raw.as_array().is_some_and(Vec::is_empty) {
        return false;
    }
    let mut current = raw;
    for part in typed_key.split('.') {
        let Some(next) = current.as_object().and_then(|map| map.get(part)) else {
            return !current.is_object();
        };
        current = next;
    }
    true
}

pub(crate) fn mergeable_options(name: &str) -> &'static [&'static str] {
    match name {
        "auth" => &["guards", "providers", "passwords"],
        "broadcasting" | "database" | "queue" => &["connections"],
        "cache" => &["stores"],
        "filesystems" => &["disks"],
        "logging" => &["channels"],
        "mail" => &["mailers"],
        _ => &[],
    }
}

impl From<Figment> for ConfigSources {
    fn from(figment: Figment) -> Self {
        Self {
            figment,
            ordered: None,
            file_domains: HashSet::new(),
            pre_file_base: None,
            after_file_layers: Vec::new(),
            cached: false,
            typed: BTreeMap::new(),
            code_context: None,
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
#[derive(Clone, Default)]
pub(crate) struct EnvironmentSnapshot {
    values: Vec<(OsString, OsString)>,
}

impl EnvironmentSnapshot {
    pub(crate) fn capture() -> Self {
        Self {
            values: std::env::vars_os().collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_dotenv(&self) -> Result<Self, ConfigError> {
        self.with_dotenv_at(Path::new(".env"))
    }

    pub(crate) fn with_dotenv_at(&self, path: &Path) -> Result<Self, ConfigError> {
        let entries = match dotenvy::from_path_iter(path) {
            Ok(entries) => entries,
            Err(error) if error.not_found() => return Ok(self.clone()),
            Err(_) => return Err(ConfigError::Invalid("无法读取 .env 文件".to_owned())),
        };
        let mut merged = self.clone();
        for entry in entries {
            let (key, value) =
                entry.map_err(|_| ConfigError::Invalid(".env 文件格式无效".to_owned()))?;
            if !merged.present(&key) {
                merged.values.push((key.into(), value.into()));
            }
        }
        Ok(merged)
    }

    pub(crate) fn get_os(&self, key: &str) -> Option<&OsStr> {
        self.values
            .iter()
            .find(|(name, _)| name == OsStr::new(key))
            .map(|(_, value)| value.as_os_str())
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
                let text = if matches!(path.as_str(), "app.key" | "app.previous_keys") {
                    value
                        .to_str()
                        .ok_or_else(|| {
                            ConfigError::Invalid("应用密钥环境配置要求 UTF-8 文本".into())
                        })?
                        .to_owned()
                } else {
                    value.to_string_lossy().into_owned()
                };
                mapped.push((path, key.to_owned(), text));
            }
        }
        mapped.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let mut root = Value::Object(JsonMap::new());
        for (path, key, text) in mapped {
            if path == "app.debug" {
                let value = super::code::parse_bool(&key, &text)?;
                merge_order(&mut root, serde_json::json!({ "app": { "debug": value } }));
                continue;
            }
            if let Some(field @ ("key" | "previous_keys")) = path.strip_prefix("app.") {
                let value = super::code::nullable_env_string(&text);
                merge_order(&mut root, serde_json::json!({ "app": { (field): value } }));
                continue;
            }
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

/// 对映射后的完整配置键使用 PHP `SORT_NATURAL` 的数字段顺序。
/// 自然文件名排序的跨包复用入口；外部资源扩展共享同一配置排序语义。
#[doc(hidden)]
pub fn natural_name_cmp(left: &str, right: &str) -> Ordering {
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

    #[test]
    fn empty_raw_root_allows_typed_paths_without_relaxing_array_or_scalar_conflicts() -> Result<()>
    {
        let mut sources = ConfigSources::from_ordered_entries(Vec::<(String, Value)>::new())?;
        assert_eq!(sources.read_view()?.raw, json!([]));
        sources.insert_typed("runtime.policy", std::sync::Arc::new(17_u8))?;
        sources.validate_typed_paths()?;
        let repository = ConfigRepository::new(sources);
        assert_eq!(repository.all()?, json!([]));
        assert_eq!(
            repository.typed::<u8>("runtime.policy")?.as_deref(),
            Some(&17)
        );

        for raw in [json!([]), json!({}), json!({"unrelated":[]})] {
            assert!(!super::raw_path_conflicts(&raw, "runtime.policy"));
        }
        for raw in [
            json!([1]),
            json!([{}]),
            json!(null),
            json!(false),
            json!(0),
            json!("value"),
            json!({"runtime":[]}),
            json!({"runtime":[1]}),
            json!({"runtime":null}),
            json!({"runtime":{"policy":[]}}),
        ] {
            assert!(super::raw_path_conflicts(&raw, "runtime.policy"));
        }
        Ok(())
    }

    #[test]
    fn ordered_overrides_preserve_code_context_profile_metadata_and_typed_boundaries() -> Result<()>
    {
        use crate::config::{ConfigLoader, ConfigManifest, Environment};
        let snapshot = EnvironmentSnapshot {
            values: vec![("OVERLAY_SENTINEL".into(), "captured-once".into())],
        };
        let loader = ConfigLoader::from_snapshot(Environment::Testing, "APP_", snapshot)?;
        let manifest = ConfigManifest::new()
            .section("reading", |_| Ok(json!({"name": "base", "keep": true})))
            .typed("factory", |_| Ok(42_u32));
        let mut original = loader.code_sources(&manifest)?;
        original.figment = original
            .figment
            .clone()
            .merge(figment::providers::Serialized::from(
                json!({"profiled": {"name": "test", "keep": "test-only"}}),
                "testing",
            ))
            .merge(figment::providers::Serialized::from(
                json!({"profiled": {"name": "prod", "keep": "prod-only"}}),
                "production",
            ))
            .select("testing");
        let metadata = original
            .figment
            .find_metadata("reading.keep")
            .map(|value| value.name.clone());
        let merged = original.clone().merge_ordered_entries([
            ("reading", json!({"name": "override", "z": 1, "a": 2})),
            ("empty", Value::Null),
            ("profiled", json!({"name": "shared-override"})),
        ])?;
        for (profile, keep) in [("testing", "test-only"), ("production", "prod-only")] {
            let selected = merged.figment.clone().select(profile);
            assert_eq!(
                selected
                    .extract_inner::<String>("profiled.name")
                    .map_err(crate::config::ConfigError::from)?,
                "shared-override"
            );
            assert_eq!(
                selected
                    .extract_inner::<String>("profiled.keep")
                    .map_err(crate::config::ConfigError::from)?,
                keep
            );
        }
        assert_eq!(merged.figment.profile(), original.figment.profile());
        assert_eq!(
            merged
                .figment
                .find_metadata("reading.keep")
                .map(|value| value.name.clone()),
            metadata
        );
        assert_eq!(
            merged
                .code_context
                .as_ref()
                .and_then(|context| context.env("OVERLAY_SENTINEL")),
            Some("captured-once".to_owned())
        );
        let repository = ConfigRepository::new(merged);
        assert_eq!(repository.string("reading.name")?, "override");
        assert!(repository.boolean("reading.keep")?);
        assert_eq!(repository.get("empty")?, Some(Value::Null));
        assert_eq!(repository.typed::<u32>("factory")?.as_deref(), Some(&42));
        let keys = repository.get("reading")?.and_then(|value| {
            value
                .as_object()
                .map(|values| values.keys().cloned().collect::<Vec<_>>())
        });
        assert_eq!(
            keys,
            Some(vec![
                "name".to_owned(),
                "keep".to_owned(),
                "z".to_owned(),
                "a".to_owned()
            ])
        );
        assert!(
            original
                .merge_ordered_entries([("factory", json!({"nested": 1}))])
                .is_err()
        );
        let cached = ConfigSources::from_cached(json!({"reading": {"name": "cached"}}))?;
        assert!(
            cached
                .clone()
                .merge_ordered_entries([("reading", json!({"name": "changed"}))])
                .is_err()
        );
        assert_eq!(
            ConfigRepository::new(cached).string("reading.name")?,
            "cached"
        );
        Ok(())
    }

    #[test]
    fn file_base_merge_is_shallow_except_laravel_mergeable_option() -> Result<()> {
        let mut sources = ConfigSources::from_ordered_entries([
            ("app", json!({"name": "base", "nested": {"old": 1}})),
            (
                "cache",
                json!({"default": "array", "stores": {"array": {"driver": "array"}}, "nested": {"old": 1}}),
            ),
        ])?;
        sources.merge_file(
            "app",
            json!({"name": "custom", "nested": {"new": 2}}),
            figment::Metadata::named("TOML file"),
        )?;
        sources.merge_file(
            "cache",
            json!({"default": "redis", "stores": {"redis": {"driver": "redis"}}, "nested": {"new": 2}}),
            figment::Metadata::named("TOML file"),
        )?;
        let repository = ConfigRepository::new(sources);
        assert_eq!(repository.get("app.nested")?, Some(json!({"new": 2})));
        assert_eq!(repository.get("cache.nested")?, Some(json!({"new": 2})));
        assert_eq!(
            repository.get("cache.stores")?,
            Some(json!({"array": {"driver": "array"}, "redis": {"driver": "redis"}}))
        );
        Ok(())
    }

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

    #[test]
    fn application_key_environment_keeps_laravel_text_and_reserved_values() -> Result<()> {
        let map = |name: &str| super::super::namespaced_environment_path(name, "APP_");
        for (raw, expected) in [
            ("00123", json!("00123")),
            (" base64:AA== ", json!(" base64:AA== ")),
            ("[x,y]", json!("[x,y]")),
            ("(TrUe)", json!("1")),
            ("false", json!("")),
            ("(empty)", json!("")),
            ("(null)", Value::Null),
            ("'null'", json!("null")),
        ] {
            let snapshot = EnvironmentSnapshot {
                values: vec![("APP_KEY".into(), raw.into())],
            };
            assert_eq!(snapshot.layer(map)?["app"]["key"], expected);
        }
        let snapshot = EnvironmentSnapshot {
            values: vec![(
                "APP_PREVIOUS_KEYS".into(),
                "00123,0,, previous ,base64:AA==".into(),
            )],
        };
        let layer = snapshot.layer(map)?;
        let config: crate::config::AppConfig =
            serde_json::from_value(layer["app"].clone()).map_err(crate::Error::internal)?;
        assert_eq!(config.previous_keys, ["00123", " previous ", "base64:AA=="]);
        Ok(())
    }

    #[test]
    fn application_key_dotenv_uses_selected_file_and_process_precedence() -> Result<()> {
        let map = |name: &str| super::super::namespaced_environment_path(name, "SHOP_");
        let directory = tempfile::tempdir().map_err(crate::Error::internal)?;
        let path = directory.path().join(".env.staging");
        std::fs::write(&path, "SHOP_KEY=0007\nSHOP_PREVIOUS_KEYS=' old ,0,00123'\n")
            .map_err(crate::Error::internal)?;
        let snapshot = EnvironmentSnapshot { values: Vec::new() };
        let dotenv = snapshot.with_dotenv_at(&path)?.layer(map)?;
        assert_eq!(dotenv["app"]["key"], "0007");
        assert_eq!(dotenv["app"]["previous_keys"], " old ,0,00123");
        let snapshot = EnvironmentSnapshot {
            values: vec![("SHOP_KEY".into(), " 00123 ".into())],
        };
        let merged = snapshot.with_dotenv_at(&path)?.layer(map)?;
        assert_eq!(merged["app"]["key"], " 00123 ");
        assert_eq!(
            merged["app"]["previous_keys"],
            dotenv["app"]["previous_keys"]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn application_key_rejects_non_utf8_without_disclosing_the_source() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let snapshot = EnvironmentSnapshot {
            values: vec![(
                "APP_KEY".into(),
                OsString::from_vec(b"secret-sentinel\xff".to_vec()),
            )],
        };
        let error = snapshot
            .layer(|name| super::super::namespaced_environment_path(name, "APP_"))
            .err();
        assert!(matches!(
            &error,
            Some(crate::config::ConfigError::Invalid(_))
        ));
        assert!(!format!("{error:?}").contains("secret-sentinel"));
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

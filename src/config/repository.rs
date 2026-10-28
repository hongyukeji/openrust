//! Provider 共享的版本化配置仓储；区段默认补齐不替代包配置的专用合并操作。

use std::any::TypeId;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use figment::{
    Figment, Metadata, Profile, Provider,
    providers::Serialized,
    value::{Dict, Map, Tag, Value as FigmentValue},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::config::{ConfigError, ConfigSourceError};
use crate::core::{Error, Result};

/// 已注册区段的结构约束；未注册区段不会被全树严格校验误拒。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionSchema {
    /// 允许任意值，适用于公开 driver 的自有选项。
    Any,
    /// 布尔值。
    Boolean,
    /// 字符串；错误不会包含字符串内容。
    String,
    /// JSON 数值。
    Number,
    /// 有符号或无符号整数。
    Integer,
    /// 已知字段组成的对象。
    Object {
        /// 字段的递归约束；是否必填由类型化反序列化决定。
        fields: BTreeMap<String, Self>,
        /// 是否容许未列出的字段。
        allow_unknown: bool,
    },
    /// 名称动态、每项具有同一结构的对象。
    Map(Box<Self>),
    /// 每个元素遵循同一结构的数组。
    Array(Box<Self>),
    /// 允许 null 或指定结构。
    Nullable(Box<Self>),
    /// 标识敏感字段；约束与内部结构相同，不在错误中输出值。
    Sensitive(Box<Self>),
}

impl SectionSchema {
    /// 构造允许任意 driver 选项的节点。
    pub fn any() -> Self {
        Self::Any
    }

    /// 构造拒绝未知字段的对象。
    pub fn object<K: Into<String>>(fields: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::Object {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
            allow_unknown: false,
        }
    }

    /// 允许对象中声明字段以外的键；对非对象节点不作改变。
    #[must_use]
    pub fn open(mut self) -> Self {
        if let Self::Object { allow_unknown, .. } = &mut self {
            *allow_unknown = true;
        }
        self
    }

    /// 构造动态命名 map。
    pub fn map(item: Self) -> Self {
        Self::Map(Box::new(item))
    }

    /// 构造数组。
    pub fn array(item: Self) -> Self {
        Self::Array(Box::new(item))
    }

    /// 允许当前约束出现 null。
    #[must_use]
    pub fn nullable(self) -> Self {
        Self::Nullable(Box::new(self))
    }

    /// 标记敏感字段；仓储 Debug 始终不输出任何配置值。
    #[must_use]
    pub fn sensitive(self) -> Self {
        Self::Sensitive(Box::new(self))
    }

    fn validate(&self, value: &Value, path: &str) -> Result<()> {
        let valid = match self {
            Self::Any => true,
            Self::Boolean => value.is_boolean(),
            Self::String => value.is_string(),
            Self::Number => value.is_number(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Nullable(_) if value.is_null() => true,
            Self::Nullable(inner) | Self::Sensitive(inner) => return inner.validate(value, path),
            Self::Object {
                fields,
                allow_unknown,
            } => {
                let Some(object) = value.as_object() else {
                    return Err(invalid(path, "expected object"));
                };
                for (key, item) in object {
                    let child = format!("{path}.{key}");
                    if let Some(schema) = fields.get(key) {
                        schema.validate(item, &child)?;
                    } else if !allow_unknown {
                        return Err(invalid(&child, "unknown field"));
                    }
                }
                true
            }
            Self::Map(schema) => {
                let Some(object) = value.as_object() else {
                    return Err(invalid(path, "expected named map"));
                };
                for (key, item) in object {
                    schema.validate(item, &format!("{path}.{key}"))?;
                }
                true
            }
            Self::Array(schema) => {
                let Some(array) = value.as_array() else {
                    return Err(invalid(path, "expected array"));
                };
                for (index, item) in array.iter().enumerate() {
                    schema.validate(item, &format!("{path}[{index}]"))?;
                }
                true
            }
        };
        if valid {
            Ok(())
        } else {
            Err(invalid(path, "schema type mismatch"))
        }
    }
}

type Validator = Arc<dyn Fn(&Figment) -> Result<()> + Send + Sync>;

#[derive(Clone)]
struct Section {
    defaults: Value,
    schema: SectionSchema,
    validate: Validator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Register,
    Boot,
    Frozen,
}

#[derive(Clone)]
enum PackageOperation {
    Merge(&'static str, Value),
    Replace(&'static str, Value),
}

struct State {
    figment: Figment,
    sections: BTreeMap<&'static str, Section>,
    operations: Vec<PackageOperation>,
    phase: Phase,
    version: u64,
    revision: u64,
}

/// 同一应用各 Provider 共享的配置视图；克隆不会复制独立仓储。
///
/// 校验器在锁外执行；候选校验失败或并发修改时不会发布半成品。
/// `snapshot` 含原始配置，交给 config show 前仍须使用既有脱敏器。
#[derive(Clone)]
pub struct ConfigRepository {
    inner: Arc<Mutex<State>>,
}

impl fmt::Debug for ConfigRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        formatter
            .debug_struct("ConfigRepository")
            .field("phase", &state.phase)
            .field("version", &state.version)
            .field("sections", &state.sections.len())
            .finish_non_exhaustive()
    }
}

impl ConfigRepository {
    /// 保留现有来源及其优先级，开始 Provider 注册阶段。
    pub fn new(sources: Figment) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                figment: sources,
                sections: BTreeMap::new(),
                operations: Vec::new(),
                phase: Phase::Register,
                version: 0,
                revision: 0,
            })),
        }
    }

    /// 注册阶段只检查原始配置来源是否声明区段；尚未加入 Provider 默认值。
    pub fn source_has_section(&self, key: &str) -> std::result::Result<bool, ConfigError> {
        validate_key(key).map_err(|error| ConfigError::Invalid(error.to_string()))?;
        let state = self.lock();
        match state.figment.find_value(key) {
            Ok(_) => Ok(true),
            Err(error) if error.missing() => Ok(false),
            Err(error) => Err(safe_source_error(error, key)),
        }
    }

    /// 登记类型化默认值、结构与校验器；默认值不会覆盖现有文件或环境值。
    ///
    /// 同名登记一律拒绝，避免忽略后来登记的校验器。
    /// 本操作仅补齐区段默认值，不代表 Laravel 包的浅合并或递归替换操作。
    pub fn register_section<T>(
        &mut self,
        _owner: TypeId,
        key: &'static str,
        defaults: T,
        schema: SectionSchema,
        validate: fn(&T) -> Result<()>,
    ) -> Result<()>
    where
        T: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        validate_key(key)?;
        let defaults = serde_json::to_value(defaults)
            .map_err(|_| invalid(key, "defaults serialization failed"))?;
        let mut state = self.lock();
        require_phase(state.phase, Phase::Register)?;
        if state.sections.contains_key(key) {
            return Err(invalid(key, "section registration conflict"));
        }
        if state.sections.keys().any(|other| {
            key.starts_with(&format!("{other}.")) || other.starts_with(&format!("{key}."))
        }) {
            return Err(invalid(key, "overlapping section registration"));
        }
        let validator_schema = schema.clone();
        let validator = Arc::new(move |figment: &Figment| {
            let value = figment.extract_inner::<T>(key).map_err(|error| {
                Error::Config(safe_source_error_with_schema(
                    error,
                    key,
                    Some(&validator_schema),
                ))
            })?;
            validate(&value).map_err(|_| invalid(key, "section validation failed"))
        });
        state.sections.insert(
            key,
            Section {
                defaults,
                schema,
                validate: validator,
            },
        );
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    /// 登记 Laravel 包的浅层默认合并；已有对象键整体获胜，数组连接并重新编号。
    ///
    /// 仅 Register 阶段排队，`validate_base` 时按登记顺序执行。
    pub fn merge_defaults(&mut self, key: &'static str, defaults: Value) -> Result<()> {
        self.queue_operation(key, defaults, false)
    }

    /// 登记包的递归默认合并；已有值胜，数组按索引替换并保留默认尾部。
    ///
    /// 与 boot 模块覆盖的方向相反；仅 Register 阶段排队。
    pub fn replace_defaults_recursively(
        &mut self,
        key: &'static str,
        defaults: Value,
    ) -> Result<()> {
        self.queue_operation(key, defaults, true)
    }

    fn queue_operation(&self, key: &'static str, defaults: Value, recursive: bool) -> Result<()> {
        validate_key(key)?;
        if !defaults.is_object() && !defaults.is_array() {
            return Err(invalid(key, "package defaults must be object or array"));
        }
        let mut state = self.lock();
        require_phase(state.phase, Phase::Register)?;
        state.operations.push(if recursive {
            PackageOperation::Replace(key, defaults)
        } else {
            PackageOperation::Merge(key, defaults)
        });
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    /// 补齐 typed 默认值后重放包操作并校验；成功后封闭注册并进入 boot。
    pub fn validate_base(&mut self) -> Result<()> {
        let (mut candidate, sections, operations, revision) = {
            let state = self.lock();
            require_phase(state.phase, Phase::Register)?;
            (
                state.figment.clone(),
                state.sections.clone(),
                state.operations.clone(),
                state.revision,
            )
        };
        for (key, section) in &sections {
            candidate = candidate.join(Serialized::default(key, &section.defaults));
        }
        for operation in operations {
            let (key, mut defaults, recursive) = match operation {
                PackageOperation::Merge(key, value) => (key, value, false),
                PackageOperation::Replace(key, value) => (key, value, true),
            };
            let current = match candidate.extract_inner::<Value>(key) {
                Ok(value) => value,
                Err(error) if error.missing() => {
                    if defaults.is_array() {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(serde_json::Map::new())
                    }
                }
                Err(error) => return Err(Error::Config(safe_source_error(error, key))),
            };
            if !current.is_object() && !current.is_array() {
                return Err(invalid(
                    key,
                    "package configuration must be object or array",
                ));
            }
            let original = candidate.find_value(key).ok();
            if recursive {
                replace_recursive(&mut defaults, current, key)?;
            } else {
                merge_shallow(&mut defaults, current, key);
            }
            candidate = replace_section(&candidate, key, defaults, original.as_ref())?;
        }
        validate_candidate(&candidate, &sections)?;
        self.publish(candidate, revision, Phase::Register, Phase::Boot, true)
    }

    /// 从当前已发布视图提取区段；包括未注册的应用自有区段。
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> std::result::Result<T, ConfigError> {
        let state = self.lock();
        let figment = state.figment.clone();
        let schema = state
            .sections
            .get(key)
            .map(|section| section.schema.clone());
        drop(state);
        figment
            .extract_inner(key)
            .map_err(|error| safe_source_error_with_schema(error, key, schema.as_ref()))
    }

    /// 返回独立的来源快照；包含秘密值，调用方必须执行适用的输出脱敏。
    pub fn snapshot(&self) -> Figment {
        self.lock().figment.clone()
    }

    /// 已发布配置版本；注册元数据和 freeze 不改变配置版本。
    pub fn version(&self) -> u64 {
        self.lock().version
    }

    /// 在 boot 时点递归覆盖模块区段；数组按索引替换并保留旧尾。
    ///
    /// 校验整个已注册配置集合后才发布；错误不改变旧视图或版本。
    pub fn overlay(&mut self, section: &str, value: Value) -> Result<()> {
        validate_key(section)?;
        let (base, sections, revision) = {
            let state = self.lock();
            require_phase(state.phase, Phase::Boot)?;
            (
                state.figment.clone(),
                state.sections.clone(),
                state.revision,
            )
        };
        let mut existing = match base.extract_inner::<Value>(section) {
            Ok(value) => value,
            Err(error) if error.missing() => Value::Null,
            Err(error) => return Err(Error::Config(safe_source_error(error, section))),
        };
        let original = base.find_value(section).ok();
        replace_recursive(&mut existing, value, section)?;
        let candidate = replace_section(&base, section, existing, original.as_ref())?;
        validate_candidate(&candidate, &sections)?;
        self.publish(candidate, revision, Phase::Boot, Phase::Boot, true)
    }

    /// 最终校验并冻结；重复冻结幂等，之后拒绝注册或覆盖。
    pub fn freeze(&mut self) -> Result<()> {
        let (candidate, sections, revision) = {
            let state = self.lock();
            if state.phase == Phase::Frozen {
                return Ok(());
            }
            require_phase(state.phase, Phase::Boot)?;
            (
                state.figment.clone(),
                state.sections.clone(),
                state.revision,
            )
        };
        validate_candidate(&candidate, &sections)?;
        self.publish(candidate, revision, Phase::Boot, Phase::Frozen, false)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn publish(
        &self,
        candidate: Figment,
        revision: u64,
        before: Phase,
        after: Phase,
        changed: bool,
    ) -> Result<()> {
        let mut state = self.lock();
        require_phase(state.phase, before)?;
        if state.revision != revision {
            return Err(invalid("configuration", "concurrent configuration update"));
        }
        let version = if changed {
            state
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("configuration", "version exhausted"))?
        } else {
            state.version
        };
        state.figment = candidate;
        state.phase = after;
        state.version = version;
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }
}

// 保留 Figment 的原始 tag 与 metadata，仅将已解析的目标 section 发布到 global。
// 清理各 profile 的旧 section 是浅替换删除键所必需的。
struct ReplacedProvider {
    data: Map<Profile, Dict>,
    metadata: Map<Tag, Metadata>,
    profile: Profile,
}

impl Provider for ReplacedProvider {
    fn metadata(&self) -> Metadata {
        Metadata::named("configuration operation")
    }
    fn data(&self) -> std::result::Result<Map<Profile, Dict>, figment::Error> {
        Ok(self.data.clone())
    }
    fn profile(&self) -> Option<Profile> {
        Some(self.profile.clone())
    }
    fn __metadata_map(&self) -> Option<Map<Tag, Metadata>> {
        Some(self.metadata.clone())
    }
}

fn remove_path(dict: &mut Dict, parts: &[&str]) {
    if let Some((head, tail)) = parts.split_first() {
        if tail.is_empty() {
            dict.remove(*head);
        } else if let Some(FigmentValue::Dict(_, child)) = dict.get_mut(*head) {
            remove_path(child, tail);
        }
    }
}

fn insert_path(dict: &mut Dict, parts: &[&str], value: FigmentValue) {
    if let Some((head, tail)) = parts.split_first() {
        if tail.is_empty() {
            dict.insert((*head).to_owned(), value);
        } else {
            let child = dict
                .entry((*head).to_owned())
                .or_insert_with(|| FigmentValue::Dict(Tag::Default, Dict::new()));
            if !matches!(child, FigmentValue::Dict(_, _)) {
                *child = FigmentValue::Dict(Tag::Default, Dict::new());
            }
            if let FigmentValue::Dict(_, child) = child {
                insert_path(child, tail, value);
            }
        }
    }
}

fn preserve_tags(new: &mut FigmentValue, old: &FigmentValue) {
    if new.deserialize::<Value>().ok() == old.deserialize::<Value>().ok() {
        *new = old.clone();
        return;
    }
    match (new, old) {
        (FigmentValue::Dict(_, new), FigmentValue::Dict(_, old)) => {
            for (key, value) in new {
                if let Some(previous) = old.get(key) {
                    preserve_tags(value, previous);
                }
            }
        }
        (FigmentValue::Array(_, new), FigmentValue::Array(_, old)) => {
            for (value, previous) in new.iter_mut().zip(old) {
                preserve_tags(value, previous);
            }
        }
        _ => {}
    }
}

fn replace_section(
    base: &Figment,
    key: &str,
    value: Value,
    old: Option<&FigmentValue>,
) -> Result<Figment> {
    let mut data = base
        .data()
        .map_err(|error| Error::Config(safe_source_error(error, key)))?;
    let mut replacement = FigmentValue::serialize(value)
        .map_err(|error| Error::Config(safe_source_error(error, key)))?;
    if let Some(old) = old {
        preserve_tags(&mut replacement, old);
    }
    let parts: Vec<_> = key.split('.').collect();
    for dict in data.values_mut() {
        remove_path(dict, &parts);
    }
    insert_path(
        data.entry(Profile::Global).or_default(),
        &parts,
        replacement,
    );
    let provider = ReplacedProvider {
        data,
        metadata: base.__metadata_map().unwrap_or_default(),
        profile: base.profile().clone(),
    };
    Ok(Figment::from(provider))
}

fn safe_source_error(source: figment::Error, key: &str) -> ConfigError {
    safe_source_error_with_schema(source, key, None)
}

fn schema_has_field(schema: &SectionSchema, field: &str) -> bool {
    match schema {
        SectionSchema::Object { fields, .. } => {
            fields.contains_key(field)
                || fields.values().any(|child| schema_has_field(child, field))
        }
        SectionSchema::Map(child)
        | SectionSchema::Array(child)
        | SectionSchema::Nullable(child)
        | SectionSchema::Sensitive(child) => schema_has_field(child, field),
        _ => false,
    }
}

fn safe_source_error_with_schema(
    source: figment::Error,
    key: &str,
    schema: Option<&SectionSchema>,
) -> ConfigError {
    let missing = source.missing();
    let message = source
        .into_iter()
        .map(|error| {
            let mut nested = error.path.join(".");
            if nested.ends_with(&format!(".{key}")) {
                nested = error
                    .path
                    .iter()
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(".");
            }
            let mut path = if nested.is_empty() {
                key.to_owned()
            } else if key.is_empty() || nested == key || nested.starts_with(&format!("{key}.")) {
                nested
            } else {
                format!("{key}.{nested}")
            };
            if let figment::error::Kind::MissingField(std::borrow::Cow::Borrowed(field)) =
                &error.kind
                && schema.is_some_and(|schema| schema_has_field(schema, field))
                && !field.is_empty()
                && field
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                path.push('.');
                path.push_str(field);
            }
            let source = match error
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.source.as_ref())
            {
                Some(figment::Source::File(_)) => "file",
                Some(figment::Source::Code(_)) => "code",
                Some(figment::Source::Custom(_)) => "custom",
                _ => "unknown",
            };
            let rule = match error.kind {
                figment::error::Kind::MissingField(_) => "missing field",
                figment::error::Kind::InvalidType(_, _) => "invalid type",
                figment::error::Kind::UnknownField(_, _) => "unknown field",
                figment::error::Kind::UnknownVariant(_, _) => "unknown variant",
                _ => "invalid configuration value",
            };
            format!("{path}: {rule} (source: {source})")
        })
        .collect::<Vec<_>>()
        .join("; ");
    ConfigError::Source(ConfigSourceError { message, missing })
}

fn invalid(path: &str, rule: &str) -> Error {
    Error::Config(ConfigError::Invalid(format!("{path}: {rule}")))
}

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.split('.').any(str::is_empty) {
        return Err(invalid("configuration", "invalid section path"));
    }
    Ok(())
}

fn require_phase(actual: Phase, required: Phase) -> Result<()> {
    if actual == required {
        Ok(())
    } else {
        Err(invalid(
            "configuration",
            "configuration phase forbids mutation",
        ))
    }
}

fn validate_candidate(figment: &Figment, sections: &BTreeMap<&'static str, Section>) -> Result<()> {
    // 即使没有注册 schema，来源解析错误也不能被视为有效空配置。
    figment
        .extract::<Value>()
        .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?;
    for (key, section) in sections {
        let value = figment
            .extract_inner::<Value>(key)
            .map_err(|error| Error::Config(safe_source_error(error, key)))?;
        section.schema.validate(&value, key)?;
        (section.validate)(figment)?;
    }
    Ok(())
}

// JSON 数组表示连续整数键，对象表示命名、稀疏或混合键。
fn php_entries(value: Value) -> Vec<(String, Value)> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        Value::Object(items) => items.into_iter().collect(),
        _ => Vec::new(),
    }
}

fn integer_key(key: &str) -> Option<i64> {
    let number = key.parse::<i64>().ok()?;
    (number.to_string() == key).then_some(number)
}

fn php_value(entries: Vec<(String, Value)>, empty_array: bool) -> Value {
    if entries.is_empty() {
        return if empty_array {
            Value::Array(Vec::new())
        } else {
            Value::Object(serde_json::Map::new())
        };
    }
    if entries
        .iter()
        .enumerate()
        .all(|(i, (key, _))| key == &i.to_string())
    {
        return Value::Array(entries.into_iter().map(|(_, value)| value).collect());
    }
    Value::Object(entries.into_iter().collect())
}

fn merge_shallow(defaults: &mut Value, current: Value, _path: &str) {
    let old = std::mem::take(defaults);
    let empty_array = old.is_array() && current.is_array();
    let mut entries: Vec<(String, Value)> = Vec::new();
    let mut next = 0usize;
    for (key, value) in php_entries(old).into_iter().chain(php_entries(current)) {
        if integer_key(&key).is_some() {
            entries.push((next.to_string(), value));
            next += 1;
        } else if let Some((_, previous)) = entries.iter_mut().find(|(name, _)| name == &key) {
            *previous = value;
        } else {
            entries.push((key, value));
        }
    }
    *defaults = php_value(entries, empty_array);
}

fn replace_recursive(existing: &mut Value, overlay: Value, path: &str) -> Result<()> {
    if (existing.is_object() || existing.is_array()) && (overlay.is_object() || overlay.is_array())
    {
        let old = std::mem::take(existing);
        let empty_array = old.is_array() && overlay.is_array();
        let mut entries = php_entries(old);
        for (key, value) in php_entries(overlay) {
            if let Some((_, previous)) = entries.iter_mut().find(|(name, _)| name == &key) {
                replace_recursive(previous, value, &format!("{path}.{key}"))?;
            } else {
                entries.push((key, value));
            }
        }
        *existing = php_value(entries, empty_array);
    } else {
        *existing = overlay;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    struct Settings {
        name: String,
        count: u32,
        items: Vec<String>,
    }

    fn defaults() -> Settings {
        Settings {
            name: "default".into(),
            count: 10,
            items: vec!["a".into(), "b".into(), "c".into()],
        }
    }

    fn schema() -> SectionSchema {
        SectionSchema::object([
            ("name", SectionSchema::String.sensitive()),
            ("count", SectionSchema::Integer),
            ("items", SectionSchema::array(SectionSchema::String)),
        ])
    }

    fn validate(settings: &Settings) -> Result<()> {
        if settings.count == 0 {
            Err(Error::BadRequest(format!(
                "secret-validator-value:{}",
                settings.name
            )))
        } else {
            Ok(())
        }
    }

    fn registered(source: Value) -> Result<ConfigRepository> {
        let mut repository = ConfigRepository::new(Figment::from(Serialized::defaults(source)));
        repository.register_section(
            TypeId::of::<Settings>(),
            "demo",
            defaults(),
            schema(),
            validate,
        )?;
        Ok(repository)
    }

    #[test]
    fn defaults_join_preserves_sources_and_unregistered_sections() -> Result<()> {
        let mut repository =
            registered(json!({"demo":{"name":"source"},"custom":{"enabled":true}}))?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Settings>("demo")?,
            Settings {
                name: "source".into(),
                ..defaults()
            }
        );
        assert!(repository.section::<bool>("custom.enabled")?);
        Ok(())
    }

    #[test]
    fn overlay_replaces_indices_keeps_tail_and_empty_array() -> Result<()> {
        let mut repository = registered(json!({}))?;
        repository.validate_base()?;
        repository.overlay("demo", json!({"items":["x"],"name":"module"}))?;
        assert_eq!(
            repository.section::<Settings>("demo")?.items,
            ["x", "b", "c"]
        );
        repository.overlay("demo", json!({"items":[]}))?;
        assert_eq!(
            repository.section::<Settings>("demo")?.items,
            ["x", "b", "c"]
        );
        Ok(())
    }

    #[test]
    fn clones_observe_new_version_but_owned_snapshot_stays_old() -> Result<()> {
        let mut repository = registered(json!({}))?;
        let observer = repository.clone();
        repository.validate_base()?;
        let snapshot = observer.snapshot();
        let previous = observer.version();
        repository.overlay("demo", json!({"name":"module"}))?;
        assert_eq!(observer.version(), previous + 1);
        assert_eq!(observer.section::<Settings>("demo")?.name, "module");
        assert_eq!(
            snapshot
                .extract_inner::<Settings>("demo")
                .map_err(|_| invalid("demo", "test extraction"))?
                .name,
            "default"
        );
        Ok(())
    }

    #[test]
    fn rejected_overlay_is_atomic_and_validator_errors_are_redacted() -> Result<()> {
        let mut repository = registered(json!({}))?;
        repository.validate_base()?;
        let previous = repository.version();
        let error = repository.overlay("demo", json!({"count":0,"name":"secret-sentinel"}));
        assert!(error.is_err());
        let text = format!("{error:?}");
        assert!(!text.contains("secret-sentinel"));
        assert!(!text.contains("secret-validator-value"));
        assert_eq!(repository.version(), previous);
        assert_eq!(repository.section::<Settings>("demo")?, defaults());
        repository.overlay("demo", json!({"count":2}))?;
        Ok(())
    }

    #[test]
    fn closed_schema_rejects_unknown_but_named_map_allows_names_and_driver_options() -> Result<()> {
        let mut repository = registered(json!({"demo":{"typo":"secret-sentinel"}}))?;
        assert!(repository.validate_base().is_err());
        assert_eq!(repository.version(), 0);
        let mut dynamic = ConfigRepository::new(Figment::new());
        dynamic.register_section(
            TypeId::of::<Settings>(), "cache",
            json!({"stores":{"named_with_underscores":{"driver":"custom","options":{"token":"secret-sentinel"}}}}),
            SectionSchema::object([("stores", SectionSchema::map(SectionSchema::object([
                ("driver",SectionSchema::String), ("options",SectionSchema::any().sensitive()),
            ])))]), |_| Ok(()),
        )?;
        dynamic.validate_base()?;
        assert!(!format!("{dynamic:?}").contains("secret-sentinel"));
        Ok(())
    }

    #[test]
    fn registration_conflicts_check_owner_type_schema_and_defaults() -> Result<()> {
        let mut repository = registered(json!({}))?;
        assert!(
            repository
                .register_section(
                    TypeId::of::<Settings>(),
                    "demo",
                    defaults(),
                    schema(),
                    |_| Err(invalid("demo", "stricter"))
                )
                .is_err()
        );
        assert!(
            repository
                .register_section(
                    TypeId::of::<String>(),
                    "demo",
                    defaults(),
                    schema(),
                    validate
                )
                .is_err()
        );
        assert!(
            repository
                .register_section(
                    TypeId::of::<Settings>(),
                    "demo",
                    defaults(),
                    SectionSchema::Any,
                    validate
                )
                .is_err()
        );
        assert!(
            repository
                .register_section(
                    TypeId::of::<Settings>(),
                    "demo",
                    json!({}),
                    schema(),
                    |_| Ok(())
                )
                .is_err()
        );
        let mut changed = defaults();
        changed.name = "changed".into();
        assert!(
            repository
                .register_section(
                    TypeId::of::<Settings>(),
                    "demo",
                    changed,
                    schema(),
                    validate
                )
                .is_err()
        );
        assert!(
            repository
                .register_section(
                    TypeId::of::<Settings>(),
                    "demo.nested",
                    json!({}),
                    SectionSchema::Any,
                    |_| Ok(())
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn phases_refuse_early_overlay_late_registration_and_frozen_writes() -> Result<()> {
        let mut repository = registered(json!({}))?;
        assert!(repository.overlay("demo", json!({})).is_err());
        assert!(repository.freeze().is_err());
        repository.validate_base()?;
        assert!(repository.validate_base().is_err());
        assert!(
            repository
                .register_section(
                    TypeId::of::<Settings>(),
                    "other",
                    defaults(),
                    schema(),
                    validate
                )
                .is_err()
        );
        let version = repository.version();
        repository.freeze()?;
        repository.freeze()?;
        assert_eq!(repository.version(), version);
        assert!(repository.overlay("demo", json!({"name":"late"})).is_err());
        let mut clone = repository.clone();
        assert!(clone.overlay("demo", json!({})).is_err());
        Ok(())
    }

    #[test]
    fn selected_profile_and_global_values_are_overridden_only_at_overlay() -> Result<()> {
        let source = Figment::from(Serialized::default("demo.name", "source"))
            .merge(Serialized::globals(json!({"demo":{"count":20}})))
            .merge(Serialized::from(
                json!({"demo":{"name":"selected"}}),
                "test",
            ))
            .select("test");
        let mut repository = ConfigRepository::new(source);
        repository.register_section(
            TypeId::of::<Settings>(),
            "demo",
            defaults(),
            schema(),
            validate,
        )?;
        repository.validate_base()?;
        assert_eq!(repository.section::<Settings>("demo")?.count, 20);
        assert_eq!(repository.section::<Settings>("demo")?.name, "selected");
        repository.overlay("demo", json!({"count":30,"name":"module"}))?;
        assert_eq!(repository.section::<Settings>("demo")?.count, 30);
        assert_eq!(repository.section::<Settings>("demo")?.name, "module");
        Ok(())
    }

    #[test]
    fn recursive_replacement_preserves_siblings_and_replaces_types() -> Result<()> {
        let mut existing = json!({"nested":{"keep":1,"items":[{"a":1,"b":2},3]},"scalar":1});
        replace_recursive(
            &mut existing,
            json!({"nested":{"items":[{"a":9}]},"scalar":{"new":true}}),
            "section",
        )?;
        assert_eq!(
            existing,
            json!({"nested":{"keep":1,"items":[{"a":9,"b":2},3]},"scalar":{"new":true}})
        );
        Ok(())
    }

    #[test]
    fn typed_missing_field_has_schema_path_and_redacts_custom_errors() -> Result<()> {
        #[derive(Serialize, Deserialize)]
        struct Child {
            required: String,
        }
        #[derive(Serialize, Deserialize)]
        struct Parent {
            child: Child,
        }
        #[derive(Serialize)]
        struct Reject;
        impl<'de> Deserialize<'de> for Reject {
            fn deserialize<D: serde::Deserializer<'de>>(
                _: D,
            ) -> std::result::Result<Self, D::Error> {
                Err(serde::de::Error::missing_field(
                    "secret-deserializer-sentinel",
                ))
            }
        }
        let mut repository = ConfigRepository::new(Figment::from(Serialized::default(
            "parent",
            json!({"child":{}}),
        )));
        repository.register_section(
            TypeId::of::<Parent>(),
            "parent",
            json!({}),
            SectionSchema::object([(
                "child",
                SectionSchema::object([("required", SectionSchema::String)]),
            )]),
            |_| Ok(()),
        )?;
        let error = repository
            .section::<Parent>("parent")
            .err()
            .ok_or_else(|| invalid("test", "expected missing"))?;
        assert!(matches!(error, ConfigError::Source(ref source) if source.missing()));
        assert!(
            error.to_string().contains("parent.child.required"),
            "{error}"
        );
        let error = repository
            .section::<Reject>("parent")
            .err()
            .ok_or_else(|| invalid("test", "expected error"))?;
        assert!(!error.to_string().contains("secret-deserializer-sentinel"));
        Ok(())
    }

    #[test]
    fn overlay_preserves_unmodified_section_metadata() -> Result<()> {
        let source = Figment::from(Serialized::default("demo.name", "from-source"));
        let mut repository = ConfigRepository::new(source);
        repository.register_section(
            TypeId::of::<Settings>(),
            "demo",
            defaults(),
            schema(),
            validate,
        )?;
        repository.validate_base()?;
        let before = repository
            .snapshot()
            .find_metadata("demo.name")
            .map(|m| m.name.clone());
        repository.overlay("demo", json!({"count": 42}))?;
        assert_eq!(
            before,
            repository
                .snapshot()
                .find_metadata("demo.name")
                .map(|m| m.name.clone())
        );
        Ok(())
    }

    #[test]
    fn shallow_replacement_removes_default_nested_keys() -> Result<()> {
        let source = Figment::from(Serialized::default(
            "package",
            json!({"nested":{"existing":1}}),
        ));
        let mut repository = ConfigRepository::new(source);
        repository.merge_defaults("package", json!({"nested":{"default":2}}))?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("package")?,
            json!({"nested":{"existing":1}})
        );
        Ok(())
    }

    #[test]
    fn php_array_keys_support_mixed_and_noncanonical_numeric_strings() -> Result<()> {
        let mut value = json!({"0":"a", "01":"named", "name":"old"});
        merge_shallow(&mut value, json!(["b"]), "package");
        assert_eq!(value, json!({"0":"a", "1":"b", "01":"named", "name":"old"}));
        let mut value = json!(["a", "tail"]);
        replace_recursive(&mut value, json!({"0":"new", "name":"named"}), "package")?;
        assert_eq!(value, json!({"0":"new", "1":"tail", "name":"named"}));
        let mut value = json!({"name":"old"});
        replace_recursive(&mut value, json!([]), "package")?;
        assert_eq!(value, json!({"name":"old"}));
        Ok(())
    }

    #[test]
    fn schema_scalar_nullable_and_open_object_boundaries() -> Result<()> {
        SectionSchema::String
            .nullable()
            .validate(&Value::Null, "value")?;
        SectionSchema::object([("value", SectionSchema::Boolean)])
            .open()
            .validate(&json!({"value":true,"extra":1}), "section")?;
        assert!(
            SectionSchema::Integer
                .validate(&json!(1.5), "count")
                .is_err()
        );
        assert!(
            SectionSchema::array(SectionSchema::Boolean)
                .validate(&json!(["secret-sentinel"]), "items")
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn custom_deserialization_errors_never_escape() -> Result<()> {
        #[derive(Debug, Serialize)]
        struct Reject;
        impl<'de> Deserialize<'de> for Reject {
            fn deserialize<D: serde::Deserializer<'de>>(
                _: D,
            ) -> std::result::Result<Self, D::Error> {
                Err(serde::de::Error::custom("secret-deserializer-sentinel"))
            }
        }
        let repository = registered(json!({"demo":{"name":"secret-source-sentinel"}}))?;
        let result = repository.section::<Reject>("demo");
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains("secret-deserializer-sentinel"));
        Ok(())
    }
    #[test]
    fn typed_errors_preserve_missing_and_nested_path_without_values() -> Result<()> {
        let repository = registered(json!({"demo":{"name":"source","count":"secret-sentinel"}}))?;
        let missing = repository.section::<Settings>("absent");
        assert!(matches!(missing, Err(ConfigError::Source(ref error)) if error.missing()));
        let wrong = repository.section::<Settings>("demo");
        assert!(matches!(wrong, Err(ConfigError::Source(ref error)) if !error.missing()));
        let text = format!("{wrong:?}");
        assert!(text.contains("demo.count"));
        assert!(!text.contains("secret-sentinel"));
        Ok(())
    }
    #[test]
    fn validator_can_read_repository_without_holding_its_lock() -> Result<()> {
        static OBSERVER: std::sync::OnceLock<ConfigRepository> = std::sync::OnceLock::new();
        fn reentrant(_: &Settings) -> Result<()> {
            if let Some(observer) = OBSERVER.get() {
                let _ = observer.version();
                assert!(observer.section::<bool>("custom.enabled")?);
            }
            Ok(())
        }
        let mut repository = ConfigRepository::new(Figment::from(Serialized::defaults(
            json!({"custom":{"enabled":true}}),
        )));
        repository.register_section(
            TypeId::of::<Settings>(),
            "demo",
            defaults(),
            schema(),
            reentrant,
        )?;
        assert!(OBSERVER.set(repository.clone()).is_ok());
        repository.validate_base()?;
        repository.overlay("demo", json!({"name":"module"}))?;
        repository.freeze()?;
        Ok(())
    }

    #[test]
    fn overlay_schema_failure_keeps_prior_version_and_source_metadata() -> Result<()> {
        let mut repository = registered(json!({"untouched":{"key":"source"}}))?;
        repository.validate_base()?;
        let before = repository.snapshot();
        let version = repository.version();
        assert!(
            repository
                .overlay("demo", json!({"items":[false]}))
                .is_err()
        );
        assert_eq!(repository.version(), version);
        repository.overlay("demo", json!({"count":11}))?;
        let after = repository.snapshot();
        assert_eq!(before.profile(), after.profile());
        assert_eq!(
            before.find_metadata("untouched.key").map(|m| &m.name),
            after.find_metadata("untouched.key").map(|m| &m.name)
        );
        assert_eq!(repository.section::<String>("untouched.key")?, "source");
        Ok(())
    }
    #[test]
    fn shallow_package_defaults_keep_existing_nested_object_whole() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::from(Serialized::defaults(
            json!({"package":{"nested":{"existing":1},"name":"environment"}}),
        )));
        repository.merge_defaults(
            "package",
            json!({"nested":{"default":2},"name":"default","extra":true}),
        )?;
        assert!(repository.section::<bool>("package.extra").is_err());
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("package")?,
            json!({"nested":{"existing":1},"name":"environment","extra":true})
        );
        Ok(())
    }

    #[test]
    fn recursive_package_defaults_keep_existing_values_and_default_array_tail() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::from(Serialized::defaults(
            json!({"package":{"nested":{"existing":1},"items":["source"]}}),
        )));
        repository.replace_defaults_recursively(
            "package",
            json!({"nested":{"default":2},"items":["default","tail"]}),
        )?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("package")?,
            json!({"nested":{"existing":1,"default":2},"items":["source","tail"]})
        );
        Ok(())
    }

    #[test]
    fn shallow_package_arrays_concatenate_defaults_then_current() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::from(Serialized::default(
            "items",
            json!(["current"]),
        )));
        repository.merge_defaults("items", json!(["default"]))?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("items")?,
            json!(["default", "current"])
        );
        Ok(())
    }

    #[test]
    fn package_operation_order_and_register_phase_are_explicit() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        repository.merge_defaults("items", json!(["first"]))?;
        repository.merge_defaults("items", json!(["second"]))?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("items")?,
            json!(["second", "first"])
        );
        assert!(repository.merge_defaults("items", json!([])).is_err());
        assert!(
            repository
                .replace_defaults_recursively("items", json!([]))
                .is_err()
        );
        repository.freeze()?;
        assert!(repository.merge_defaults("items", json!([])).is_err());
        Ok(())
    }

    #[test]
    fn invalid_package_and_mixed_array_representations_are_safe_and_atomic() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        assert!(
            repository
                .merge_defaults("package", json!("secret-sentinel"))
                .is_err()
        );
        assert!(
            repository
                .replace_defaults_recursively("package", Value::Null)
                .is_err()
        );
        repository.merge_defaults("package", json!([]))?;
        repository.replace_defaults_recursively("package", json!({"key":true}))?;
        repository.validate_base()?;
        assert_eq!(repository.section::<Value>("package")?, json!({"key":true}));
        let mut repository = registered(json!({}))?;
        repository.validate_base()?;
        let version = repository.version();
        assert!(
            repository
                .overlay("demo", json!({"items":{"key":"secret-sentinel"}}))
                .is_err()
        );
        assert_eq!(repository.version(), version);
        assert_eq!(repository.section::<Settings>("demo")?, defaults());
        Ok(())
    }

    #[test]
    fn package_operations_use_existing_profile_values_and_typed_defaults_first() -> Result<()> {
        let mut repository = registered(json!({"demo":{"name":"source"}}))?;
        repository.merge_defaults("demo", json!({"name":"package","count":99}))?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Settings>("demo")?,
            Settings {
                name: "source".into(),
                ..defaults()
            }
        );
        Ok(())
    }
}

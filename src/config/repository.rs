//! Provider 共享的版本化配置仓储；区段默认补齐不替代包配置的专用合并操作。

use std::any::TypeId;
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use figment::{
    Figment, Metadata, Profile, Provider,
    providers::Serialized,
    value::{Dict, Map, Tag, Value as FigmentValue},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::config::code::TypedValue;
use crate::config::code::{ConfigContext, ConfigManifest, EvaluatedEntry, ReadView};
use crate::config::sources::{merge_order, mergeable_options};
use crate::config::sources::{paths_overlap, raw_path_conflicts};
use crate::config::{ConfigError, ConfigSourceError, ConfigSources};
use crate::core::{Error, Result};
use crate::support::Collection;

macro_rules! typed_reader {
    ($name:ident, $or:ident, $or_else:ident, $output:ty, $convert:ident, $description:literal) => {
        #[doc = $description]
        pub fn $name(&self, key: &str) -> std::result::Result<$output, ConfigError> {
            $convert(key, self.typed_value_or_else(key, || Value::Null)?)
        }

        #[doc = concat!($description, " 仅当键缺失时使用给定默认值。")]
        pub fn $or(&self, key: &str, default: Value) -> std::result::Result<$output, ConfigError> {
            self.$or_else(key, || default)
        }

        #[doc = concat!($description, " 仅当键缺失时调用一次默认值闭包。")]
        pub fn $or_else(
            &self,
            key: &str,
            default: impl FnOnce() -> Value,
        ) -> std::result::Result<$output, ConfigError> {
            $convert(key, self.typed_value_or_else(key, default)?)
        }
    };
}

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
                    if let Some(schema) = fields.get(key) {
                        schema.validate(item, &format!("{path}.{key}"))?;
                    } else if !allow_unknown {
                        return Err(invalid(path, "unknown field"));
                    }
                }
                true
            }
            Self::Map(schema) => {
                let Some(object) = value.as_object() else {
                    return Err(invalid(path, "expected named map"));
                };
                for item in object.values() {
                    schema.validate(item, &format!("{path}.<entry>"))?;
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
    owner: TypeId,
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
struct State {
    origin: Figment,
    figment: Figment,
    raw: Option<Value>,
    file_domains: HashSet<String>,
    pre_file_base: Option<Value>,
    after_file_layers: Vec<Value>,
    cached: bool,
    typed: BTreeMap<String, TypedValue>,
    code_context: Option<ConfigContext>,
    register_effects: Vec<RegisterEffect>,
    sections: BTreeMap<&'static str, Section>,
    phase: Phase,
    version: u64,
    revision: u64,
}

#[derive(Clone)]
enum RegisterEffect {
    Write(String, Value),
    // 精确键的数字项遵循 array_merge，不能把发布后的数组当作 set 覆盖晚默认值。
    MergeNumbers { key: String, defaults: Value },
}

impl State {
    // 只恢复先前成功发布的实际变化；未改动的文件值仍由 late base 规则决定。
    fn replay_section_effects(
        &self,
        key: &str,
        candidate: &mut Figment,
        order: &mut Value,
    ) -> Result<()> {
        for effect in &self.register_effects {
            match effect {
                RegisterEffect::Write(path, value) => {
                    let write = if path == key
                        || path
                            .strip_prefix(key)
                            .is_some_and(|suffix| suffix.starts_with('.'))
                    {
                        Some((path.clone(), value.clone()))
                    } else {
                        key.strip_prefix(path)
                            .and_then(|suffix| suffix.strip_prefix('.'))
                            .and_then(|relative| read_dot_value(value, relative))
                            .map(|value| (key.to_owned(), value.clone()))
                    };
                    if let Some(write) = write {
                        let (updated, raw) =
                            apply_set_entries(candidate.clone(), Some(order.clone()), &[write])?;
                        *candidate = updated;
                        *order = raw;
                    }
                }
                RegisterEffect::MergeNumbers {
                    key: path,
                    defaults,
                } if path == key
                    || path
                        .strip_prefix(key)
                        .is_some_and(|suffix| suffix.starts_with('.')) =>
                {
                    let (updated, merged) = preview_register_operation(
                        candidate,
                        Some(order),
                        path,
                        defaults.clone(),
                        false,
                    )?;
                    *candidate = updated;
                    set_dot_value(order, path, merged);
                }
                RegisterEffect::MergeNumbers { .. } => {}
            }
        }
        Ok(())
    }

    fn frozen_section_candidate(
        &self,
        key: &str,
        defaults: Value,
    ) -> Result<(Figment, Option<Value>, Value)> {
        let mut candidate = self.figment.clone();
        // 缓存是已求值配置；晚注册仅安装校验，不重新播放默认值。
        if self.cached {
            let current = candidate
                .extract_inner::<Value>(key)
                .map_err(|error| Error::Config(safe_source_error(error, key)))?;
            return Ok((candidate, self.raw.clone(), current));
        }
        if let Some(domain) = self.containing_file_domain(key) {
            let defaults = if domain == key {
                defaults
            } else {
                let mut parent = Value::Object(serde_json::Map::new());
                set_dot_value(&mut parent, &key[domain.len() + 1..], defaults);
                parent
            };
            let current = candidate
                .extract_inner::<Value>(domain)
                .map_err(|error| Error::Config(safe_source_error(error, domain)))?;
            let (combined, _) = file_default_patch(domain, &defaults, &current);
            let original = candidate.find_value(domain).ok();
            candidate = replace_section(&candidate, domain, combined.clone(), original.as_ref())?;
            let mut raw = self.raw.clone();
            if let Some(raw) = &mut raw {
                set_dot_value(raw, domain, combined);
            }
            let merged = candidate
                .extract_inner::<Value>(key)
                .map_err(|error| Error::Config(safe_source_error(error, key)))?;
            return Ok((candidate, raw, merged));
        }
        let current = match candidate.extract_inner::<Value>(key) {
            Ok(value) => Some(value),
            Err(error) if error.missing() => None,
            Err(error) => return Err(Error::Config(safe_source_error(error, key))),
        };
        let mut merged = defaults.clone();
        if let Some(current) = current {
            let prior = self.raw.as_ref().and_then(|raw| read_dot_value(raw, key));
            let current = prior.map_or(current.clone(), |raw| reorder_like(raw, current));
            merge_order(&mut merged, current.clone());
            self.restore_descendant_file_defaults(key, &defaults, &current, &mut merged);
            if let Some(prior) = prior {
                merged = reorder_like(prior, merged);
            }
        }
        let original = candidate.find_value(key).ok();
        candidate = replace_section(&candidate, key, merged.clone(), original.as_ref())?;
        let mut raw = self.raw.clone();
        if let Some(raw) = &mut raw {
            set_dot_value(raw, key, merged.clone());
        }
        Ok((candidate, raw, merged))
    }

    fn restore_descendant_file_defaults(
        &self,
        key: &str,
        defaults: &Value,
        current: &Value,
        merged: &mut Value,
    ) {
        let mut domains = self
            .file_domains
            .iter()
            .filter_map(|domain| {
                domain
                    .strip_prefix(key)
                    .and_then(|suffix| suffix.strip_prefix('.'))
                    .map(|relative| (domain.as_str(), relative))
            })
            .collect::<Vec<_>>();
        domains.sort_by_key(|(domain, _)| domain.len());
        for (domain, relative) in domains {
            // 原始当前值保留文件条目的浅覆盖；不能读取已递归补默认后的污染值。
            if let (Some(defaults), Some(current)) = (
                read_dot_value(defaults, relative),
                read_dot_value(current, relative),
            ) {
                let (combined, _) = file_default_patch(domain, defaults, current);
                set_dot_value(merged, relative, combined);
            }
        }
    }

    fn containing_file_domain(&self, key: &str) -> Option<&str> {
        self.file_domains
            .iter()
            .filter(|path| {
                key.strip_prefix(path.as_str())
                    .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('.'))
            })
            .max_by_key(|path| path.len())
            .map(String::as_str)
    }

    // 注册时立即发布给下一个 Provider，同时保持递归文件的 Laravel 浅合并边界。
    fn section_candidate(
        &self,
        key: &str,
        defaults: Value,
    ) -> Result<(Figment, Option<Value>, Value)> {
        let mut candidate = self.figment.clone();
        if self.phase == Phase::Register && self.cached {
            return Ok((candidate, self.raw.clone(), defaults));
        }
        if self.phase == Phase::Frozen {
            return self.frozen_section_candidate(key, defaults);
        }

        let mut order = match &self.raw {
            Some(raw) => raw.clone(),
            None => candidate
                .extract::<Value>()
                .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?,
        };
        if let Some(domain) = self.containing_file_domain(key) {
            // 子区段的默认值仍属于父文件；从文件边界合并才能保留显式条目的浅替换。
            let defaults = if domain == key {
                defaults
            } else {
                let mut parent_defaults = Value::Object(serde_json::Map::new());
                set_dot_value(&mut parent_defaults, &key[domain.len() + 1..], defaults);
                parent_defaults
            };
            let key = domain;
            let current = read_dot_value(&order, key).cloned().unwrap_or(Value::Null);
            let (combined, patch) = file_default_patch(key, &defaults, &current);
            if current.is_array() && defaults.is_array() {
                let original = candidate.find_value(key).ok();
                candidate = replace_section(&candidate, key, combined.clone(), original.as_ref())?;
            } else if patch.as_object().is_some_and(|entries| !entries.is_empty()) {
                candidate = candidate.join(Serialized::default(key, &patch));
            }
            for option in mergeable_options(key) {
                if defaults.get(*option).is_some_and(is_php_array)
                    && current.get(*option).is_some_and(is_php_array)
                {
                    let option_path = format!("{key}.{option}");
                    let value = combined
                        .get(*option)
                        .cloned()
                        .ok_or_else(|| invalid(&option_path, "merged option missing"))?;
                    let original = candidate.find_value(&option_path).ok();
                    candidate =
                        replace_section(&candidate, &option_path, value, original.as_ref())?;
                }
            }
            set_dot_value(&mut order, key, combined);
        } else if self.file_domains.iter().any(|path| {
            path.strip_prefix(key)
                .is_some_and(|suffix| suffix.starts_with('.'))
        }) {
            let base = self
                .pre_file_base
                .as_ref()
                .ok_or_else(|| invalid(key, "file base missing"))?;
            let defaults = Figment::from(Serialized::defaults(base.clone()))
                .join(Serialized::default(key, &defaults));
            let section_base = defaults
                .extract_inner::<Value>(key)
                .map_err(|error| Error::Config(safe_source_error(error, key)))?;
            let mut replay = base.clone();
            set_dot_value(&mut replay, key, section_base);
            for layer in &self.after_file_layers {
                merge_order(&mut replay, layer.clone());
            }
            let published = read_dot_value(&replay, key)
                .cloned()
                .ok_or_else(|| invalid(key, "base root missing"))?;
            set_dot_value(&mut order, key, published.clone());
            let original = candidate.find_value(key).ok();
            candidate = replace_section(&candidate, key, published, original.as_ref())?;
            self.replay_section_effects(key, &mut candidate, &mut order)?;
        } else {
            candidate = candidate.join(Serialized::default(key, &defaults));
            let mut current = read_dot_value(&order, key).cloned().unwrap_or(Value::Null);
            merge_order(&mut current, defaults);
            set_dot_value(&mut order, key, current);
        }
        let extracted = candidate
            .extract::<Value>()
            .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?;
        let raw = reorder_like(&order, extracted);
        for path in &self.file_domains {
            if read_dot_value(&raw, path) != read_dot_value(&order, path) {
                return Err(invalid(
                    path,
                    &format!(
                        "file configuration projection differs from raw values after registering {key}"
                    ),
                ));
            }
        }
        Ok((candidate, Some(raw), Value::Null))
    }
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
    /// 本次启动是否从已编译缓存恢复；模块配置文件在此状态下不得重读。
    pub fn is_cached(&self) -> bool {
        self.lock().cached
    }

    /// 保留现有来源及其优先级，开始 Provider 注册阶段。
    pub fn new(sources: impl Into<ConfigSources>) -> Self {
        let (
            figment,
            raw,
            file_domains,
            pre_file_base,
            after_file_layers,
            cached,
            typed,
            code_context,
        ) = sources.into().into_parts();
        Self {
            inner: Arc::new(Mutex::new(State {
                origin: figment.clone(),
                figment,
                raw,
                file_domains,
                pre_file_base,
                after_file_layers,
                cached,
                typed,
                code_context,
                register_effects: Vec::new(),
                sections: BTreeMap::new(),
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
        match state.origin.find_value(key) {
            Ok(_) => Ok(true),
            Err(error) if error.missing() => Ok(false),
            Err(error) => Err(safe_source_error(error, key)),
        }
    }

    /// 登记类型化默认值、结构与校验器；文件内区段沿所属文件的浅合并边界，
    /// 仅有子文件时按 Laravel 剩余 base 根写入覆盖，文件后来源仍优先。
    ///
    /// 同名登记一律拒绝，避免忽略后来登记的校验器。
    /// 本操作仅补齐区段默认值，不代表 Laravel 包的浅合并或递归替换操作。
    pub fn register_section<T>(
        &mut self,
        owner: TypeId,
        key: &'static str,
        defaults: T,
        schema: SectionSchema,
        validate: fn(&T) -> Result<()>,
    ) -> Result<()>
    where
        T: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        require_phase(self.lock().phase, Phase::Register)?;
        self.register_section_for(owner, false, key, defaults, schema, validate)
    }

    /// Provider 专用登记；仅显式 force 可替换自身区段。
    pub(crate) fn register_section_for<T>(
        &mut self,
        owner: TypeId,
        force: bool,
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
        let snapshot = {
            let state = self.lock();
            if state.phase == Phase::Boot {
                return Err(invalid(
                    "configuration",
                    "configuration phase forbids mutation",
                ));
            }
            if let Some(existing) = state.sections.get(key)
                && (existing.owner != owner || !force)
            {
                return Err(invalid(key, "section registration conflict"));
            }
            if state.sections.keys().any(|other| {
                key.starts_with(&format!("{other}.")) || other.starts_with(&format!("{key}."))
            }) {
                return Err(invalid(key, "overlapping section registration"));
            }
            state.clone()
        };
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
        let replacement = Section {
            owner,
            schema,
            validate: validator,
        };
        let (candidate, raw, merged) = snapshot.section_candidate(key, defaults)?;
        if snapshot.phase == Phase::Frozen {
            replacement.schema.validate(&merged, key)?;
            (replacement.validate)(&candidate)?;
        }
        let mut state = self.lock();
        if state.phase != snapshot.phase || state.revision != snapshot.revision {
            return Err(invalid("configuration", "concurrent configuration update"));
        }
        let version = if snapshot.phase == Phase::Frozen {
            state
                .version
                .checked_add(1)
                .ok_or_else(|| invalid(key, "version exhausted"))?
        } else {
            state.version
        };
        state.figment = candidate;
        state.raw = raw;
        state.version = version;
        state.sections.insert(key, replacement);
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    /// 登记 Laravel 包的浅层默认合并；已有对象键整体获胜，数组连接并重新编号。
    ///
    /// Register 阶段立即发布，`validate_base` 只校验已发布视图。
    pub fn merge_defaults(&mut self, key: &'static str, defaults: Value) -> Result<()> {
        self.apply_register_operation(key, defaults, false)
    }

    /// Provider 注册时应用浅层默认合并；Frozen 阶段逐项发布。
    pub(crate) fn merge_defaults_for(&mut self, key: &'static str, defaults: Value) -> Result<()> {
        self.apply_operation(key, defaults, false)
    }

    /// 登记包的递归默认合并；已有值胜，数组按索引替换并保留默认尾部。
    ///
    /// 与 boot 模块覆盖的方向相反；Register 阶段立即发布。
    pub fn replace_defaults_recursively(
        &mut self,
        key: &'static str,
        defaults: Value,
    ) -> Result<()> {
        self.apply_register_operation(key, defaults, true)
    }

    /// Provider 注册时应用递归默认合并；Frozen 阶段逐项发布。
    pub(crate) fn replace_defaults_recursively_for(
        &mut self,
        key: &'static str,
        defaults: Value,
    ) -> Result<()> {
        self.apply_operation(key, defaults, true)
    }

    fn apply_operation(&self, key: &'static str, defaults: Value, recursive: bool) -> Result<()> {
        if self.lock().phase == Phase::Register {
            return self.apply_register_operation(key, defaults, recursive);
        }
        validate_key(key)?;
        if !defaults.is_object() && !defaults.is_array() {
            return Err(invalid(key, "package defaults must be object or array"));
        }
        let (figment, raw, sections, revision) = {
            let state = self.lock();
            require_phase(state.phase, Phase::Frozen)?;
            (
                state.figment.clone(),
                state.raw.clone(),
                state.sections.clone(),
                state.revision,
            )
        };
        let current = match figment.extract_inner::<Value>(key) {
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
        let current = match raw.as_ref().and_then(|order| read_dot_value(order, key)) {
            Some(prior) => reorder_like(prior, current),
            None => current,
        };
        let mut merged = defaults;
        if recursive {
            replace_recursive(&mut merged, current, key)?;
        } else {
            merge_shallow(&mut merged, current, key);
        }
        let original = figment.find_value(key).ok();
        let candidate = replace_section(&figment, key, merged.clone(), original.as_ref())?;
        validate_candidate(&candidate, &sections)?;
        let mut raw = raw;
        if let Some(raw) = &mut raw {
            set_dot_value(raw, key, merged);
        }
        let mut state = self.lock();
        require_phase(state.phase, Phase::Frozen)?;
        if state.revision != revision {
            return Err(invalid("configuration", "concurrent configuration update"));
        }
        let version = state
            .version
            .checked_add(1)
            .ok_or_else(|| invalid(key, "version exhausted"))?;
        state.figment = candidate;
        state.raw = raw;
        state.version = version;
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    fn apply_register_operation(
        &self,
        key: &'static str,
        defaults: Value,
        recursive: bool,
    ) -> Result<()> {
        validate_key(key)?;
        if !defaults.is_object() && !defaults.is_array() {
            return Err(invalid(key, "package defaults must be object or array"));
        }
        let mut state = self.lock();
        require_phase(state.phase, Phase::Register)?;
        if state.cached {
            return Ok(());
        }
        let before = state
            .raw
            .as_ref()
            .and_then(|raw| read_dot_value(raw, key))
            .cloned()
            .or_else(|| state.figment.extract_inner::<Value>(key).ok());
        let numeric_defaults = if recursive {
            None
        } else {
            numeric_merge_defaults(&defaults)
        };
        let (candidate, preview) = preview_register_operation(
            &state.figment,
            state.raw.as_ref(),
            key,
            defaults,
            recursive,
        )?;
        let mut effects = Vec::new();
        if let Some(defaults) = numeric_defaults.as_ref() {
            effects.push(RegisterEffect::MergeNumbers {
                key: key.to_owned(),
                defaults: defaults.clone(),
            });
        }
        if !preview.is_array() || numeric_defaults.is_none() {
            collect_register_effects(
                before.as_ref(),
                &preview,
                key,
                numeric_defaults.is_some(),
                &mut effects,
            );
        }
        state.figment = candidate;
        if let Some(raw) = &mut state.raw {
            set_dot_value(raw, key, preview);
        }
        state.register_effects.extend(effects);
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    /// 校验已按注册顺序发布的视图；成功后封闭注册并进入 boot。
    pub fn validate_base(&mut self) -> Result<()> {
        let (candidate, raw, file_domains, sections, revision) = {
            let state = self.lock();
            require_phase(state.phase, Phase::Register)?;
            (
                state.figment.clone(),
                state.raw.clone(),
                state.file_domains.clone(),
                state.sections.clone(),
                state.revision,
            )
        };
        validate_candidate(&candidate, &sections)?;
        let order = match raw {
            Some(raw) => raw,
            None => candidate
                .extract::<Value>()
                .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?,
        };
        let extracted = candidate
            .extract::<Value>()
            .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?;
        let candidate_raw = reorder_like(&order, extracted);
        for key in &file_domains {
            if read_dot_value(&candidate_raw, key) != read_dot_value(&order, key) {
                return Err(invalid(
                    key,
                    "file configuration projection differs from raw values",
                ));
            }
        }
        self.publish(
            candidate,
            Some(candidate_raw),
            revision,
            (Phase::Register, Phase::Boot),
            true,
            Vec::new(),
        )
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

    /// 从当前快照读取原始值；缺失为 `None`，显式 null 为 `Some(Value::Null)`。
    ///
    /// 完整字面键优先于点路径；返回值可能包含秘密，输出前须脱敏。
    pub fn get(&self, key: &str) -> std::result::Result<Option<Value>, ConfigError> {
        Ok(read_value(&self.all()?, key).cloned())
    }

    /// 仅在键缺失时返回默认值；null、false、零及空值均保留。
    pub fn get_or(&self, key: &str, default: Value) -> std::result::Result<Value, ConfigError> {
        Ok(self.get(key)?.unwrap_or(default))
    }

    fn typed_value_or_else(
        &self,
        key: &str,
        default: impl FnOnce() -> Value,
    ) -> std::result::Result<Value, ConfigError> {
        Ok(self.get(key)?.unwrap_or_else(default))
    }

    typed_reader!(
        string,
        string_or,
        string_or_else,
        String,
        config_string,
        "严格读取字符串配置。"
    );
    typed_reader!(
        integer,
        integer_or,
        integer_or_else,
        i64,
        config_integer,
        "严格读取 i64 整数配置。"
    );
    typed_reader!(
        float,
        float_or,
        float_or_else,
        f64,
        config_float,
        "严格读取浮点配置；整数不隐式转换。"
    );
    typed_reader!(
        boolean,
        boolean_or,
        boolean_or_else,
        bool,
        config_boolean,
        "严格读取布尔配置。"
    );
    typed_reader!(
        array,
        array_or,
        array_or_else,
        Value,
        config_array,
        "严格读取数组配置，保留有序列表或混合键对象。"
    );
    typed_reader!(
        collection,
        collection_or,
        collection_or_else,
        Collection<Value>,
        config_collection,
        "严格读取数组配置并转换为保键集合。"
    );

    /// 判断完整字面键或点路径是否存在；显式 null 仍算存在。
    pub fn has(&self, key: &str) -> std::result::Result<bool, ConfigError> {
        Ok(read_value(&self.all()?, key).is_some())
    }

    /// 在同一快照批量读取；缺失为 null，保留首次键顺序，重复键取最后结果。
    pub fn get_many(
        &self,
        keys: &[&str],
    ) -> std::result::Result<Vec<(String, Value)>, ConfigError> {
        let values = self.all()?;
        let mut result = Vec::with_capacity(keys.len());
        for key in keys {
            insert_read(
                &mut result,
                key,
                read_value(&values, key).cloned().unwrap_or(Value::Null),
            );
        }
        Ok(result)
    }

    /// 在同一快照批量读取并按键提供默认值；显式 null 不使用默认值。
    ///
    /// 结果保留首次键顺序，重复键以后一次读取结果覆盖。
    pub fn get_many_with_defaults(
        &self,
        keys: &[(&str, Value)],
    ) -> std::result::Result<Vec<(String, Value)>, ConfigError> {
        let values = self.all()?;
        let mut result = Vec::with_capacity(keys.len());
        for (key, default) in keys {
            insert_read(
                &mut result,
                key,
                read_value(&values, key).unwrap_or(default).clone(),
            );
        }
        Ok(result)
    }

    /// 返回当前已发布的独立原始值快照；包含秘密，输出前须脱敏。
    ///
    /// 来源或反序列化失败时整体返回安全错误，不发布部分结果。
    pub fn all(&self) -> std::result::Result<Value, ConfigError> {
        let state = self.lock();
        if let Some(raw) = &state.raw {
            return Ok(raw.clone());
        }
        let figment = state.figment.clone();
        drop(state);
        figment
            .extract()
            .map_err(|error| safe_source_error(error, "configuration"))
    }

    /// 读取同一配置仓储中已编译的 Rust 类型或工厂；原始值视图不包含此值。
    pub fn typed<T: Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> std::result::Result<Option<Arc<T>>, ConfigError> {
        let value = self.lock().typed.get(key).cloned();
        value
            .map(|value| {
                Arc::downcast::<T>(value).map_err(|_| {
                    ConfigError::Invalid("代码配置类型不匹配（源值未公开）".to_owned())
                })
            })
            .transpose()
    }

    /// 返回独立的来源快照；包含秘密值，调用方必须执行适用的输出脱敏。
    pub fn snapshot(&self) -> Figment {
        self.lock().figment.clone()
    }

    /// 已发布配置版本；注册元数据和 freeze 不改变配置版本。
    pub fn version(&self) -> u64 {
        self.lock().version
    }

    /// 按 Laravel `Arr::set` 的点路径写入一项原始配置。
    ///
    /// 可信应用代码可在注册、启动及运行期调用；运行期不会重新校验启动 schema，
    /// 已解析的服务也不会自动重建。失败时原始值、类型化投影及版本均不改变。
    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        self.set_many([(key, value)])
    }

    /// 按迭代顺序写入多项配置，并以一个版本原子发布。
    ///
    /// 空批次不改变版本；重复键保留原位置并以最后写入值为准。
    pub fn set_many<K>(&self, entries: impl IntoIterator<Item = (K, Value)>) -> Result<()>
    where
        K: Into<String>,
    {
        let entries: Vec<(String, Value)> = entries
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect();
        if entries.is_empty() {
            return Ok(());
        }
        let (base, raw, revision, phase) = {
            let state = self.lock();
            (
                state.figment.clone(),
                state.raw.clone(),
                state.revision,
                state.phase,
            )
        };
        let (candidate, raw) = apply_set_entries(base, raw, &entries)?;
        self.publish(
            candidate,
            Some(raw),
            revision,
            (phase, phase),
            true,
            entries,
        )
    }

    /// 在 boot 时点递归覆盖模块区段；数组按索引替换并保留旧尾。
    ///
    /// 校验整个已注册配置集合后才发布；错误不改变旧视图或版本。
    pub fn overlay(&mut self, section: &str, value: Value) -> Result<()> {
        validate_key(section)?;
        let (base, raw, sections, revision) = {
            let state = self.lock();
            require_phase(state.phase, Phase::Boot)?;
            (
                state.figment.clone(),
                state.raw.clone(),
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
        let mut raw = match raw {
            Some(raw) => raw,
            None => base
                .extract::<Value>()
                .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?,
        };
        let mut raw_existing = read_dot_value(&raw, section)
            .cloned()
            .unwrap_or(Value::Null);
        replace_recursive(&mut raw_existing, value.clone(), section)?;
        replace_recursive(&mut existing, value, section)?;
        let candidate = replace_section(&base, section, existing, original.as_ref())?;
        validate_candidate(&candidate, &sections)?;
        set_dot_value(&mut raw, section, raw_existing);
        self.publish(
            candidate,
            Some(raw),
            revision,
            (Phase::Boot, Phase::Boot),
            true,
            Vec::new(),
        )
    }

    /// 在 Provider boot 时用同次环境快照原子应用模块代码配置。
    ///
    /// 已编译缓存跳过可序列化函数；含 Rust 类型的模块清单拒绝缓存。
    #[allow(
        clippy::too_many_lines,
        reason = "保留单一快照、依赖候选、schema及CAS发布的原子边界"
    )]
    pub fn apply_code_manifest(
        &mut self,
        manifest: &ConfigManifest,
        console_command: Option<&str>,
    ) -> Result<()> {
        if manifest.includes_toml() {
            return Err(invalid(
                "configuration",
                "module TOML source requires ModuleConfigFiles",
            ));
        }
        let (context, cached, revision, sections, mut candidate, mut raw, mut typed) = {
            let state = self.lock();
            require_phase(state.phase, Phase::Boot)?;
            (
                state.code_context.clone(),
                state.cached,
                state.revision,
                state.sections.clone(),
                state.figment.clone(),
                state.raw.clone(),
                state.typed.clone(),
            )
        };
        if manifest.has_typed() && (cached || console_command == Some("config:cache")) {
            return Err(invalid(
                "configuration",
                "runtime Rust configuration cannot use config cache",
            ));
        }
        if cached {
            return Ok(());
        }
        let context = context
            .ok_or_else(|| invalid("configuration", "missing code configuration environment"))?;
        let mut raw = raw
            .take()
            .ok_or_else(|| invalid("configuration", "missing raw configuration"))?;
        manifest.validate().map_err(Error::from)?;
        if manifest
            .typed_keys()
            .any(|key| typed.keys().any(|existing| paths_overlap(existing, key)))
        {
            return Err(invalid(
                "configuration",
                "typed configuration paths overlap",
            ));
        }
        let initial = ReadView {
            raw: raw.clone(),
            typed: typed.keys().cloned().collect(),
        };
        let evaluated = manifest
            .evaluate_with(&context, initial, |entry| {
                match entry {
                    EvaluatedEntry::Section(key, value) => {
                        let mut existing = match candidate.extract_inner::<Value>(&key) {
                            Ok(value) => value,
                            Err(error) if error.missing() => Value::Null,
                            Err(error) => return Err(safe_source_error(error, &key)),
                        };
                        let mut raw_existing =
                            read_dot_value(&raw, &key).cloned().unwrap_or(Value::Null);
                        replace_recursive(&mut existing, value.clone(), &key)
                            .map_err(code_merge_error)?;
                        replace_recursive(&mut raw_existing, value, &key)
                            .map_err(code_merge_error)?;
                        let original = candidate.find_value(&key).ok();
                        candidate = replace_section(&candidate, &key, existing, original.as_ref())
                            .map_err(code_merge_error)?;
                        set_dot_value(&mut raw, &key, raw_existing);
                    }
                    EvaluatedEntry::Typed(key, value) => {
                        if typed.keys().any(|existing| paths_overlap(existing, &key)) {
                            return Err(ConfigError::Invalid(
                                "代码配置类型路径重复（源值未公开）".to_owned(),
                            ));
                        }
                        typed.insert(key, value);
                    }
                }
                if typed.keys().any(|key| raw_path_conflicts(&raw, key)) {
                    return Err(ConfigError::Invalid(
                        "代码配置类型路径与原始值冲突（源值未公开）".to_owned(),
                    ));
                }
                Ok(ReadView {
                    raw: raw.clone(),
                    typed: typed.keys().cloned().collect(),
                })
            })
            .map_err(Error::from)?;
        if evaluated == 0 {
            return Ok(());
        }
        validate_candidate(&candidate, &sections)?;
        let mut state = self.lock();
        require_phase(state.phase, Phase::Boot)?;
        if state.revision != revision {
            return Err(invalid("configuration", "concurrent configuration update"));
        }
        state.version = state
            .version
            .checked_add(1)
            .ok_or_else(|| invalid("configuration", "version exhausted"))?;
        state.figment = candidate;
        state.raw = Some(raw);
        state.typed = typed;
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    /// 最终校验并冻结；重复冻结幂等，之后拒绝注册或覆盖。
    pub fn freeze(&mut self) -> Result<()> {
        let (candidate, raw, sections, revision) = {
            let state = self.lock();
            if state.phase == Phase::Frozen {
                return Ok(());
            }
            require_phase(state.phase, Phase::Boot)?;
            (
                state.figment.clone(),
                state.raw.clone(),
                state.sections.clone(),
                state.revision,
            )
        };
        validate_candidate(&candidate, &sections)?;
        self.publish(
            candidate,
            raw,
            revision,
            (Phase::Boot, Phase::Frozen),
            false,
            Vec::new(),
        )
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn publish(
        &self,
        candidate: Figment,
        raw: Option<Value>,
        revision: u64,
        transition: (Phase, Phase),
        changed: bool,
        register_writes: Vec<(String, Value)>,
    ) -> Result<()> {
        let (before, after) = transition;
        let mut state = self.lock();
        require_phase(state.phase, before)?;
        if state.revision != revision {
            return Err(invalid("configuration", "concurrent configuration update"));
        }
        if raw
            .as_ref()
            .is_some_and(|raw| state.typed.keys().any(|key| raw_path_conflicts(raw, key)))
        {
            return Err(invalid(
                "configuration",
                "raw and typed configuration paths overlap",
            ));
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
        state.raw = raw;
        state.phase = after;
        state.version = version;
        if after == Phase::Register {
            state.register_effects.extend(
                register_writes
                    .into_iter()
                    .map(|(key, value)| RegisterEffect::Write(key, value)),
            );
        } else {
            state.register_effects.clear();
        }
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }
}

fn config_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn config_type_error(key: &str, expected: &str, actual: &Value) -> ConfigError {
    ConfigError::Invalid(format!(
        "{key}: expected {expected}, got {}",
        config_type(actual)
    ))
}

fn config_string(key: &str, value: Value) -> std::result::Result<String, ConfigError> {
    match value {
        Value::String(text) => Ok(text),
        other => Err(config_type_error(key, "string", &other)),
    }
}

fn config_integer(key: &str, value: Value) -> std::result::Result<i64, ConfigError> {
    match value {
        Value::Number(number) if number.is_i64() || number.is_u64() => number
            .as_i64()
            .ok_or_else(|| ConfigError::Invalid(format!("{key}: integer exceeds i64 range"))),
        other => Err(config_type_error(key, "integer", &other)),
    }
}

fn config_float(key: &str, value: Value) -> std::result::Result<f64, ConfigError> {
    match value {
        Value::Number(number) if number.is_f64() => number
            .as_f64()
            .ok_or_else(|| ConfigError::Invalid(format!("{key}: expected float"))),
        other => Err(config_type_error(key, "float", &other)),
    }
}

fn config_boolean(key: &str, value: Value) -> std::result::Result<bool, ConfigError> {
    match value {
        Value::Bool(boolean) => Ok(boolean),
        other => Err(config_type_error(key, "boolean", &other)),
    }
}

fn config_array(key: &str, value: Value) -> std::result::Result<Value, ConfigError> {
    match value {
        Value::Array(_) | Value::Object(_) => Ok(value),
        other => Err(config_type_error(key, "array", &other)),
    }
}

fn config_collection(
    key: &str,
    value: Value,
) -> std::result::Result<Collection<Value>, ConfigError> {
    let array = config_array(key, value)?;
    Collection::from_config_array(array)
        .ok_or_else(|| ConfigError::Invalid(format!("{key}: expected array")))
}

// Laravel 只在整个输入 key 上优先查字面键；之后逐段遍历，不查剩余点串。
fn read_value<'a>(values: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(value) = read_entry(values, key) {
        return Some(value);
    }
    let mut current = values;
    for segment in key.split('.') {
        current = read_entry(current, segment)?;
    }
    Some(current)
}

fn apply_set_entries(
    base: Figment,
    raw: Option<Value>,
    entries: &[(String, Value)],
) -> Result<(Figment, Value)> {
    let mut raw = match raw {
        Some(raw) => raw,
        None => base
            .extract::<Value>()
            .map_err(|error| Error::Config(safe_source_error(error, "configuration")))?,
    };
    let mut touched = Vec::new();
    for (key, value) in entries {
        let head = key.split('.').next().unwrap_or("");
        if !touched.iter().any(|previous| previous == head) {
            touched.push(head.to_owned());
        }
        set_dot_value(&mut raw, key, value.clone());
    }
    let mut candidate = base;
    for key in touched {
        let value = read_entry(&raw, &key)
            .cloned()
            .ok_or_else(|| invalid("configuration", "raw projection failed"))?;
        let original = candidate.find_value(&key).ok();
        candidate = replace_section(&candidate, &key, value, original.as_ref())?;
    }
    Ok((candidate, raw))
}

fn read_entry<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(items) => items.get(key),
        Value::Array(items) => {
            let index = key.parse::<usize>().ok()?;
            if index.to_string() == key {
                items.get(index)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn read_dot_value<'a>(values: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(values, |current, segment| read_entry(current, segment))
}

fn insert_read(result: &mut Vec<(String, Value)>, key: &str, value: Value) {
    if let Some((_, previous)) = result.iter_mut().find(|(existing, _)| existing == key) {
        *previous = value;
    } else {
        result.push((key.to_owned(), value));
    }
}

// Figment 的字典按键排序；启动默认值发布时只采纳其值，不丢弃已经写入的原始键序。
pub(crate) fn reorder_like(previous: &Value, candidate: Value) -> Value {
    match (previous, candidate) {
        (Value::Object(old), Value::Object(mut current)) => {
            let mut ordered = serde_json::Map::new();
            for (key, before) in old {
                if let Some(after) = current.shift_remove(key) {
                    ordered.insert(key.clone(), reorder_like(before, after));
                }
            }
            ordered.extend(current);
            Value::Object(ordered)
        }
        (Value::Array(old), Value::Array(current)) => Value::Array(
            current
                .into_iter()
                .enumerate()
                .map(|(index, after)| match old.get(index) {
                    Some(before) => reorder_like(before, after),
                    None => after,
                })
                .collect(),
        ),
        (Value::Array(old), Value::Object(mut current)) => {
            let mut entries = Vec::with_capacity(current.len());
            for (index, before) in old.iter().enumerate() {
                let key = index.to_string();
                if let Some(after) = current.shift_remove(&key) {
                    entries.push((key, reorder_like(before, after)));
                }
            }
            entries.extend(current);
            php_value(entries, true)
        }
        (_, current) => current,
    }
}

// Laravel 写入只将完整 key 拆段；即使根上已有字面 "a.b"，也写入 a -> b。
pub(crate) fn set_dot_value(current: &mut Value, key: &str, value: Value) {
    let parts: Vec<_> = key.split('.').collect();
    let Some((leaf, ancestors)) = parts.split_last() else {
        return;
    };
    let mut level = std::mem::take(current);
    let mut parents = Vec::with_capacity(ancestors.len());
    for head in ancestors {
        let child = read_entry(&level, head)
            .filter(|item| item.is_object() || item.is_array())
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        parents.push((*head, level));
        level = child;
    }
    set_php_entry(&mut level, leaf, value);
    for (head, mut parent) in parents.into_iter().rev() {
        set_php_entry(&mut parent, head, level);
        level = parent;
    }
    *current = level;
}

fn set_php_entry(current: &mut Value, key: &str, value: Value) {
    let mut entries = php_entries(std::mem::take(current));
    if let Some((_, previous)) = entries.iter_mut().find(|(name, _)| name == key) {
        *previous = value;
    } else {
        entries.push((key.to_owned(), value));
    }
    *current = php_value(entries, true);
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

fn overlay_tags(new: &mut FigmentValue, overlay: &FigmentValue) {
    if new.deserialize::<Value>().ok() == overlay.deserialize::<Value>().ok() {
        *new = overlay.clone();
        return;
    }
    match (new, overlay) {
        (FigmentValue::Dict(_, new), FigmentValue::Dict(_, overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = new.get_mut(key) {
                    overlay_tags(existing, value);
                }
            }
        }
        (FigmentValue::Array(_, new), FigmentValue::Array(_, overlay)) => {
            for (existing, value) in new.iter_mut().zip(overlay) {
                overlay_tags(existing, value);
            }
        }
        _ => {}
    }
}

pub(crate) fn replace_section(
    base: &Figment,
    key: &str,
    value: Value,
    old: Option<&FigmentValue>,
) -> Result<Figment> {
    replace_section_overlay(base, key, value, old, None)
}

pub(crate) fn replace_section_overlay(
    base: &Figment,
    key: &str,
    value: Value,
    old: Option<&FigmentValue>,
    overlay: Option<&FigmentValue>,
) -> Result<Figment> {
    let mut data = base
        .data()
        .map_err(|error| Error::Config(safe_source_error(error, key)))?;
    let mut replacement = FigmentValue::serialize(value)
        .map_err(|error| Error::Config(safe_source_error(error, key)))?;
    if let Some(old) = old {
        preserve_tags(&mut replacement, old);
    }
    if let Some(overlay) = overlay {
        overlay_tags(&mut replacement, overlay);
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

fn safe_schema_path(
    schema: &SectionSchema,
    key: &str,
    source_path: &[String],
    missing: Option<&str>,
) -> String {
    let key_parts: Vec<&str> = key.split('.').collect();
    let mut parts: Vec<&str> = source_path
        .iter()
        .flat_map(|part| part.split('.'))
        .collect();
    if parts.ends_with(&key_parts) && !parts.starts_with(&key_parts) {
        parts.reverse();
    }
    let relative = parts.strip_prefix(key_parts.as_slice()).unwrap_or(&parts);
    let mut output = key.to_owned();
    let mut node = schema;
    for part in relative.iter().copied().chain(missing) {
        while let SectionSchema::Nullable(inner) | SectionSchema::Sensitive(inner) = node {
            node = inner;
        }
        match node {
            SectionSchema::Object { fields, .. } => {
                let Some(next) = fields.get(part) else { break };
                output.push('.');
                output.push_str(part);
                node = next;
            }
            SectionSchema::Map(next) => {
                output.push_str(".<entry>");
                node = next;
            }
            SectionSchema::Array(next) => {
                if part.is_empty()
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                    || part.parse::<usize>().is_err()
                    || (part.starts_with('0') && part != "0")
                {
                    break;
                }
                output.push_str(".<index>");
                node = next;
            }
            _ => break,
        }
    }
    output
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
            // Figment 路径可来自配置源；数组段即使是数字也不能作为可信索引回显。
            let missing = match &error.kind {
                figment::error::Kind::MissingField(std::borrow::Cow::Borrowed(field)) => {
                    Some(*field)
                }
                _ => None,
            };
            let path = schema.map_or_else(
                || key.to_owned(),
                |schema| safe_schema_path(schema, key, &error.path, missing),
            );
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

fn code_merge_error(error: Error) -> ConfigError {
    match error {
        Error::Config(error) => error,
        _ => ConfigError::Invalid("模块代码配置覆盖失败（源值未公开）".to_owned()),
    }
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

pub(crate) fn php_value(entries: Vec<(String, Value)>, empty_array: bool) -> Value {
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

// 来源中的 JSON 对象也可能以规范数字键表达 PHP 数组；复用写入的键形态规则。
pub(crate) fn normalize_php_source(value: Value, empty_array: bool) -> Value {
    match value {
        Value::Object(items) => php_value(
            items
                .into_iter()
                .map(|(key, value)| (key, normalize_php_source(value, false)))
                .collect(),
            empty_array,
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|value| normalize_php_source(value, false))
                .collect(),
        ),
        value => value,
    }
}

pub(crate) fn merge_shallow(defaults: &mut Value, current: Value, _path: &str) {
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

fn numeric_merge_defaults(defaults: &Value) -> Option<Value> {
    match defaults {
        Value::Array(_) => Some(defaults.clone()),
        Value::Object(entries) => {
            let numeric: serde_json::Map<String, Value> = entries
                .iter()
                .filter(|(key, _)| integer_key(key).is_some())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            (!numeric.is_empty()).then_some(Value::Object(numeric))
        }
        _ => None,
    }
}

fn collect_register_effects(
    before: Option<&Value>,
    after: &Value,
    path: &str,
    skip_numeric: bool,
    writes: &mut Vec<RegisterEffect>,
) {
    if before == Some(after) {
        return;
    }
    if let Some(entries) = after.as_object()
        && before.is_none_or(Value::is_object)
    {
        if entries.is_empty() || entries.keys().any(|key| key.contains('.')) {
            writes.push(RegisterEffect::Write(path.to_owned(), after.clone()));
            return;
        }
        for (key, value) in entries {
            if skip_numeric && integer_key(key).is_some() {
                continue;
            }
            let prior = before.and_then(|value| value.get(key));
            collect_register_effects(prior, value, &format!("{path}.{key}"), false, writes);
        }
    } else {
        writes.push(RegisterEffect::Write(path.to_owned(), after.clone()));
    }
}

fn preview_register_operation(
    figment: &Figment,
    raw: Option<&Value>,
    key: &str,
    defaults: Value,
    recursive: bool,
) -> Result<(Figment, Value)> {
    let current = match figment.extract_inner::<Value>(key) {
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
    let current = match raw.and_then(|order| read_dot_value(order, key)) {
        Some(prior) => reorder_like(prior, current),
        None => current,
    };
    if !is_php_array(&current) {
        return Err(invalid(
            key,
            "package configuration must be object or array",
        ));
    }
    let mut preview = defaults;
    if recursive {
        replace_recursive(&mut preview, current, key)?;
    } else {
        merge_shallow(&mut preview, current, key);
    }
    let original = figment.find_value(key).ok();
    let candidate = replace_section(figment, key, preview.clone(), original.as_ref())?;
    Ok((candidate, preview))
}

// 精确文件区段沿 Laravel base→file 的顶层浅合并；只给 Figment 缺失键打补丁，
// 避免 join 整个 typed 默认对象把普通嵌套旧项递归补回。
fn file_default_patch(key: &str, defaults: &Value, current: &Value) -> (Value, Value) {
    if defaults.is_array() && current.is_array() {
        let mut combined = defaults.clone();
        merge_shallow(&mut combined, current.clone(), key);
        return (combined, Value::Object(serde_json::Map::new()));
    }
    let (Some(base), Some(source)) = (defaults.as_object(), current.as_object()) else {
        return (current.clone(), Value::Object(serde_json::Map::new()));
    };
    let mut combined = defaults.clone();
    merge_shallow(&mut combined, current.clone(), key);
    let mut patch = serde_json::Map::new();
    for (name, value) in base {
        if !source.contains_key(name) {
            patch.insert(name.clone(), value.clone());
        }
    }
    for option in mergeable_options(key) {
        if let (Some(old), Some(new)) = (base.get(*option), source.get(*option))
            && is_php_array(old)
            && is_php_array(new)
        {
            let mut merged = old.clone();
            merge_shallow(&mut merged, new.clone(), option);
            if let Some(section) = combined.as_object_mut() {
                section.insert((*option).to_owned(), merged);
            }
        }
    }
    (combined, Value::Object(patch))
}

fn is_php_array(value: &Value) -> bool {
    value.is_object() || value.is_array()
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
    use figment::providers::Serialized;
    use serde::Deserialize;
    use serde_json::json;
    use std::cell::Cell;

    fn code_repository(raw: &Value) -> Result<ConfigRepository> {
        let entries = raw
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("test", "expected object"))?;
        let mut sources = ConfigSources::from_ordered_entries(entries)?;
        sources.set_code_context(ConfigContext::new(
            crate::config::EnvironmentSnapshot::capture(),
        ));
        let mut repository = ConfigRepository::new(sources);
        repository.validate_base()?;
        Ok(repository)
    }

    #[test]
    fn module_dependencies_read_recursive_boot_candidate_and_publish_once() -> Result<()> {
        let mut repository = code_repository(
            &json!({"app":{"name":"boot-app"},"package":{"items":[1,2,3],"nested":{"old":1}}}),
        )?;
        let version = repository.version();
        let manifest = ConfigManifest::new()
            .section("package", |_| Ok(json!({"items":[9],"nested":{"new":2}})))
            .typed("package.runtime", |_| Ok(17_u16))
            .section("consumer", |context| {
                assert_eq!(context.require::<Vec<u8>>("package.items")?, vec![9, 2, 3]);
                assert_eq!(
                    context.require::<Value>("package.nested")?,
                    json!({"old":1,"new":2})
                );
                assert!(context.get::<Value>("package.runtime").is_err());
                Ok(json!({"name":context.require::<String>("app.name")?}))
            });
        repository.apply_code_manifest(&manifest, None)?;
        assert_eq!(repository.version(), version + 1);
        assert_eq!(repository.string("consumer.name")?, "boot-app");
        assert_eq!(repository.get("package.items")?, Some(json!([9, 2, 3])));
        assert_eq!(
            repository.typed::<u16>("package.runtime")?.as_deref(),
            Some(&17)
        );
        Ok(())
    }

    #[test]
    fn module_dependency_failure_does_not_publish_a_partial_candidate() -> Result<()> {
        let mut repository = code_repository(&json!({"package":{"number":1}}))?;
        let before = repository.all()?;
        let version = repository.version();
        let manifest = ConfigManifest::new()
            .section("package", |_| Ok(json!({"number":2})))
            .section("consumer", |context| {
                Ok(json!({"value":context.require::<String>("package.number")?}))
            });
        assert!(repository.apply_code_manifest(&manifest, None).is_err());
        assert_eq!(repository.all()?, before);
        assert_eq!(repository.version(), version);
        Ok(())
    }

    #[test]
    fn module_dependency_snapshot_and_cas_keep_concurrent_write_without_partial_publish()
    -> Result<()> {
        let mut repository = code_repository(&json!({"package":{"number":1}}))?;
        let version = repository.version();
        let writer = repository.clone();
        let manifest = ConfigManifest::new()
            .section("package", move |_| {
                writer
                    .set("concurrent", json!(true))
                    .map_err(code_merge_error)?;
                Ok(json!({"number":2}))
            })
            .section("consumer", |context| {
                assert_eq!(context.get::<bool>("concurrent")?, None);
                assert_eq!(context.require::<u8>("package.number")?, 2);
                Ok(json!({"ok":true}))
            });
        let error = repository
            .apply_code_manifest(&manifest, None)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("concurrent configuration update"));
        assert_eq!(repository.get("package.number")?, Some(json!(1)));
        assert_eq!(repository.get("consumer")?, None);
        assert!(repository.boolean("concurrent")?);
        assert_eq!(repository.version(), version + 1);
        Ok(())
    }

    #[test]
    fn module_existing_typed_overlap_is_preflighted_before_side_effects() -> Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut repository = code_repository(&json!({}))?;
        repository
            .apply_code_manifest(&ConfigManifest::new().typed("runtime", |_| Ok(1_u8)), None)?;
        let count = Arc::new(AtomicUsize::new(0));
        let called = count.clone();
        let manifest = ConfigManifest::new()
            .section("first", move |_| {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(json!({}))
            })
            .typed("runtime.child", |_| Ok(2_u8));
        assert!(repository.apply_code_manifest(&manifest, None).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(repository.typed::<u8>("runtime")?.as_deref(), Some(&1));
        Ok(())
    }

    #[test]
    fn cached_source_does_not_replay_register_defaults() -> Result<()> {
        let cached = json!({"demo": {"child": {"new": 2}, "items": [7]}});
        let mut repository = ConfigRepository::new(ConfigSources::from_cached(cached.clone())?);
        repository.merge_defaults("demo", json!({"from_a": 1, "items": [1]}))?;
        repository.register_section(
            TypeId::of::<()>(),
            "demo",
            json!({"from_b": 2, "items": [9]}),
            SectionSchema::any(),
            |_: &Value| Ok(()),
        )?;
        assert_eq!(repository.get("demo")?, Some(cached["demo"].clone()));
        repository.validate_base()?;
        assert_eq!(repository.get("demo.items")?, Some(json!([7])));
        Ok(())
    }

    // 父 auth 文件的显式 Provider 必须整体获胜，不能混入 eloquent 模型默认值。
    #[test]
    fn parent_auth_file_keeps_explicit_provider_through_registration_and_freeze() -> Result<()> {
        let mut sources = ConfigSources::from_ordered_entries([("auth", json!({}))])?;
        sources.merge_code_section(
            "auth",
            json!({"providers": {"users": {"driver": "app-users", "table": "users"}}}),
        )?;
        sources = sources.merge_ordered_entries([(
            "auth",
            json!({"providers": {"users": {"connection": "application"}}}),
        )])?;
        let mut repository = ConfigRepository::new(sources);
        repository.set("auth.providers.users.table", json!("accounts"))?;
        repository.register_section(
            TypeId::of::<()>(),
            "auth.defaults",
            json!({"guard": "web"}),
            SectionSchema::any(),
            |_: &Value| Ok(()),
        )?;
        repository.register_section(
            TypeId::of::<()>(),
            "auth.providers",
            json!({
                "users": {"driver": "eloquent", "model": "app::models::User"},
                "external": {"driver": "custom"}
            }),
            SectionSchema::any(),
            |providers: &Value| {
                if providers["users"].get("model").is_some() {
                    return Err(invalid("auth.providers", "unexpected model default"));
                }
                Ok(())
            },
        )?;
        repository.register_section(
            TypeId::of::<()>(),
            "auth.timebox_duration",
            200_000_u64,
            SectionSchema::Integer,
            |_| Ok(()),
        )?;
        let expected = json!({
            "users": {"driver": "app-users", "table": "accounts", "connection": "application"},
            "external": {"driver": "custom"}
        });
        assert_eq!(repository.get("auth.providers")?, Some(expected.clone()));
        repository.validate_base()?;
        assert_eq!(repository.section::<Value>("auth.providers")?, expected);
        assert_eq!(repository.get("auth.defaults.guard")?, Some(json!("web")));
        assert_eq!(
            repository.get("auth.timebox_duration")?,
            Some(json!(200_000))
        );
        repository.freeze()?;
        assert_eq!(repository.section::<Value>("auth.providers")?, expected);
        repository.set("auth.providers.users.table", json!("runtime_accounts"))?;
        assert_eq!(
            repository.get("auth.providers.users.table")?,
            Some(json!("runtime_accounts"))
        );
        assert_eq!(repository.get("auth.providers.users.model")?, None);
        Ok(())
    }

    // 父文件没有 providers 时，子区段仍能补入完整默认值。
    #[test]
    fn parent_file_missing_child_receives_section_defaults() -> Result<()> {
        let mut sources = ConfigSources::from_ordered_entries([("auth", json!({}))])?;
        sources.merge_code_section("auth", json!({"defaults": {"guard": "web"}}))?;
        let mut repository = ConfigRepository::new(sources);
        let providers = json!({"users": {"driver": "eloquent", "model": "app::models::User"}});
        repository.register_section(
            TypeId::of::<()>(),
            "auth.providers",
            providers.clone(),
            SectionSchema::any(),
            |_: &Value| Ok(()),
        )?;
        repository.validate_base()?;
        assert_eq!(repository.section::<Value>("auth.providers")?, providers);
        assert_eq!(repository.get("auth.defaults.guard")?, Some(json!("web")));
        Ok(())
    }

    // 同样的来源未标记文件域时保留普通深默认；不能全局改为浅合并。
    #[test]
    fn child_section_without_file_domain_keeps_deep_defaults() -> Result<()> {
        let mut repository = ConfigRepository::new(ConfigSources::from_ordered_entries([(
            "auth",
            json!({"providers": {"users": {"driver": "app-users", "table": "users"}}}),
        )])?);
        repository.register_section(
            TypeId::of::<()>(),
            "auth.providers",
            json!({"users": {"driver": "eloquent", "model": "app::models::User"}}),
            SectionSchema::any(),
            |_: &Value| Ok(()),
        )?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("auth.providers")?,
            json!({"users": {"driver": "app-users", "table": "users", "model": "app::models::User"}})
        );
        Ok(())
    }

    // 子区段比 Laravel 可合并选项更深时，也不能穿透显式 Provider 条目的浅边界。
    #[test]
    fn nested_section_defaults_do_not_reopen_explicit_provider_fields() -> Result<()> {
        let mut sources = ConfigSources::from_ordered_entries([("auth", json!({}))])?;
        sources.merge_code_section(
            "auth",
            json!({"providers": {"users": {"driver": "app-users", "table": "users"}}}),
        )?;
        let mut repository = ConfigRepository::new(sources);
        repository.register_section(
            TypeId::of::<()>(),
            "auth.providers.users",
            json!({"driver": "eloquent", "model": "app::models::User"}),
            SectionSchema::any(),
            |_: &Value| Ok(()),
        )?;
        repository.validate_base()?;
        assert_eq!(
            repository.section::<Value>("auth.providers.users")?,
            json!({"driver": "app-users", "table": "users"})
        );
        Ok(())
    }

    #[test]
    fn strict_typed_readers_follow_laravel_value_kinds() -> Result<()> {
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(json!({
            "s": "hello", "i": 3, "f": 3.5, "b": true,
            "list": ["x", "y"], "mixed": {"2": "two", "name": "named", "0": "zero"},
            "none": null, "secret": "private-value"
        }))));
        repository.set("u", json!(u64::MAX))?;
        assert_eq!(repository.string("s")?, "hello");
        assert_eq!(repository.integer("i")?, 3);
        assert_eq!(repository.float("f")?, 3.5);
        assert!(repository.boolean("b")?);
        assert_eq!(repository.array("list")?, json!(["x", "y"]));
        assert_eq!(
            repository.array("mixed")?,
            json!({"2": "two", "name": "named", "0": "zero"})
        );
        assert_eq!(
            repository.collection("mixed")?.to_value()?,
            repository.array("mixed")?
        );
        assert_eq!(
            repository.collection("list")?.to_value()?,
            repository.array("list")?
        );

        for error in [
            repository.string("none").err(),
            repository.string("missing").err(),
            repository.integer("f").err(),
            repository.integer("u").err(),
            repository.float("i").err(),
            repository.boolean("i").err(),
            repository.array("i").err(),
            repository.collection("i").err(),
            repository.integer("secret").err(),
        ] {
            let message = error
                .ok_or_else(|| invalid("typed", "expected strict type error"))?
                .to_string();
            assert!(!message.contains("private-value"));
            assert!(!message.contains("18446744073709551615"));
        }
        assert!(
            repository
                .integer("u")
                .err()
                .ok_or_else(|| invalid("typed", "expected range error"))?
                .to_string()
                .contains("i64 range")
        );
        assert!(
            repository
                .float("i")
                .err()
                .ok_or_else(|| invalid("typed", "expected float error"))?
                .to_string()
                .contains("got integer")
        );
        assert!(
            repository
                .collection("i")
                .err()
                .ok_or_else(|| invalid("typed", "expected array error"))?
                .to_string()
                .contains("expected array")
        );
        Ok(())
    }

    #[test]
    fn strict_defaults_only_run_for_missing_keys() -> Result<()> {
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(json!({
            "s": "present", "i": 2, "f": 1.5, "b": false, "a": [], "n": null
        }))));
        assert_eq!(repository.string_or("s", json!(5))?, "present");
        assert_eq!(repository.integer_or("i", json!("bad"))?, 2);
        assert_eq!(repository.float_or("f", json!(1))?, 1.5);
        assert!(!repository.boolean_or("b", json!("bad"))?);
        assert_eq!(repository.array_or("a", json!(false))?, json!([]));
        assert!(repository.collection_or("a", json!(false))?.is_empty());

        let calls = Cell::new(0);
        assert_eq!(
            repository.string_or_else("s", || {
                calls.set(calls.get() + 1);
                json!("ignored")
            })?,
            "present"
        );
        assert_eq!(
            repository.integer_or_else("missing", || {
                calls.set(calls.get() + 1);
                json!(4)
            })?,
            4
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            repository.string_or("missing", json!("fallback"))?,
            "fallback"
        );
        assert_eq!(repository.float_or("missing", json!(2.5))?, 2.5);
        assert!(repository.boolean_or("missing", json!(true))?);
        assert_eq!(
            repository.array_or("missing", json!({"name": 1}))?,
            json!({"name": 1})
        );
        assert_eq!(
            repository
                .collection_or("missing", json!([1]))?
                .to_value()?,
            json!([1])
        );
        assert!(repository.integer_or("missing", json!("bad")).is_err());
        assert!(repository.float_or("missing", json!(2)).is_err());
        assert!(repository.string_or("n", json!("ignored")).is_err());
        assert!(repository.array_or("n", json!([])).is_err());
        assert!(repository.collection_or("n", json!([])).is_err());
        assert!(
            repository
                .boolean_or_else("n", || {
                    calls.set(calls.get() + 1);
                    json!(true)
                })
                .is_err()
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(repository.float_or_else("missing", || json!(2.25))?, 2.25);
        assert!(!repository.boolean_or_else("missing", || json!(false))?);
        assert_eq!(
            repository.array_or_else("missing", || json!([2]))?,
            json!([2])
        );
        assert_eq!(
            repository
                .collection_or_else("missing", || json!({"k": 3}))?
                .to_value()?,
            json!({"k": 3})
        );
        Ok(())
    }

    #[test]
    fn config_collection_does_not_inherit_input_bag_limits() -> Result<()> {
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(json!({}))));
        let mut mixed = serde_json::Map::new();
        mixed.insert("2".to_owned(), json!("two"));
        mixed.insert("name".to_owned(), json!("named"));
        mixed.insert("0".to_owned(), json!("zero"));
        repository.set("mixed", Value::Object(mixed))?;
        assert_eq!(
            repository
                .collection("mixed")?
                .all()
                .iter()
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>(),
            ["2", "name", "0"]
        );
        repository.set("wide", json!(vec![0; 1024]))?;
        assert_eq!(repository.collection("wide")?.all().len(), 1024);
        repository.set("wide", json!(vec![0; 1025]))?;
        assert_eq!(
            repository.array("wide")?.as_array().map(Vec::len),
            Some(1025)
        );
        assert_eq!(repository.collection("wide")?.all().len(), 1025);
        for depth in [64, 65] {
            let mut deep = json!("leaf");
            for _ in 0..depth {
                deep = json!([deep]);
            }
            repository.set("deep", json!([deep]))?;
            let collection = repository.collection("deep")?;
            assert_eq!(collection.all().len(), 1);
            assert!(collection.get(0_i64).is_some());
        }
        assert!(Collection::from_value(json!(vec![0; 1025])).is_err());
        Ok(())
    }

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
        repository.set("demo.count", json!(43))?;
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
        assert!(repository.section::<bool>("package.extra")?);
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
    // 区分缺失与 null，并防止把假值、空键或字面点键交给 Figment 路径解释。
    #[test]
    fn raw_reading_preserves_null_false_empty_and_literal_key_priority() -> Result<()> {
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(json!({
            "null": null, "false": false, "zero": 0, "empty": "", "": "empty-key",
            "a.b": "literal", "a": {"b": "nested", "b.c": "not-a-path"},
            "items": [{"name": "first"}], "named": {"01": "named"}
        }))));
        assert_eq!(repository.get("missing")?, None);
        assert!(!repository.has("missing")?);
        for (key, expected) in [
            ("null", Value::Null),
            ("false", json!(false)),
            ("zero", json!(0)),
            ("empty", json!("")),
        ] {
            assert!(repository.has(key)?);
            assert_eq!(repository.get(key)?, Some(expected.clone()));
            assert_eq!(repository.get_or(key, json!("default"))?, expected);
        }
        assert_eq!(
            repository.get_or("missing", json!("default"))?,
            json!("default")
        );
        assert_eq!(repository.get("")?, Some(json!("empty-key")));
        assert!(repository.has("")?);
        assert_eq!(repository.get("a.b")?, Some(json!("literal")));
        assert!(repository.has("a.b")?);
        assert_eq!(repository.get("a.b.c")?, None);
        assert!(!repository.has("a.b.c")?);
        assert_eq!(repository.get("items.0.name")?, Some(json!("first")));
        assert_eq!(repository.get("items.00.name")?, None);
        assert_eq!(repository.get("items.+0.name")?, None);
        assert_eq!(repository.get("named.01")?, Some(json!("named")));
        assert_eq!(repository.get(".")?, None);
        Ok(())
    }

    // 批量结果必须按首次出现顺序保留键，重复缺失键用最后一次默认值。
    #[test]
    fn raw_many_preserves_order_defaults_null_and_duplicate_semantics() -> Result<()> {
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(
            json!({"null": null, "value": false}),
        )));
        assert_eq!(
            repository.get_many(&["value", "missing", "null", "value"])?,
            vec![
                ("value".into(), json!(false)),
                ("missing".into(), Value::Null),
                ("null".into(), Value::Null)
            ]
        );
        assert_eq!(
            repository.get_many_with_defaults(&[
                ("missing", json!(1)),
                ("null", json!(2)),
                ("value", json!(3)),
                ("missing", json!(4)),
            ])?,
            vec![
                ("missing".into(), json!(4)),
                ("null".into(), Value::Null),
                ("value".into(), json!(false))
            ]
        );
        assert_eq!(repository.get_many(&[])?.len(), 0);
        assert_eq!(repository.get_many_with_defaults(&[])?.len(), 0);
        Ok(())
    }

    // 原始值读取不得写回仓储或改变版本、默认值发布和 freeze 屏障。
    #[test]
    fn raw_snapshots_are_independent_and_reads_preserve_lifecycle() -> Result<()> {
        let mut repository = registered(json!({}))?;
        assert_eq!(
            repository.get("demo")?,
            Some(json!({
                "name":"default","count":10,"items":["a","b","c"]
            }))
        );
        assert_eq!(repository.version(), 0);
        repository.validate_base()?;
        let version = repository.version();
        let mut detached = repository.all()?;
        detached["demo"]["name"] = json!("detached");
        let before = repository.get("demo")?;
        repository.overlay("demo", json!({"name": "new"}))?;
        assert_eq!(repository.version(), version + 1);
        assert_ne!(before, repository.get("demo")?);
        assert_eq!(detached["demo"]["name"], json!("detached"));
        repository.freeze()?;
        let version = repository.version();
        assert_eq!(repository.get("demo.name")?, Some(json!("new")));
        assert!(repository.has("demo.name")?);
        let _ = repository.get_many(&["demo", "missing"])?;
        let _ = repository.get_many_with_defaults(&[("missing", json!(false))])?;
        let _ = repository.all()?;
        assert_eq!(repository.version(), version);
        assert!(repository.overlay("demo", json!({"name": "late"})).is_err());
        Ok(())
    }

    // 来源错误可能含任意秘密；任何原始读取都必须整体失败且不回传原文。
    #[test]
    fn raw_source_failure_is_redacted_and_never_returns_partial_results() {
        struct FailedSource;
        impl Provider for FailedSource {
            fn metadata(&self) -> Metadata {
                Metadata::named("secret-metadata-sentinel")
            }
            fn data(&self) -> std::result::Result<Map<Profile, Dict>, figment::Error> {
                Err(figment::Error::from("secret-source-sentinel")
                    .with_path("secret-path-sentinel"))
            }
        }
        let repository = ConfigRepository::new(
            Figment::from(Serialized::defaults(
                json!({"valid": "secret-value-sentinel"}),
            ))
            .merge(FailedSource),
        );
        let errors = [
            repository.all().err(),
            repository.get("valid").err(),
            repository
                .get_or("missing", json!("secret-default-sentinel"))
                .err(),
            repository.has("missing").err(),
            repository.get_many(&["valid", "missing"]).err(),
            repository
                .get_many_with_defaults(&[("valid", Value::Null)])
                .err(),
            repository
                .set("valid", json!("new"))
                .err()
                .map(|error| match error {
                    Error::Config(config) => config,
                    other => ConfigError::Invalid(other.to_string()),
                }),
        ];
        for error in errors {
            assert!(error.is_some());
            let display = error.as_ref().map(ToString::to_string).unwrap_or_default();
            let debug = format!("{error:?}");
            assert!(!display.contains("secret-"), "{display}");
            assert!(!debug.contains("secret-"), "{debug}");
        }
        assert_eq!(repository.version(), 0);
    }

    #[test]
    fn source_path_is_not_trusted_even_with_a_schema() {
        let schema = SectionSchema::object([(
            "child",
            SectionSchema::object([("required", SectionSchema::String)]),
        )]);
        let error = safe_source_error_with_schema(
            figment::Error::from("secret-source-sentinel")
                .with_path("parent.child.secret-path-sentinel"),
            "parent",
            Some(&schema),
        );
        assert!(error.to_string().contains("parent.child:"));
        assert!(!error.to_string().contains("secret-"));
        assert!(!format!("{error:?}").contains("secret-"));
    }

    #[test]
    fn schema_validation_never_reflects_unknown_or_dynamic_source_keys() {
        let schema = SectionSchema::object([
            ("known", SectionSchema::String),
            (
                "stores",
                SectionSchema::map(SectionSchema::object([(
                    "token",
                    SectionSchema::String.sensitive(),
                )])),
            ),
            (
                "secrets",
                SectionSchema::map(SectionSchema::String).sensitive(),
            ),
            (
                "private",
                SectionSchema::object([("known", SectionSchema::String)]).sensitive(),
            ),
        ]);
        for value in [
            json!({"secret-unknown-key-sentinel": true}),
            json!({"stores": {"secret-map-key-sentinel": {"token": false}}}),
            json!({"secrets": {"secret-sensitive-key-sentinel": false}}),
            json!({"private": {"secret-sensitive-unknown-sentinel": false}}),
        ] {
            let error = schema.validate(&value, "parent").err();
            assert!(error.is_some());
            let display = error.as_ref().map(ToString::to_string).unwrap_or_default();
            let debug = format!("{error:?}");
            assert!(!display.contains("secret-"), "{display}");
            assert!(!debug.contains("secret-"), "{debug}");
        }
        let error = schema
            .validate(&json!({"stores": {"business": {"token": false}}}), "parent")
            .err();
        assert!(
            error
                .as_ref()
                .is_some_and(|error| error.to_string().contains("parent.stores.<entry>.token"))
        );
    }

    #[test]
    fn typed_source_path_redacts_numeric_array_segments() {
        let schema = SectionSchema::object([(
            "items",
            SectionSchema::array(SectionSchema::object([("name", SectionSchema::String)])),
        )]);
        for (path, expected) in [
            ("parent.items.0.name", "parent.items.<index>.name"),
            ("parent.items.123456789.name", "parent.items.<index>.name"),
            ("parent.items.01.name", "parent.items"),
            ("parent.items.secret-index-sentinel.name", "parent.items"),
            ("parent.name", "parent"),
        ] {
            let error = safe_source_error_with_schema(
                figment::Error::from("secret-value-sentinel").with_path(path),
                "parent",
                Some(&schema),
            );
            assert!(
                error.to_string().starts_with(&format!("{expected}:")),
                "{error}"
            );
            assert!(!error.to_string().contains("secret-"));
            assert!(!format!("{error:?}").contains("secret-"));
            assert!(!error.to_string().contains("123456789"));
            assert!(!format!("{error:?}").contains("123456789"));
        }
        let map_schema = SectionSchema::object([(
            "stores",
            SectionSchema::map(SectionSchema::object([("token", SectionSchema::String)])),
        )]);
        let error = safe_source_error_with_schema(
            figment::Error::from("secret-value-sentinel")
                .with_path("parent.stores.secret-map-key-sentinel.token"),
            "parent",
            Some(&map_schema),
        );
        assert!(
            error
                .to_string()
                .starts_with("parent.stores.<entry>.token:")
        );
        assert!(!format!("{error:?}").contains("secret-"));
    }

    // Serialized 来源在序列化失败时也不能泄露自定义错误或默认值。
    #[test]
    fn raw_serialization_failure_is_redacted() {
        struct Reject;
        impl Serialize for Reject {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("secret-serializer-sentinel"))
            }
        }
        let repository =
            ConfigRepository::new(Figment::from(Serialized::default("broken", Reject)));
        let result =
            repository.get_many_with_defaults(&[("missing", json!("secret-default-sentinel"))]);
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains("secret-"));
        assert!(repository.all().is_err());
    }

    // 并发发布时批量读取的两个字段必须来自同一版本，不能逐键克隆快照。
    #[test]
    fn raw_many_reads_one_snapshot_during_concurrent_publication() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        repository.register_section(
            TypeId::of::<Value>(),
            "pair",
            json!({"left": 0, "right": 0}),
            SectionSchema::Any,
            |_| Ok(()),
        )?;
        repository.validate_base()?;
        let reader = repository.clone();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let writer_barrier = barrier.clone();
        let writer = std::thread::spawn(move || -> Result<()> {
            writer_barrier.wait();
            for value in 1..=64 {
                repository.overlay("pair", json!({"left": value, "right": value}))?;
                std::thread::yield_now();
            }
            Ok(())
        });
        barrier.wait();
        for _ in 0..64 {
            let pair = reader.get_many(&["pair.left", "pair.right"])?;
            assert_eq!(pair[0].1, pair[1].1);
            let pair = reader.get_many_with_defaults(&[
                ("pair.left", Value::Null),
                ("pair.right", Value::Null),
            ])?;
            assert_eq!(pair[0].1, pair[1].1);
            let all = reader.all()?;
            assert_eq!(all["pair"]["left"], all["pair"]["right"]);
            std::thread::yield_now();
        }
        writer
            .join()
            .map_err(|_| invalid("test", "writer failed"))??;
        Ok(())
    }

    fn describe_php_shape(value: &Value) -> Value {
        match value {
            Value::Array(items) => json!({
                "shape": "list",
                "keys": (0..items.len()).map(|index| json!(["int", index])).collect::<Vec<_>>(),
                "values": items.iter().map(describe_php_shape).collect::<Vec<_>>()
            }),
            Value::Object(items) => json!({
                "shape": "map",
                "keys": items.keys().map(|key| match integer_key(key) {
                    Some(number) => json!(["int", number]),
                    None => json!(["string", key]),
                }).collect::<Vec<_>>(),
                "values": items.values().map(describe_php_shape).collect::<Vec<_>>()
            }),
            other => other.clone(),
        }
    }

    // 固定 PHP Repository/Arr 的最终值、键序与数组形态，防止 Figment 字典排序掩盖回归。
    #[test]
    fn runtime_set_matches_fixed_php_shapes_and_key_order() -> Result<()> {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/config_runtime_set.json"))
                .map_err(|_| invalid("test", "invalid PHP fixture"))?;
        let cases = fixture["cases"]
            .as_array()
            .ok_or_else(|| invalid("test", "missing PHP cases"))?;
        for case in cases {
            let initial = case["initial"].clone();
            let repository = if initial.as_object().is_some_and(serde_json::Map::is_empty) {
                ConfigRepository::new(Figment::new())
            } else {
                ConfigRepository::new(Figment::from(Serialized::defaults(initial)))
            };
            let writes = case["writes"]
                .as_array()
                .ok_or_else(|| invalid("test", "missing PHP writes"))?;
            for write in writes {
                let key = write[0]
                    .as_str()
                    .ok_or_else(|| invalid("test", "invalid PHP write key"))?;
                repository.set(key, write[1].clone())?;
            }
            assert_eq!(
                describe_php_shape(&repository.all()?),
                case["expected"],
                "{}",
                case["name"]
            );
            if case["name"] == "literal_and_order" {
                assert_eq!(repository.get("a.b")?, Some(json!("literal")));
                assert_eq!(
                    repository
                        .get("a")?
                        .and_then(|value| value.get("b").cloned()),
                    Some(json!("updated"))
                );
            }
        }
        let batch = &fixture["batch"];
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(
            batch["initial"].clone(),
        )));
        let writes = batch["writes"]
            .as_array()
            .ok_or_else(|| invalid("test", "missing PHP batch"))?
            .iter()
            .map(|write| {
                write[0]
                    .as_str()
                    .map(|key| (key.to_owned(), write[1].clone()))
                    .ok_or_else(|| invalid("test", "invalid PHP batch key"))
            })
            .collect::<Result<Vec<_>>>()?;
        repository.set_many(writes)?;
        assert_eq!(describe_php_shape(&repository.all()?), batch["expected"]);
        Ok(())
    }

    // Register/Boot 写入的稀疏顺序跨默认值发布、覆盖和冻结保持，启动校验仍生效。
    #[test]
    fn runtime_set_survives_startup_phases_without_losing_order_or_schema_boundary() -> Result<()> {
        let mut repository = registered(json!({}))?;
        repository.set_many([
            ("sample.2", json!("two")),
            ("sample.0", json!("zero")),
            ("sample.1", json!("one")),
            ("sample.name", json!("named")),
        ])?;
        let before = repository.get("sample")?;
        repository.validate_base()?;
        assert_eq!(repository.get("sample")?, before);
        repository.overlay("sample", json!({"name": "boot-name", "extra": true}))?;
        let sample = repository
            .get("sample")?
            .ok_or_else(|| invalid("test", "missing sample"))?;
        let keys = sample
            .as_object()
            .ok_or_else(|| invalid("test", "sample lost sparse shape"))?
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(keys, ["2", "0", "1", "name", "extra"]);
        repository.overlay("demo", json!({"name": "boot"}))?;
        assert_eq!(repository.get("sample")?, Some(sample.clone()));
        repository.freeze()?;
        assert_eq!(repository.get("sample")?, Some(sample));
        let version = repository.version();
        repository.set("demo.count", json!("secret-invalid-count"))?;
        assert_eq!(repository.version(), version + 1);
        assert_eq!(
            repository.get("demo.count")?,
            Some(json!("secret-invalid-count"))
        );
        let error = repository
            .section::<Settings>("demo")
            .err()
            .ok_or_else(|| invalid("test", "expected typed decode error"))?;
        assert!(!error.to_string().contains("secret-invalid-count"));

        let mut invalid_start = registered(json!({}))?;
        invalid_start.set("demo.count", json!("bad"))?;
        let version = invalid_start.version();
        assert!(invalid_start.validate_base().is_err());
        assert_eq!(invalid_start.version(), version);
        assert_eq!(invalid_start.get("demo.count")?, Some(json!("bad")));

        let mut numeric_root = ConfigRepository::new(Figment::new());
        numeric_root.set_many([("0", json!("first")), ("1", json!("second"))])?;
        let list = numeric_root.all()?;
        assert!(list.is_array());
        numeric_root.validate_base()?;
        numeric_root.freeze()?;
        assert_eq!(numeric_root.all()?, list);
        Ok(())
    }

    // 版本耗尽、旧 revision 与空批次均不能发布部分 raw 或投影。
    #[test]
    fn runtime_batch_failure_and_empty_batch_preserve_one_state() -> Result<()> {
        let repository =
            ConfigRepository::new(Figment::from(Serialized::defaults(json!({"value": 1}))));
        let before = repository.all()?;
        repository.set_many(Vec::<(String, Value)>::new())?;
        assert_eq!(repository.version(), 0);
        {
            let mut state = repository.lock();
            state.version = u64::MAX;
        }
        assert!(
            repository
                .set_many([("value", json!(2)), ("other", json!(3))])
                .is_err()
        );
        assert_eq!(repository.all()?, before);
        assert_eq!(
            repository
                .snapshot()
                .extract_inner::<u32>("value")
                .map_err(ConfigError::from)?,
            1
        );
        assert_eq!(repository.version(), u64::MAX);

        {
            let mut state = repository.lock();
            state.version = 0;
        }
        let stale = repository.lock().revision;
        repository.set("value", json!(4))?;
        let candidate = repository.snapshot();
        let raw = repository.all()?;
        assert!(
            repository
                .publish(
                    candidate,
                    Some(raw),
                    stale,
                    (Phase::Register, Phase::Register),
                    true,
                    Vec::new(),
                )
                .is_err()
        );
        assert_eq!(repository.get("value")?, Some(json!(4)));
        assert_eq!(repository.version(), 1);
        Ok(())
    }

    #[test]
    fn startup_raw_reorder_only_keeps_keys_present_in_candidate() {
        let previous = json!({"ordered": {"b": 1, "a": 2, "removed": 3}});
        let candidate = json!({"ordered": {"a": 20, "b": 10, "new": 4}});
        let result = reorder_like(&previous, candidate);
        let object = result["ordered"].as_object();
        assert_eq!(
            object.map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["b", "a", "new"])
        );
        assert_eq!(result["ordered"]["a"], 20);
        assert!(result["ordered"].get("removed").is_none());
    }

    #[test]
    fn figment_input_order_limit_is_explicit_before_runtime_writes() -> Result<()> {
        let repository =
            ConfigRepository::new(Figment::from(Serialized::defaults(json!({"z": 1, "a": 2}))));
        let source_order = repository
            .all()?
            .as_object()
            .map(|items| items.keys().cloned().collect::<Vec<_>>())
            .ok_or_else(|| invalid("test", "missing source map"))?;
        assert_eq!(source_order, ["a", "z"]);
        repository.set("tail", json!(3))?;
        let after = repository
            .all()?
            .as_object()
            .map(|items| items.keys().cloned().collect::<Vec<_>>())
            .ok_or_else(|| invalid("test", "missing written map"))?;
        assert_eq!(after, ["a", "z", "tail"]);
        Ok(())
    }

    // 来源原序要穿过 typed 默认补齐、包浅合并、boot 覆盖和冻结后的写入。
    #[test]
    fn ordered_sources_survive_defaults_package_boot_and_frozen_write() -> Result<()> {
        let sources = ConfigSources::from_ordered_entries([
            ("package", json!({"m": 1, "a": 20})),
            ("recursive", json!({"m": {"old": 1}, "a": 20})),
            ("demo", json!({"count": 2, "name": "from-source"})),
            ("untouched", json!({"z": 1, "a": 2})),
        ])?;
        let mut repository = ConfigRepository::new(sources);
        repository.register_section(
            TypeId::of::<Settings>(),
            "demo",
            defaults(),
            schema(),
            validate,
        )?;
        repository.register_section(
            TypeId::of::<bool>(),
            "zeta",
            true,
            SectionSchema::Boolean,
            |_| Ok(()),
        )?;
        repository.register_section(
            TypeId::of::<u32>(),
            "alpha",
            7_u32,
            SectionSchema::Integer,
            |_| Ok(()),
        )?;
        repository.merge_defaults("package", json!({"z": 0, "a": 10}))?;
        repository.replace_defaults_recursively(
            "recursive",
            json!({"z": 0, "m": {"first": 0}, "a": 10}),
        )?;
        repository.validate_base()?;
        let raw = repository.all()?;
        assert_eq!(
            raw.as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec![
                "package",
                "recursive",
                "demo",
                "untouched",
                "zeta",
                "alpha"
            ])
        );
        assert_eq!(
            raw["package"]
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["z", "a", "m"])
        );
        assert_eq!(raw["package"]["a"], 20);
        assert_eq!(
            raw["recursive"]
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["z", "m", "a"])
        );
        assert_eq!(
            raw["recursive"]["m"]
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["first", "old"])
        );
        assert_eq!(
            raw["demo"]
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["count", "name", "items"])
        );
        repository.overlay("untouched", json!({"a": 3, "tail": 4}))?;
        repository.freeze()?;
        repository.set("untouched.final", json!(5))?;
        assert_eq!(
            repository.all()?["untouched"]
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["z", "a", "tail", "final"])
        );
        assert_eq!(repository.section::<Settings>("demo")?.count, 2);
        Ok(())
    }

    // 批量读取与写入各自只发布一个完整版本；clone 共享服务，旧快照独立。
    #[test]
    fn runtime_batch_is_atomic_for_concurrent_readers() -> Result<()> {
        let repository = ConfigRepository::new(Figment::from(Serialized::defaults(
            json!({"pair": {"left": 0, "right": 0}}),
        )));
        let reader = repository.clone();
        let old = repository.all()?;
        let writer = std::thread::spawn(move || -> Result<()> {
            for value in 1..=64 {
                repository.set_many([("pair.left", json!(value)), ("pair.right", json!(value))])?;
                std::thread::yield_now();
            }
            Ok(())
        });
        for _ in 0..64 {
            let values = reader.get_many(&["pair.left", "pair.right"])?;
            assert_eq!(values[0].1, values[1].1);
            let pair: Value = reader.section("pair")?;
            assert_eq!(pair["left"], pair["right"]);
            std::thread::yield_now();
        }
        writer
            .join()
            .map_err(|_| invalid("test", "writer failed"))??;
        assert_eq!(old["pair"]["left"], 0);
        assert_eq!(reader.get("pair.left")?, Some(json!(64)));
        assert_eq!(reader.version(), 64);
        Ok(())
    }

    #[test]
    fn register_preview_is_visible_once_and_force_schema_keeps_owner_boundary() -> Result<()> {
        struct First;
        struct Second;
        let first = TypeId::of::<First>();
        let mut repository = ConfigRepository::new(Figment::new());
        repository.register_section_for(
            first,
            false,
            "preview",
            json!({"name":"first"}),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        assert_eq!(repository.section::<Value>("preview")?["name"], "first");
        assert!(!repository.source_has_section("preview")?);
        repository.merge_defaults_for("preview", json!({"items":[1]}))?;
        repository.merge_defaults_for("preview", json!({"items":[2]}))?;
        let before: Value = repository.section("preview")?;
        assert_eq!(before["items"], json!([1]));
        repository.register_section_for(
            first,
            true,
            "preview",
            json!({"name":"force"}),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        assert_eq!(repository.section::<Value>("preview")?["items"], json!([1]));
        assert_eq!(
            repository
                .register_section_for(
                    TypeId::of::<Second>(),
                    true,
                    "preview",
                    json!({}),
                    SectionSchema::any(),
                    |_| Ok(())
                )
                .err()
                .map(|error| error.code().to_owned()),
            Some("configuration_error".to_owned())
        );
        repository.validate_base()?;
        let after: Value = repository.section("preview")?;
        assert_eq!(after["items"], json!([1]));
        repository.freeze()?;
        repository.register_section_for(
            first,
            true,
            "late",
            json!({"enabled":true}),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        assert_eq!(repository.section::<Value>("late")?["enabled"], true);
        Ok(())
    }

    #[test]
    fn register_top_level_numeric_merge_applies_once_in_order() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        repository.merge_defaults_for("numbers", json!([1]))?;
        repository.merge_defaults_for("numbers", json!([2]))?;
        assert_eq!(repository.get("numbers")?, Some(json!([2, 1])));
        repository.validate_base()?;
        assert_eq!(repository.get("numbers")?, Some(json!([2, 1])));
        repository.freeze()?;
        assert_eq!(repository.get("numbers")?, Some(json!([2, 1])));
        Ok(())
    }

    #[test]
    fn register_merge_then_set_replaces_array_even_after_force() -> Result<()> {
        struct Owner;
        let owner = TypeId::of::<Owner>();
        let mut repository = ConfigRepository::new(Figment::new());
        repository.register_section_for(
            owner,
            false,
            "numbers",
            json!([0]),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        repository.merge_defaults_for("numbers", json!([1]))?;
        repository.set("numbers", json!([2]))?;
        assert_eq!(repository.get("numbers")?, Some(json!([2])));
        repository.register_section_for(
            owner,
            true,
            "numbers",
            json!([3]),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        assert_eq!(repository.get("numbers")?, Some(json!([2])));
        assert!(!repository.source_has_section("numbers")?);
        repository.validate_base()?;
        assert_eq!(repository.get("numbers")?, Some(json!([2])));
        Ok(())
    }

    #[test]
    fn register_set_then_merge_keeps_php_numeric_append_order() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        repository.set("numbers", json!([2]))?;
        repository.merge_defaults_for("numbers", json!([1]))?;
        assert_eq!(repository.get("numbers")?, Some(json!([1, 2])));
        repository.validate_base()?;
        assert_eq!(repository.get("numbers")?, Some(json!([1, 2])));
        Ok(())
    }

    #[test]
    fn register_child_set_after_merge_preserves_mixed_numeric_key_order() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        repository.merge_defaults_for("mixed", json!([1]))?;
        repository.set("mixed.name", json!("named"))?;
        repository.set("mixed.3", json!("third"))?;
        repository.set("mixed.0", json!(2))?;
        let before = repository
            .get("mixed")?
            .ok_or_else(|| invalid("test", "missing mixed values"))?;
        assert_eq!(before, json!({"0":2,"name":"named","3":"third"}));
        assert_eq!(
            before
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["0", "name", "3"])
        );
        repository.validate_base()?;
        assert_eq!(repository.get("mixed")?, Some(before));
        Ok(())
    }

    #[test]
    fn register_ancestor_set_after_nested_merge_discards_old_descendant() -> Result<()> {
        let mut repository = ConfigRepository::new(Figment::new());
        repository.merge_defaults_for("group.child", json!({"old":1}))?;
        repository.set("group", json!({"child":{"new":2}}))?;
        let before = json!({"child":{"new":2}});
        assert_eq!(repository.get("group")?, Some(before.clone()));
        repository.validate_base()?;
        assert_eq!(repository.get("group")?, Some(before));
        Ok(())
    }

    #[test]
    fn source_values_win_section_defaults_before_and_after_freeze() -> Result<()> {
        struct Owner;
        let owner = TypeId::of::<Owner>();
        let source = Figment::from(Serialized::defaults(json!({"probe":{"port":8080}})));
        let mut repository = ConfigRepository::new(source);
        repository.register_section_for(
            owner,
            false,
            "probe",
            json!({"port":3000,"enabled":true}),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        assert_eq!(
            repository.section::<Value>("probe")?,
            json!({"port":8080,"enabled":true})
        );
        repository.validate_base()?;
        repository.freeze()?;
        repository.register_section_for(
            owner,
            true,
            "probe",
            json!({"port":3000,"enabled":false}),
            SectionSchema::any(),
            |_| Ok(()),
        )?;
        assert_eq!(
            repository.section::<Value>("probe")?,
            json!({"port":8080,"enabled":true})
        );
        assert_eq!(repository.get("probe.port")?, Some(json!(8080)));
        assert_eq!(repository.all()?["probe"]["port"], 8080);
        Ok(())
    }
    // 动态 Provider 在 Frozen 阶段登记同一文件的子区段，仍不得深补显式条目。
    #[test]
    fn frozen_child_section_keeps_parent_file_entry_and_runtime_values() -> Result<()> {
        let mut sources = ConfigSources::from_ordered_entries([("auth", json!({}))])?;
        sources.merge_code_section(
            "auth",
            json!({
                "providers": {"users": {"driver": "database", "table": "accounts"}},
                "guards": {"web": {"driver": "session", "provider": "users"}}
            }),
        )?;
        let mut repository = ConfigRepository::new(sources);
        repository.validate_base()?;
        repository.freeze()?;
        repository.set("auth.providers.users.table", json!("runtime_accounts"))?;
        repository.register_section_for(
            TypeId::of::<()>(), false, "auth.providers",
            json!({"users": {"driver": "eloquent", "model": "User"}, "extra": {"driver": "custom"}}),
            SectionSchema::any(), |_: &Value| Ok(())
        )?;
        assert_eq!(
            repository.get("auth.providers.users")?,
            Some(json!({"driver": "database", "table": "runtime_accounts"}))
        );
        assert_eq!(
            repository.get("auth.providers.extra")?,
            Some(json!({"driver": "custom"}))
        );
        assert_eq!(
            repository.get("auth.guards.web.provider")?,
            Some(json!("users"))
        );
        repository.set("auth.providers.users.table", json!("second_runtime"))?;
        assert_eq!(repository.get("auth.providers.users.model")?, None);
        Ok(())
    }

    // 缓存 Frozen 晚注册仅装校验；不能补 model，也不能增加缓存没有的 Provider。
    #[test]
    fn cached_frozen_section_does_not_replay_defaults() -> Result<()> {
        let cached =
            json!({"auth": {"providers": {"users": {"driver": "database", "table": "accounts"}}}});
        let mut repository = ConfigRepository::new(ConfigSources::from_cached(cached.clone())?);
        repository.validate_base()?;
        repository.freeze()?;
        repository.register_section_for(
            TypeId::of::<()>(), false, "auth.providers",
            json!({"users": {"driver": "eloquent", "model": "User"}, "extra": {"driver": "custom"}}),
            SectionSchema::any(), |_: &Value| Ok(())
        )?;
        assert_eq!(repository.all()?, cached);
        repository.set("auth.providers.users.table", json!("runtime_accounts"))?;
        assert_eq!(
            repository.get("auth.providers.users.table")?,
            Some(json!("runtime_accounts"))
        );
        assert_eq!(repository.get("auth.providers.users.model")?, None);
        assert_eq!(repository.get("auth.providers.extra")?, None);
        Ok(())
    }
    #[test]
    fn frozen_ancestor_section_preserves_descendant_file_boundary_runtime_and_siblings()
    -> Result<()> {
        let mut sources = ConfigSources::from_ordered_entries([(
            "auth",
            json!({
                "defaults": {"guard": "web"},
                "runtime_note": "source-note",
                "session": {"timeout": 30}
            }),
        )])?;
        // 真正的文件边界位于待注册auth的子路径，不是auth本身。
        sources.merge_code_section(
            "auth.providers",
            json!({"users": {"driver": "database", "table": "accounts"}}),
        )?;
        let mut repository = ConfigRepository::new(sources);
        repository.validate_base()?;
        repository.freeze()?;
        repository.set_many([
            ("auth.providers.users.table", json!("runtime_accounts")),
            ("auth.defaults.guard", json!("admin")),
            ("auth.runtime_note", json!("runtime-note")),
            ("auth.session.timeout", json!(900)),
            ("auth.session.runtime_only", json!(true)),
        ])?;
        let before = repository.version();
        repository.register_section_for(
            TypeId::of::<()>(),
            false,
            "auth",
            json!({
                "providers": {
                    "users": {"driver": "eloquent", "model": "User"},
                    "extra": {"driver": "custom"}
                },
                "defaults": {"guard": "web", "passwords": "users"},
                "runtime_note": "default-note",
                "session": {"timeout": 5, "default_only": true},
                "new_nonfile_sibling": {"enabled": true}
            }),
            SectionSchema::any(),
            |_: &Value| Ok(()),
        )?;
        // users条目是auth.providers文件的浅覆盖；不能偷偷补model，但缺失extra应添加。
        assert_eq!(
            repository.get("auth.providers")?,
            Some(json!({
                "users": {"driver": "database", "table": "runtime_accounts"},
                "extra": {"driver": "custom"}
            }))
        );
        assert_eq!(repository.get("auth.providers.users.model")?, None);
        // 真正非文件子区段保持普通Frozen的递归默认合并，同时保留运行时及兄弟项。
        assert_eq!(
            repository.get("auth.defaults")?,
            Some(json!({"guard": "admin", "passwords": "users"}))
        );
        assert_eq!(
            repository.get("auth.session")?,
            Some(json!({"timeout": 900, "runtime_only": true, "default_only": true}))
        );
        assert_eq!(
            repository.get("auth.runtime_note")?,
            Some(json!("runtime-note"))
        );
        assert_eq!(
            repository.get("auth.new_nonfile_sibling.enabled")?,
            Some(json!(true))
        );
        assert_eq!(repository.version(), before + 1);
        // 发布后继续写入同一现值，不得再从pre_file_base重放旧的accounts。
        repository.set("auth.providers.users.table", json!("second_runtime"))?;
        assert_eq!(
            repository.get("auth.providers.users.table")?,
            Some(json!("second_runtime"))
        );
        assert_eq!(repository.get("auth.providers.users.model")?, None);
        Ok(())
    }
}

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock, Weak},
};

use super::{ConfigError, Value};
use crate::contracts::config::{self, Repository as _};

macro_rules! public_reader {
    ($name:ident,$with:ident,$lazy:ident,$output:ty,$default:ty) => {
        pub fn $name(&self, key: &str) -> Result<$output, ConfigError> {
            <Self as config::Repository>::$name(self, key)
        }
        pub fn $with(&self, key: &str, default: $default) -> Result<$output, ConfigError> {
            <Self as config::Repository>::$with(self, key, default)
        }
        pub fn $lazy(
            &self,
            key: &str,
            default: impl FnOnce() -> $default,
        ) -> Result<$output, ConfigError> {
            <Self as config::Repository>::$lazy(self, key, Box::new(default))
        }
    };
}
/// Stores configuration values under dot-separated paths.
pub struct Repository {
    values: RwLock<BTreeMap<String, Value>>,
    owner: Weak<crate::container::Container>,
    macros: Arc<super::ConfigMacroRegistry>,
}

impl Repository {
    public_reader!(string, string_with_default, string_or_else, String, String);
    public_reader!(integer, integer_with_default, integer_or_else, i64, i64);
    public_reader!(float, float_with_default, float_or_else, f64, f64);
    public_reader!(boolean, boolean_with_default, boolean_or_else, bool, bool);
    public_reader!(array, array_with_default, array_or_else, Value, Value);
    public_reader!(
        collection,
        collection_with_default,
        collection_or_else,
        crate::support::Collection,
        Value
    );
    pub fn offset_exists(&self, key: &str) -> bool {
        <Self as config::Repository>::offset_exists(self, key)
    }
    pub fn offset_get(&self, key: &str) -> Option<Value> {
        <Self as config::Repository>::offset_get(self, key)
    }
    pub fn offset_set(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        <Self as config::Repository>::offset_set(self, key, value)
    }
    pub fn offset_unset(&self, key: &str) -> Result<(), ConfigError> {
        <Self as config::Repository>::offset_unset(self, key)
    }
    pub fn get_or(&self, key: &str, default: Value) -> Value {
        <Self as config::Repository>::get_or(self, key, default)
    }
    pub fn get_or_else(&self, key: &str, default: impl FnOnce() -> Value) -> Value {
        <Self as config::Repository>::get_or_else(self, key, Box::new(default))
    }
    pub fn get_many<'a, K: Into<super::ConfigKey<'a>>>(
        &self,
        keys: impl IntoIterator<Item = K>,
    ) -> indexmap::IndexMap<String, Value> {
        <Self as config::Repository>::get_many(self, keys.into_iter().map(Into::into).collect())
    }
    pub fn set_many<K: Into<String>>(
        &self,
        values: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<(), ConfigError> {
        <Self as config::Repository>::set_many(
            self,
            &values
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect::<Vec<_>>(),
        )
    }
    pub fn new(values: BTreeMap<String, Value>) -> Self {
        Self {
            values: RwLock::new(values),
            owner: Weak::new(),
            macros: Arc::default(),
        }
    }

    pub(crate) fn with_owner(mut self, owner: Weak<crate::container::Container>) -> Self {
        self.owner = owner;
        self
    }

    // 常驻进程中只隔离应用语言配置，其他配置继续遵循既有 Repository 契约。
    fn locale_overrides(&self) -> Option<Arc<LocaleOverrides>> {
        use crate::contracts::container::Container as _;
        self.owner
            .upgrade()
            .filter(|owner| {
                owner.has_active_scope() && !crate::foundation::Application::is_bootstrapping(owner)
            })
            .and_then(|owner| owner.make().ok())
    }

    pub(crate) fn replace_with(&self, loaded: Self) {
        *self.values.write().expect("configuration lock poisoned") = loaded
            .values
            .into_inner()
            .expect("configuration lock poisoned");
    }

    fn insert_list_value(&self, key: &str, value: Value, prepend: bool) -> Result<(), ConfigError> {
        if (key == "app" || key.starts_with("app."))
            && let Some(overrides) = self.locale_overrides()
            && (overrides
                .0
                .lock()
                .expect("语言配置作用域锁中毒")
                .contains_key("app")
                || matches!(key, "app.locale" | "app.fallback_locale"))
        {
            return self.set(key, list_value(self.get(key), key, value, prepend)?);
        }
        let mut values = self.values.write().expect("configuration lock poisoned");
        let items = list_value(get(&values, key).cloned(), key, value, prepend)?;
        set(&mut values, key, items);
        Ok(())
    }
}

impl config::Repository for Repository {
    fn macro_registry(&self) -> Result<Arc<super::ConfigMacroRegistry>, ConfigError> {
        use crate::contracts::container::Container as _;
        if let Some(owner) = self.owner.upgrade() {
            return owner.make().map_err(Into::into);
        }
        if crate::support::facades::current_container().is_some() {
            super::macros::current_registry()
        } else {
            Ok(self.macros.clone())
        }
    }

    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    fn get(&self, key: &str) -> Option<Value> {
        if (key == "app" || key.starts_with("app.")) && self.locale_overrides().is_some() {
            return get(&self.all(), key).cloned();
        }
        get(
            &self.values.read().expect("configuration lock poisoned"),
            key,
        )
        .cloned()
    }

    fn all(&self) -> BTreeMap<String, Value> {
        let mut values = self
            .values
            .read()
            .expect("configuration lock poisoned")
            .clone();
        if let Some(overrides) = self.locale_overrides() {
            for (key, value) in overrides.0.lock().expect("语言配置作用域锁中毒").iter() {
                set(&mut values, key, value.clone());
            }
        }
        values
    }

    fn set(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        if let Some(overrides) = self.locale_overrides() {
            let mut overrides = overrides.0.lock().expect("语言配置作用域锁中毒");
            if key == "app" {
                // 父键整体替换必须丢弃先前的子键覆盖，包括缺失语言字段。
                overrides.clear();
                overrides.insert(key.to_owned(), value);
                return Ok(());
            }
            if matches!(key, "app.locale" | "app.fallback_locale")
                || (key.starts_with("app.") && overrides.contains_key("app"))
            {
                // 在已覆盖的 app 对象上按写入顺序修改，避免排序重放旧子键。
                if let Some(app) = overrides.get_mut("app") {
                    set_value(app, key.strip_prefix("app.").expect("应用子键"), &value);
                } else {
                    overrides.insert(key.to_owned(), value);
                }
                return Ok(());
            }
        }
        set(
            &mut self.values.write().expect("configuration lock poisoned"),
            key,
            value,
        );
        Ok(())
    }

    fn prepend(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.insert_list_value(key, value, true)
    }

    fn push(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.insert_list_value(key, value, false)
    }
}

fn list_value(
    current: Option<Value>,
    key: &str,
    value: Value,
    prepend: bool,
) -> Result<Value, ConfigError> {
    let mut items = match current {
        Some(Value::Array(items)) => Value::Array(items),
        Some(Value::Object(items)) => Value::Object(items),
        None => Value::Array(Vec::new()),
        Some(Value::Null) if !prepend => Value::Array(Vec::new()),
        Some(_) => {
            return Err(ConfigError::InvalidType {
                key: key.to_owned(),
                expected: "an array",
            });
        }
    };
    if prepend {
        items = crate::support::Arr::prepend(&items, value, None);
    } else {
        crate::support::Arr::push(&mut items, None, [value]).map_err(|_| {
            ConfigError::InvalidType {
                key: key.to_owned(),
                expected: "an array with an available integer key",
            }
        })?;
    }
    Ok(items)
}

fn get<'a>(values: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a Value> {
    // 与原仓储和旧版回归一致，完整字面键优先于点路径。
    if let Some(value) = values.get(key) {
        return Some(value);
    }
    let mut path = key.split('.');
    let mut value = values.get(path.next()?)?;
    for segment in path {
        value = match value {
            Value::Array(items) => {
                let index = segment.parse::<usize>().ok()?;
                if segment.starts_with('+') || (segment.len() > 1 && segment.starts_with('0')) {
                    return None;
                }
                items.get(index)?
            }
            _ => value.get(segment)?,
        };
    }
    Some(value)
}

fn set(values: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    let Some((root, path)) = key.split_once('.') else {
        values.insert(key.to_owned(), value);
        return;
    };
    set_value(
        values.entry(root.to_owned()).or_insert(Value::Null),
        path,
        &value,
    );
}

fn set_value(node: &mut Value, path: &str, value: &Value) {
    // 与 Arr::set 共用字面点路径写入，保留数组兄弟项、稀疏索引和对象键。
    crate::support::data_write(
        node,
        &path.split('.').collect::<Vec<_>>(),
        value,
        true,
        false,
    );
}

/// 应用语言在单次请求／队列任务中的配置覆盖。
#[derive(Default)]
pub(crate) struct LocaleOverrides(Mutex<BTreeMap<String, Value>>);

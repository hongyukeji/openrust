use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock, Weak},
};

use super::{ConfigError, Value};
use crate::contracts::config::{self, Repository as _};

/// Stores configuration values under dot-separated paths.
pub struct Repository {
    values: RwLock<BTreeMap<String, Value>>,
    owner: Weak<crate::container::Container>,
}

impl Repository {
    pub fn new(values: BTreeMap<String, Value>) -> Self {
        Self {
            values: RwLock::new(values),
            owner: Weak::new(),
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
        validate_key(key)?;
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
        validate_key(key)?;
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

fn validate_key(key: &str) -> Result<(), ConfigError> {
    if key.split('.').any(str::is_empty) {
        return Err(ConfigError::InvalidKey {
            key: key.to_owned(),
        });
    }
    Ok(())
}

fn get<'a>(values: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a Value> {
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

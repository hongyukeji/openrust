//! 同一容器作用域内的日志上下文与记录投影。

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{Map, Value};

use crate::Result;
use crate::container::ScopeFactory;
use crate::foundation::RegisterContext;
use crate::support::{BoxFuture, ServiceProvider};

/// 当前作用域的结构化字段。隐藏字段须显式读取。
#[derive(Default)]
pub struct Repository(Mutex<Fields>);

#[derive(Default)]
struct Fields {
    ordinary: Map<String, Value>,
    hidden: Map<String, Value>,
}

impl Repository {
    fn fields(&self) -> MutexGuard<'_, Fields> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 写入普通字段。数字键按 Laravel 的合并规则追加并重新编号。
    pub fn add(&self, key: impl Into<String>, value: Value) {
        let key = key.into();
        merge_field(&mut self.fields().ordinary, key, value);
    }

    /// 写入隐藏字段，保存在独立集合。
    pub fn add_hidden(&self, key: impl Into<String>, value: Value) {
        let key = key.into();
        merge_field(&mut self.fields().hidden, key, value);
    }

    /// 按字面键读取普通字段，缺失或 `null` 时返回 `None`。
    pub fn get(&self, key: &str) -> Option<Value> {
        read(&self.fields().ordinary, key)
    }

    /// 按字面键读取隐藏字段，缺失或 `null` 时返回 `None`。
    pub fn get_hidden(&self, key: &str) -> Option<Value> {
        read(&self.fields().hidden, key)
    }

    /// 判断普通键是否存在，值为 `null` 的键也存在。
    pub fn has(&self, key: &str) -> bool {
        self.fields().ordinary.contains_key(key)
    }

    /// 判断隐藏键是否存在，值为 `null` 的键也存在。
    pub fn has_hidden(&self, key: &str) -> bool {
        self.fields().hidden.contains_key(key)
    }

    /// 克隆当前普通字段，包含显式写入的 `null`。
    pub fn all(&self) -> Map<String, Value> {
        self.fields().ordinary.clone()
    }

    /// 显式克隆当前隐藏字段。
    pub fn all_hidden(&self) -> Map<String, Value> {
        self.fields().hidden.clone()
    }

    /// 移除一个普通字段。
    pub fn forget(&self, key: &str) {
        self.fields().ordinary.shift_remove(key);
    }

    /// 移除一个隐藏字段。
    pub fn forget_hidden(&self, key: &str) {
        self.fields().hidden.shift_remove(key);
    }

    /// 清空本作用域的两组字段。
    pub fn flush(&self) {
        *self.fields() = Fields::default();
    }
}

fn read(fields: &Map<String, Value>, key: &str) -> Option<Value> {
    fields.get(key).filter(|value| !value.is_null()).cloned()
}

fn integer_key(key: &str) -> Option<i64> {
    key.parse::<i64>()
        .ok()
        .filter(|value| value.to_string() == key)
}

fn compact_numeric_keys(fields: &mut Map<String, Value>) -> usize {
    // PHP array_merge 保留字符串键，并把已有数字键按原顺序压紧。
    let mut next = 0_usize;
    if fields.keys().any(|key| integer_key(key).is_some()) {
        *fields = std::mem::take(fields)
            .into_iter()
            .map(|(key, value)| {
                let key = if integer_key(&key).is_some() {
                    let key = next.to_string();
                    next += 1;
                    key
                } else {
                    key
                };
                (key, value)
            })
            .collect();
    }
    next
}

fn merge_field(fields: &mut Map<String, Value>, key: String, value: Value) {
    let next = compact_numeric_keys(fields);
    let key = if integer_key(&key).is_some() {
        next.to_string()
    } else {
        key
    };
    fields.insert(key, value);
}

/// 在应用容器登记每个入口独立的日志上下文。
pub struct ContextServiceProvider;

impl ServiceProvider for ContextServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.scoped_if::<Repository, _>(|_| {
                Box::pin(async { Ok(Arc::new(Repository::default())) })
            })
            .await?;
            Ok(())
        })
    }
}

pub(super) fn log_extra(scope_factory: &ScopeFactory) -> Map<String, Value> {
    let mut extra = scope_factory
        .container()
        .and_then(|container| container.get_entry_ready::<Repository>())
        .map_or_else(Map::new, |repository| repository.all());
    compact_numeric_keys(&mut extra);
    extra
}

#[cfg(test)]
#[path = "../../tests/unit/logging/context.rs"]
mod tests;

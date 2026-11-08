//! Session 与请求共享的有序 PHP 值树；对象只通过显式策略进入持久编码。

use std::{any::Any, fmt, sync::Arc};

use serde_json::{Map, Number, Value};

use super::ArrayKey;
use crate::{Error, Result};

/// 可保存在请求内值树的共享对象。
pub trait DataObject: Any + Send + Sync {
    /// 安全的类型下转入口；不得把敏感路径或内容写入错误。
    fn as_any(&self) -> &dyn Any;

    /// 显式 JSON 持久投影；`None` 表示对象不提供投影。
    /// 第三方实现须自行限制返回前的瞬时分配。
    fn json_projection(&self) -> Option<Value> {
        None
    }

    /// 显式字符串转换，对应对象的字符串协议。
    fn string_projection(&self) -> Option<String> {
        None
    }

    /// 显式模型数组投影，默认复用 JSON 投影。
    fn array_projection(&self) -> Option<Value> {
        self.json_projection()
    }
}

/// 对象保存策略；只影响编码，不能改变请求内对象身份。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ObjectEncoding {
    /// 拒绝持久对象，保持既有纯数据编码格式。
    #[default]
    Reject,
    /// 使用对象显式提供的 JSON 投影。
    JsonProjection,
}

/// 有序 PHP 键数组、JSON 标量和共享的类型安全对象。
#[derive(Clone)]
pub enum DataValue {
    /// 空值。
    Null,
    /// 布尔值。
    Bool(bool),
    /// 有限 JSON 数字。
    Number(Number),
    /// 字符串。
    String(String),
    /// 保持插入顺序与整数或字符串键身份的 PHP 数组。
    Array(Vec<(ArrayKey, DataValue)>),
    /// Clone 共享对象身份，不复制对象资源。
    Object(Arc<dyn DataObject>),
}

impl fmt::Debug for DataValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("Null"),
            Self::Bool(value) => value.fmt(formatter),
            Self::Number(value) => value.fmt(formatter),
            Self::String(_) => formatter.write_str("String([REDACTED])"),
            Self::Array(values) => formatter.debug_tuple("Array").field(&values.len()).finish(),
            Self::Object(_) => formatter.write_str("Object([REDACTED])"),
        }
    }
}

impl DataValue {
    /// 直接读取 PHP 数组键，不解释点号。
    pub fn direct(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Array(entries) => entries.iter().find_map(|(candidate, value)| {
                (candidate == &ArrayKey::from(key)).then_some(value)
            }),
            _ => None,
        }
    }

    /// Laravel Session 的完整字面键优先路径读取。
    pub fn get(&self, key: &str) -> Result<Option<&Self>> {
        if key.len() > 20 * 1024 * 1024 {
            return Err(limit());
        }
        if let Some(value) = self.direct(key) {
            return Ok(Some(value));
        }
        if key.split('.').take(65).count() > 64 {
            return Err(limit());
        }
        let mut current = self;
        for (depth, part) in key.split('.').enumerate() {
            if depth >= 64 {
                return Err(limit());
            }
            let Some(value) = current.direct(part) else {
                return Ok(None);
            };
            current = value;
        }
        Ok(Some(current))
    }

    /// 原子设置 dot 路径，保留未改动分支的对象句柄。
    pub fn set(&mut self, key: &str, value: Self) -> Result<()> {
        if key.len() > 20 * 1024 * 1024 {
            return Err(limit());
        }
        let path = key.split('.').take(65).collect::<Vec<_>>();
        if path.len() > 64 {
            return Err(limit());
        }
        let mut candidate = self.clone();
        candidate.set_path(&path, value);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    fn set_path(&mut self, path: &[&str], value: Self) {
        let Some((&head, tail)) = path.split_first() else {
            return;
        };
        if !self.is_array() {
            *self = Self::Array(Vec::new());
        }
        let Self::Array(entries) = self else { return };
        let key = ArrayKey::from(head);
        let position = entries.iter().position(|(candidate, _)| candidate == &key);
        if let Some(index) = position {
            if tail.is_empty() {
                entries[index].1 = value;
            } else {
                entries[index].1.set_path(tail, value);
            }
        } else {
            let mut child = Self::Array(Vec::new());
            if tail.is_empty() {
                child = value;
            } else {
                child.set_path(tail, value);
            }
            entries.push((key, child));
        }
    }

    /// 原子删除完整字面键，未命中时按 dot 路径删除。
    pub fn forget(&mut self, key: &str) -> Result<()> {
        if key.len() > 20 * 1024 * 1024 {
            return Err(limit());
        }
        let mut candidate = self.clone();
        if !candidate.remove_direct(key) {
            let path = key.split('.').take(65).collect::<Vec<_>>();
            if path.len() > 64 {
                return Err(limit());
            }
            candidate.remove_path(&path);
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    fn remove_direct(&mut self, key: &str) -> bool {
        let Self::Array(entries) = self else {
            return false;
        };
        let key = ArrayKey::from(key);
        if let Some(index) = entries.iter().position(|(candidate, _)| candidate == &key) {
            entries.remove(index);
            true
        } else {
            false
        }
    }

    fn remove_path(&mut self, path: &[&str]) {
        let Some((&head, tail)) = path.split_first() else {
            return;
        };
        if tail.is_empty() {
            self.remove_direct(head);
        } else if let Self::Array(entries) = self
            && let Some((_, child)) = entries
                .iter_mut()
                .find(|(key, _)| key == &ArrayKey::from(head))
        {
            child.remove_path(tail);
        }
    }

    /// 检查树深、节点、键和对象数量，错误不包含输入值。
    pub fn validate(&self) -> Result<()> {
        fn walk(
            value: &DataValue,
            depth: usize,
            prefix: usize,
            nodes: &mut usize,
            objects: &mut usize,
            bytes: &mut usize,
        ) -> Result<()> {
            if depth > 64 {
                return Err(limit());
            }
            match value {
                DataValue::Array(entries) => {
                    if entries.len() > 1024usize.saturating_sub(*nodes) {
                        return Err(limit());
                    }
                    let mut keys = std::collections::HashSet::new();
                    for (key, child) in entries {
                        let key_len = match key {
                            ArrayKey::Integer(_) => 20,
                            ArrayKey::String(value) => value.len(),
                        };
                        if key_len > 20 * 1024 * 1024 {
                            return Err(limit());
                        }
                        let key_text = key.to_string();
                        if *bytes > 20 * 1024 * 1024 - key_text.len() {
                            return Err(limit());
                        }
                        if !keys.insert(key_text.clone())
                            || *key != ArrayKey::from(key_text.as_str())
                        {
                            return Err(invalid_keys());
                        }
                        *nodes += 1;
                        let path = prefix
                            .saturating_add(key_text.len())
                            .saturating_add(usize::from(depth > 0));
                        *bytes = bytes.saturating_add(path);
                        if *nodes > 1024 || *bytes > 20 * 1024 * 1024 {
                            return Err(limit());
                        }
                        walk(child, depth + 1, path, nodes, objects, bytes)?;
                    }
                }
                DataValue::Object(_) => {
                    *objects += 1;
                    if *objects > 1024 {
                        return Err(limit());
                    }
                }
                DataValue::String(value) => {
                    *bytes = bytes.saturating_add(value.len());
                    if *bytes > 20 * 1024 * 1024 {
                        return Err(limit());
                    }
                }
                _ => {}
            }
            Ok(())
        }
        walk(self, 0, 0, &mut 0, &mut 0, &mut 0)
    }

    /// 包装对象为共享句柄。
    pub fn object<T: DataObject>(object: T) -> Self {
        Self::Object(Arc::new(object))
    }

    /// 类型安全借用对象；不把内部路径或内容暴露给日志。
    pub fn downcast_ref<T: DataObject>(&self) -> Option<&T> {
        match self {
            Self::Object(object) => object.as_any().downcast_ref::<T>(),
            _ => None,
        }
    }

    /// 比较两个值是否引用相同对象。
    pub fn same_object(&self, other: &Self) -> bool {
        matches!((self, other), (Self::Object(a), Self::Object(b)) if Arc::ptr_eq(a, b))
    }

    /// 是否为空值。
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// 是否为 PHP 数组。
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// 只借用数组条目，保留顺序和 PHP 键身份。
    pub fn entries(&self) -> Option<&[(ArrayKey, Self)]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// 有界纯数据投影；未投影对象返回明确错误。
    pub fn to_json(&self) -> Result<Value> {
        self.project(ObjectEncoding::Reject)
    }

    /// 按保存策略投影；只允许对象自己声明的 JSON 值。
    pub fn project(&self, encoding: ObjectEncoding) -> Result<Value> {
        self.project_inner(encoding, false)
    }

    /// 仅用于请求内记录预算预检；结果不能传入持久后端。
    #[cfg(any(feature = "session", test))]
    pub(crate) fn budget_json(&self) -> Result<Value> {
        self.project_inner(ObjectEncoding::Reject, true)
    }

    fn project_inner(&self, encoding: ObjectEncoding, placeholder: bool) -> Result<Value> {
        let mut budget = ProjectionBudget::default();
        let result = budget.project_value(self, encoding, placeholder, 0)?;
        super::data::validate_limits(&result).map_err(|_| limit())?;
        Ok(result)
    }

    /// 从已验证的 JSON 值保留关联键的顺序。
    pub fn from_json(value: Value) -> Result<Self> {
        super::data::validate_limits(&value).map_err(|_| limit())?;
        let result = convert_json_value(value)?;
        result.validate()?;
        Ok(result)
    }
}

#[derive(Default)]
struct ProjectionBudget {
    nodes: usize,
    bytes: usize,
    objects: usize,
}

impl ProjectionBudget {
    fn node(&mut self, depth: usize) -> Result<()> {
        self.nodes = self.nodes.saturating_add(1);
        if depth > 64 || self.nodes > 1024 {
            return Err(limit());
        }
        Ok(())
    }

    fn bytes(&mut self, count: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(count);
        if self.bytes > 20 * 1024 * 1024 {
            return Err(limit());
        }
        Ok(())
    }

    fn visit_json(&mut self, value: &Value, depth: usize, count_self: bool) -> Result<()> {
        if count_self {
            self.node(depth)?;
        }
        match value {
            Value::String(value) => self.bytes(value.len())?,
            Value::Array(values) => {
                for value in values {
                    self.visit_json(value, depth + 1, true)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    self.bytes(key.len())?;
                    self.visit_json(value, depth + 1, true)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn project_object(
        &mut self,
        object: &Arc<dyn DataObject>,
        encoding: ObjectEncoding,
        placeholder: bool,
        depth: usize,
    ) -> Result<Value> {
        match encoding {
            ObjectEncoding::Reject if placeholder => Ok(Value::Object(Map::new())),
            ObjectEncoding::Reject => Err(unprojected()),
            ObjectEncoding::JsonProjection => {
                self.objects = self.objects.saturating_add(1);
                if self.objects > 1024 {
                    return Err(limit());
                }
                let value = object.json_projection().ok_or_else(unprojected)?;
                self.visit_json(&value, depth, false)?;
                Ok(value)
            }
        }
    }

    fn project_array(
        &mut self,
        entries: &[(ArrayKey, DataValue)],
        encoding: ObjectEncoding,
        placeholder: bool,
        depth: usize,
    ) -> Result<Value> {
        if entries.len() > 1024usize.saturating_sub(self.nodes) {
            return Err(limit());
        }
        let mut keys = std::collections::HashSet::new();
        for (key, _) in entries {
            let key_len = match key {
                ArrayKey::Integer(_) => 20,
                ArrayKey::String(value) => value.len(),
            };
            if key_len > 20 * 1024 * 1024 {
                return Err(limit());
            }
            let key_text = key.to_string();
            self.bytes(key_text.len())?;
            if !keys.insert(key_text.clone()) || *key != ArrayKey::from(key_text.as_str()) {
                return Err(invalid_keys());
            }
        }
        if entries.iter().enumerate().all(|(index, (key, _))| {
            i64::try_from(index).is_ok_and(|index| *key == ArrayKey::Integer(index))
        }) {
            entries
                .iter()
                .map(|(_, value)| self.project_value(value, encoding, placeholder, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array)
        } else {
            let mut object = Map::new();
            for (key, value) in entries {
                object.insert(
                    key.to_string(),
                    self.project_value(value, encoding, placeholder, depth + 1)?,
                );
            }
            Ok(Value::Object(object))
        }
    }

    fn project_value(
        &mut self,
        value: &DataValue,
        encoding: ObjectEncoding,
        placeholder: bool,
        depth: usize,
    ) -> Result<Value> {
        if depth > 0 {
            self.node(depth)?;
        }
        match value {
            DataValue::Null => Ok(Value::Null),
            DataValue::Bool(value) => Ok(Value::Bool(*value)),
            DataValue::Number(value) => Ok(Value::Number(value.clone())),
            DataValue::String(value) => {
                self.bytes(value.len())?;
                Ok(Value::String(value.clone()))
            }
            DataValue::Object(object) => self.project_object(object, encoding, placeholder, depth),
            DataValue::Array(entries) => self.project_array(entries, encoding, placeholder, depth),
        }
    }
}

fn convert_json_value(value: Value) -> Result<DataValue> {
    match value {
        Value::Null => Ok(DataValue::Null),
        Value::Bool(value) => Ok(DataValue::Bool(value)),
        Value::Number(value) => Ok(DataValue::Number(value)),
        Value::String(value) => Ok(DataValue::String(value)),
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                Ok((
                    ArrayKey::Integer(i64::try_from(index).map_err(|_| limit())?),
                    convert_json_value(value)?,
                ))
            })
            .collect::<Result<Vec<_>>>()
            .map(DataValue::Array),
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| Ok((ArrayKey::from(key), convert_json_value(value)?)))
            .collect::<Result<Vec<_>>>()
            .map(DataValue::Array),
    }
}

impl TryFrom<Value> for DataValue {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        Self::from_json(value)
    }
}

impl From<bool> for DataValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for DataValue {
    fn from(value: i64) -> Self {
        Self::Number(Number::from(value))
    }
}

impl From<u64> for DataValue {
    fn from(value: u64) -> Self {
        Self::Number(Number::from(value))
    }
}

impl From<String> for DataValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for DataValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

fn unprojected() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "data_object_not_serializable",
        "对象没有可用的持久投影",
    )
}

fn limit() -> Error {
    Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "data_value_limits_exceeded",
        "值超过资源限制",
    )
}

fn invalid_keys() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "data_value_keys_invalid",
        "值含有重复或未规范化的 PHP 键",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Resource(String);
    struct Opaque;
    struct LargeProjection;

    impl DataObject for Opaque {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    impl DataObject for LargeProjection {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn json_projection(&self) -> Option<Value> {
            Some(Value::Array(vec![Value::Null; 600]))
        }
    }

    impl DataObject for Resource {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn json_projection(&self) -> Option<Value> {
            Some(json!({}))
        }
    }

    #[test]
    fn object_identity_survives_nested_copy_and_replacement() -> Result<()> {
        let secret = DataValue::object(Resource("/private/upload.tmp".to_owned()));
        let mut tree = DataValue::Array(Vec::new());
        tree.set("profile.avatar", secret.clone())?;
        let snapshot = tree.clone();
        let nested = tree.get("profile.avatar")?.ok_or_else(unprojected)?;
        assert!(nested.same_object(&secret));
        assert_eq!(
            nested
                .downcast_ref::<Resource>()
                .map(|resource| resource.0.as_str()),
            Some("/private/upload.tmp")
        );
        assert!(!format!("{tree:?}").contains("/private/upload.tmp"));
        assert_eq!(
            tree.to_json().err().map(|error| error.code().to_owned()),
            Some("data_object_not_serializable".to_owned())
        );
        assert_eq!(
            tree.project(ObjectEncoding::JsonProjection)?,
            json!({"profile":{"avatar":{}}})
        );
        tree.forget("profile.avatar")?;
        assert!(tree.get("profile.avatar")?.is_none());
        assert!(
            snapshot
                .get("profile.avatar")?
                .is_some_and(|value| value.same_object(&secret))
        );
        Ok(())
    }

    #[test]
    fn duplicate_or_noncanonical_keys_never_project() {
        let duplicate = DataValue::Array(vec![
            (ArrayKey::from("x"), DataValue::Null),
            (ArrayKey::from("x"), DataValue::Bool(true)),
        ]);
        assert_eq!(
            duplicate
                .validate()
                .err()
                .map(|error| error.code().to_owned()),
            Some("data_value_keys_invalid".to_owned())
        );
        assert!(duplicate.to_json().is_err());
        let object = DataValue::object(Resource("secret".to_owned()));
        let duplicate_object = DataValue::Array(vec![
            (ArrayKey::from("x"), object.clone()),
            (ArrayKey::from("x"), object),
        ]);
        assert!(
            duplicate_object
                .project(ObjectEncoding::JsonProjection)
                .is_err()
        );
        let noncanonical =
            DataValue::Array(vec![(ArrayKey::String("1".to_owned()), DataValue::Null)]);
        assert!(noncanonical.validate().is_err());
        assert!(
            noncanonical
                .project(ObjectEncoding::JsonProjection)
                .is_err()
        );
    }

    #[test]
    fn json_policy_requires_explicit_object_projection() {
        let value = DataValue::object(Opaque);
        assert_eq!(
            value
                .project(ObjectEncoding::JsonProjection)
                .err()
                .map(|error| error.code().to_owned()),
            Some("data_object_not_serializable".to_owned())
        );
        assert_eq!(value.budget_json().ok(), Some(json!({})));
    }

    #[test]
    fn multiple_object_projections_share_one_node_budget() {
        let first = DataValue::object(LargeProjection);
        let second = DataValue::object(LargeProjection);
        let value = DataValue::Array(vec![
            (ArrayKey::from("a"), first),
            (ArrayKey::from("b"), second),
        ]);
        assert_eq!(
            value
                .project(ObjectEncoding::JsonProjection)
                .err()
                .map(|error| error.code().to_owned()),
            Some("data_value_limits_exceeded".to_owned())
        );
    }

    #[test]
    fn wide_public_array_is_rejected_before_key_collection() {
        let value = DataValue::Array(
            (0..2000)
                .map(|index| (ArrayKey::Integer(index), DataValue::Null))
                .collect(),
        );
        assert_eq!(
            value
                .project(ObjectEncoding::JsonProjection)
                .err()
                .map(|error| error.code().to_owned()),
            Some("data_value_limits_exceeded".to_owned())
        );
        assert!(value.validate().is_err());
    }
}

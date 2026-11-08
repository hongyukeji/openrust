//! 模型属性的原值及日期对象身份。

use super::ModelObject;
use crate::{Error, Result, support::InputDate};
use serde_json::{Map, Value};
use std::{
    fmt,
    sync::{Arc, RwLock},
};

/// 保持水合与赋值顺序的属性映射。
pub type AttributeMap = indexmap::IndexMap<String, AttributeValue>;

/// Eloquent 原始属性及访问器值。
///
/// 已解码的 JSON 子树保持原样。日期对象保留时区和身份，需在数组、JSON 或 SQL
/// 边界应用对应的日期投影。
#[derive(Clone)]
pub enum AttributeValue {
    /// 标量或完整 JSON 子树。
    Json(Value),
    /// 具有共享身份的日期对象。
    Date(ModelDate),
    /// 由自定义类转换持有的共享对象。
    Object(ModelObject),
}

impl fmt::Debug for AttributeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(_) => f.write_str("Json([REDACTED])"),
            Self::Date(value) => value.fmt(f),
            Self::Object(value) => value.fmt(f),
        }
    }
}

impl AttributeValue {
    /// 显式读取 JSON 值。对象需要在相应投影边界转换。
    pub fn as_json(&self) -> Result<&Value> {
        match self {
            Self::Json(value) => Ok(value),
            Self::Date(_) | Self::Object(_) => Err(object_projection_required()),
        }
    }

    /// 将原值转回 JSON，保留完整形状。
    pub fn try_into_json(self) -> Result<Value> {
        match self {
            Self::Json(value) => Ok(value),
            Self::Date(_) | Self::Object(_) => Err(object_projection_required()),
        }
    }

    pub(super) fn into_array_json(self) -> Result<Value> {
        match self {
            Self::Json(value) => Ok(value),
            Self::Date(date) => serde_json::to_value(date.snapshot()?).map_err(Error::internal),
            Self::Object(value) => value.project_json(),
        }
    }

    /// 借用属性中的日期对象。
    pub const fn as_date(&self) -> Option<&ModelDate> {
        match self {
            Self::Date(value) => Some(value),
            Self::Json(_) | Self::Object(_) => None,
        }
    }

    /// 借用类转换创建的对象。
    pub const fn as_object(&self) -> Option<&ModelObject> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub(super) fn object_revision(&self) -> Result<Option<u64>> {
        match self {
            Self::Date(value) => value.revision().map(Some),
            Self::Object(value) => value.revision().map(Some),
            Self::Json(_) => Ok(None),
        }
    }

    /// 判断是否为显式 null。
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Json(Value::Null))
    }
}

impl From<Value> for AttributeValue {
    fn from(value: Value) -> Self {
        Self::Json(value)
    }
}

impl From<ModelDate> for AttributeValue {
    fn from(value: ModelDate) -> Self {
        Self::Date(value)
    }
}

impl From<ModelObject> for AttributeValue {
    fn from(value: ModelObject) -> Self {
        Self::Object(value)
    }
}

/// 具有 PHP 对象身份的模型日期。
///
/// 克隆句柄共享身份。可变日期在转换成功后更新同一对象，不可变日期返回新对象。
/// Calendar 的 `InputDate` 承担时刻、微秒、时区身份及格式化。
#[derive(Clone)]
pub struct ModelDate(Arc<DateObject>);

enum DateObject {
    Mutable(RwLock<MutableDate>),
    Immutable(InputDate),
}

struct MutableDate {
    value: InputDate,
    revision: u64,
    sql_bindings: usize,
}

pub(super) struct DateBinding {
    date: ModelDate,
}

impl Drop for DateBinding {
    fn drop(&mut self) {
        if let DateObject::Mutable(state) = &*self.date.0 {
            let mut state = state
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.sql_bindings -= 1;
        }
    }
}

impl ModelDate {
    /// 从 Calendar 快照创建独立日期对象。
    pub fn new(value: InputDate, immutable: bool) -> Self {
        let object = if immutable {
            DateObject::Immutable(value)
        } else {
            DateObject::Mutable(RwLock::new(MutableDate {
                value,
                revision: 0,
                sql_bindings: 0,
            }))
        };
        Self(Arc::new(object))
    }

    /// 返回对象是否采用不可变日期操作。
    pub fn is_immutable(&self) -> bool {
        matches!(&*self.0, DateObject::Immutable(_))
    }

    /// 比较两个句柄的对象身份。
    pub fn same_object(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// 获取包含原始时区的日期快照。
    pub fn snapshot(&self) -> Result<InputDate> {
        match &*self.0 {
            DateObject::Immutable(value) => Ok(value.clone()),
            DateObject::Mutable(state) => state
                .read()
                .map(|state| state.value.clone())
                .map_err(|_| super::attribute_store::state_error()),
        }
    }

    pub(super) fn revision(&self) -> Result<u64> {
        match &*self.0 {
            DateObject::Immutable(_) => Ok(0),
            DateObject::Mutable(state) => state
                .read()
                .map(|state| state.revision)
                .map_err(|_| super::attribute_store::state_error()),
        }
    }

    // 只保留绑定登记，不持有锁跨 SQL I/O；取消或失败也会释放登记。
    pub(super) fn pin_for_binding(&self) -> Result<Option<DateBinding>> {
        let DateObject::Mutable(state) = &*self.0 else {
            return Ok(None);
        };
        let mut state = state
            .write()
            .map_err(|_| super::attribute_store::state_error())?;
        state.sql_bindings = state
            .sql_bindings
            .checked_add(1)
            .ok_or_else(super::attribute_store::state_error)?;
        Ok(Some(DateBinding { date: self.clone() }))
    }

    /// 复制时刻、时区和可变类别，产生新对象身份。
    pub fn copy(&self) -> Result<Self> {
        Ok(Self::new(self.snapshot()?, self.is_immutable()))
    }

    /// 根据日期快照计算新值。并发修改过原对象时拒绝提交旧结果。
    ///
    /// 日期参与正在执行的 SQL 绑定时拒绝修改。转换闭包在锁外运行，失败时保留原日期。
    pub fn modify(&self, operation: impl FnOnce(InputDate) -> Result<InputDate>) -> Result<Self> {
        let DateObject::Mutable(state) = &*self.0 else {
            return Ok(Self::new(operation(self.snapshot()?)?, true));
        };
        let (value, revision) = {
            let state = state
                .read()
                .map_err(|_| super::attribute_store::state_error())?;
            if state.sql_bindings != 0 {
                return Err(date_binding_conflict());
            }
            (state.value.clone(), state.revision)
        };
        let value = operation(value)?;
        let mut state = state
            .write()
            .map_err(|_| super::attribute_store::state_error())?;
        if state.sql_bindings != 0 {
            return Err(date_binding_conflict());
        }
        if revision != state.revision {
            return Err(super::attribute_store::read_conflict(
                "model_date_update_conflict",
            ));
        }
        state.revision = revision
            .checked_add(1)
            .ok_or_else(super::attribute_store::state_error)?;
        state.value = value;
        Ok(self.clone())
    }
}

fn date_binding_conflict() -> crate::Error {
    super::attribute_store::read_conflict("model_date_binding_pending")
}

impl fmt::Debug for ModelDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelDate")
            .field("immutable", &self.is_immutable())
            .finish_non_exhaustive()
    }
}

pub(super) fn object_projection_required() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "model_attribute_object_projection_required",
        "模型对象需要显式投影",
    )
}

pub(super) fn from_json(values: Map<String, Value>) -> AttributeMap {
    values
        .into_iter()
        .map(|(name, value)| (name, value.into()))
        .collect()
}

pub(super) fn into_json(values: AttributeMap) -> Result<Map<String, Value>> {
    values
        .into_iter()
        .map(|(name, value)| Ok((name, value.try_into_json()?)))
        .collect()
}

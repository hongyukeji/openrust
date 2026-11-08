//! 由类转换持有的共享模型对象。
use crate::{Result, support::DataObject};
use serde_json::Value;
use std::{
    fmt,
    sync::{Arc, RwLock},
};

/// 保留类型与身份的模型对象。修改时替换有类型快照，避免持锁运行应用代码。
#[derive(Clone)]
pub struct ModelObject(Arc<RwLock<State>>);

struct State {
    value: Arc<dyn DataObject>,
    revision: u64,
}

impl ModelObject {
    /// 创建由模型持有的对象。对象内部应通过本句柄的修改入口更新。
    pub fn new(value: impl DataObject) -> Self {
        Self(Arc::new(RwLock::new(State {
            value: Arc::new(value),
            revision: 0,
        })))
    }

    /// 读取可用 `DataObject::as_any` 下转的当前快照。
    pub fn snapshot(&self) -> Result<Arc<dyn DataObject>> {
        self.0
            .read()
            .map(|state| Arc::clone(&state.value))
            .map_err(|_| super::attribute_store::state_error())
    }

    /// 判断两个句柄是否共享同一对象。
    pub fn same_object(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// 在锁外计算新快照；已有并发修改时拒绝覆盖。
    pub fn modify<T: DataObject>(&self, update: impl FnOnce(&T) -> Result<T>) -> Result<()> {
        let (value, revision) = {
            let state = self
                .0
                .read()
                .map_err(|_| super::attribute_store::state_error())?;
            (Arc::clone(&state.value), state.revision)
        };
        let value = value.as_any().downcast_ref::<T>().ok_or_else(|| {
            crate::Error::custom("model_object_type_mismatch", "对象快照与请求类型不一致")
        })?;
        let next = Arc::new(update(value)?);
        let retired = {
            let mut state = self
                .0
                .write()
                .map_err(|_| super::attribute_store::state_error())?;
            if state.revision != revision {
                return Err(super::attribute_store::read_conflict(
                    "model_object_update_conflict",
                ));
            }
            state.revision = revision
                .checked_add(1)
                .ok_or_else(super::attribute_store::state_error)?;
            std::mem::replace(&mut state.value, next)
        };
        drop(retired);
        Ok(())
    }

    pub(super) fn revision(&self) -> Result<u64> {
        self.0
            .read()
            .map(|state| state.revision)
            .map_err(|_| super::attribute_store::state_error())
    }

    pub(super) fn project_json(&self) -> Result<Value> {
        self.snapshot()?
            .json_projection()
            .ok_or_else(super::attribute_value::object_projection_required)
    }
}

impl fmt::Debug for ModelObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ModelObject([REDACTED])")
    }
}

//! 工厂显式实参与构造覆盖帧共用的异构值载体。

#![forbid(unsafe_code)]

use std::{
    any::Any,
    fmt,
    sync::{Arc, Mutex},
};

use super::{container_error, lock};
use crate::foundation::lifecycle::{CallbackDisposal, TaskExecutionFailure, disposal};
use crate::{Error, Result};

/// 调用者显式选择的参数键，不执行 PHP 的隐式键类型转换。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParameterKey {
    /// 构造参数或工厂参数的精确名称。
    Name(Arc<str>),
    /// 工厂可观察的整数索引；不隐式作为构造参数名称的替代。
    Index(i64),
}

impl ParameterKey {
    /// 创建精确名称，不裁剪空白或转换数字文本。
    #[must_use]
    pub fn name(name: impl Into<Arc<str>>) -> Self {
        Self::Name(name.into())
    }
}

/// 一个显式 null 或无需序列化、复制的类型化参数值。
///
/// `Arc<dyn Trait>` 可作为 `new(arc)` 的值保存，再按同一
/// `Arc<dyn Trait>` 类型读取；本载体不执行运行期 trait 投影。
#[derive(Clone)]
pub struct ParameterValue {
    value: Option<ParameterPayload>,
}

#[derive(Clone)]
enum ParameterPayload {
    Ordinary(Arc<dyn Any + Send + Sync>),
    Service(super::Value),
}

impl ParameterValue {
    /// 创建显式 null，区别于参数集合中没有该键。
    #[must_use]
    pub const fn null() -> Self {
        Self { value: None }
    }

    /// 持有任意线程安全值，不要求值实现 Clone 或 Serialize。
    #[must_use]
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self::from_arc(Arc::new(value))
    }

    /// 复用已有具体类型 Arc，不复制其内部对象。
    #[must_use]
    pub fn from_arc<T: Any + Send + Sync>(value: Arc<T>) -> Self {
        Self {
            value: Some(ParameterPayload::Ordinary(value)),
        }
    }

    /// 用作类实参，保留可供属性回调匹配的服务类型。
    #[must_use]
    pub fn service<T: ?Sized + Send + Sync + 'static>(value: Arc<T>) -> Self {
        Self::from_erased(super::erase(value))
    }

    pub(super) fn from_erased(value: super::Value) -> Self {
        Self {
            value: Some(ParameterPayload::Service(value)),
        }
    }

    pub(super) fn service_value(&self) -> Option<&super::Value> {
        match self.value.as_ref()? {
            ParameterPayload::Service(value) => Some(value),
            ParameterPayload::Ordinary(_) => None,
        }
    }

    fn erased(&self) -> Option<&Arc<dyn Any + Send + Sync>> {
        Some(match self.value.as_ref()? {
            ParameterPayload::Ordinary(value) => value,
            ParameterPayload::Service(value) => value.erased(),
        })
    }

    /// 判断值是否为显式 null。
    #[must_use]
    pub const fn is_null(&self) -> bool {
        self.value.is_none()
    }

    pub(super) fn downcast_ref<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.erased()?.downcast_ref()
    }

    /// 按声明类型读取同一对象；null 与错误类型分别返回明确错误。
    ///
    /// 不执行转换、默认值、依赖解析或输出对象的调试内容。
    pub fn downcast<T: Any + Send + Sync>(&self) -> Result<Arc<T>> {
        let Some(value) = self.erased() else {
            return Err(Error::custom(
                "container_parameter_null",
                "显式 null 参数不能读取为非空对象",
            ));
        };
        Arc::clone(value).downcast::<T>().map_err(|_| {
            Error::custom(
                "container_parameter_type_mismatch",
                "参数的声明类型与请求读取类型不同",
            )
        })
    }
}

impl fmt::Debug for ParameterValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(if self.is_null() {
            "ParameterValue::Null"
        } else {
            "ParameterValue::Value(<redacted>)"
        })
    }
}

/// 保留位置及异构值的调用参数快照。
///
///
/// 同一精确键后写覆盖值并保留原位置。克隆共享参数对象，集合修改彼此隔离。
/// 创建参数集合不会改变容器状态。
#[derive(Clone, Debug, Default)]
pub struct ParameterOverrides {
    pub(super) entries: Vec<(ParameterKey, ParameterValue)>,
}

impl ParameterOverrides {
    /// 创建空实参集合。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 覆盖同键值或追加新项；旧值交还调用方，避免隐藏的提前析构。
    pub fn insert(&mut self, key: ParameterKey, value: ParameterValue) -> Option<ParameterValue> {
        if let Some((_, previous)) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| existing == &key)
        {
            return Some(std::mem::replace(previous, value));
        }
        self.entries.push((key, value));
        None
    }

    /// 读取精确键。缺项返回 `None`，显式 null 返回 `Some`。
    #[must_use]
    pub fn get(&self, key: &ParameterKey) -> Option<&ParameterValue> {
        self.entries
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value)
    }

    /// 按输入位置借用全部键值，不暴露可变内部集合。
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&ParameterKey, &ParameterValue)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    /// 返回不同精确键的数量。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 判断集合是否没有参数。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 解析器克隆只持有读取能力。调用结束时，仍存活的解析器不再读取原调用参数。
#[derive(Clone)]
pub(super) struct ParameterView(Arc<Mutex<Option<ParameterOverrides>>>);

impl ParameterView {
    pub(super) fn get_named(&self, name: &str) -> Option<ParameterValue> {
        lock(&self.0)
            .as_ref()?
            .iter()
            .find_map(|(key, value)| match key {
                ParameterKey::Name(key) if key.as_ref() == name => Some(value.clone()),
                _ => None,
            })
    }

    pub(super) fn snapshot(&self) -> ParameterOverrides {
        lock(&self.0).clone().unwrap_or_default()
    }
}

/// 单次调用拥有帧的退役责任，完成或取消时都会先关闭读取，再锁外逐值释放。
pub(super) struct ParameterFrame {
    view: ParameterView,
    disposal: CallbackDisposal,
}

impl ParameterFrame {
    pub(super) fn new(values: ParameterOverrides, disposal: CallbackDisposal) -> Self {
        Self {
            view: ParameterView(Arc::new(Mutex::new(Some(values)))),
            disposal,
        }
    }

    pub(super) fn set_disposal(&mut self, disposal: CallbackDisposal) {
        self.disposal = disposal;
    }

    pub(super) fn view(&self) -> ParameterView {
        self.view.clone()
    }

    pub(super) fn finish<T>(self, result: Result<T>) -> Result<T> {
        let Some(values) = self.take() else {
            return result;
        };
        self.disposal.run(|| {
            Self::dispose_values(values);
            if !disposal::seen() {
                return result;
            }
            let cleanup = container_error("resource_task_interrupted", "调用参数退役包含异常");
            match result {
                Err(primary) => Err(TaskExecutionFailure::error(primary, cleanup)),
                Ok(value) => {
                    disposal::dispose(value, None);
                    Err(cleanup)
                }
            }
        })
    }

    fn take(&self) -> Option<ParameterOverrides> {
        lock(&self.view.0).take()
    }

    fn dispose_values(values: ParameterOverrides) {
        for (_, value) in values.entries {
            disposal::dispose(value, None);
        }
    }
}

impl Drop for ParameterFrame {
    fn drop(&mut self) {
        if let Some(values) = self.take() {
            self.disposal.run(|| Self::dispose_values(values));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ParameterFrame, ParameterKey, ParameterOverrides, ParameterValue, ParameterView};
    use crate::foundation::lifecycle::{CallbackDisposal, TaskExecutionFailure};
    use crate::{Error, Result};
    use std::error::Error as _;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn frame_retirement_closes_views_before_each_value_drop() -> Result<()> {
        struct ObserveClose {
            reader: Arc<Mutex<Option<ParameterView>>>,
            drops: Arc<AtomicUsize>,
            interrupt: bool,
        }

        impl Drop for ObserveClose {
            fn drop(&mut self) {
                let view = super::lock(&self.reader);
                if view
                    .as_ref()
                    .is_some_and(|view| view.0.try_lock().is_ok_and(|values| values.is_none()))
                {
                    self.drops.fetch_add(1, Ordering::AcqRel);
                }
                if self.interrupt {
                    std::panic::resume_unwind(Box::new("参数析构探针"));
                }
            }
        }

        for finish_with_error in [false, true] {
            let reader = Arc::new(Mutex::new(None));
            let drops = Arc::new(AtomicUsize::new(0));
            let mut values = ParameterOverrides::new();
            for (name, interrupt) in [("first", true), ("second", false)] {
                values.insert(
                    ParameterKey::name(name),
                    ParameterValue::new(ObserveClose {
                        reader: Arc::clone(&reader),
                        drops: Arc::clone(&drops),
                        interrupt,
                    }),
                );
            }
            let frame = ParameterFrame::new(values, CallbackDisposal::new(None, None));
            let view = frame.view();
            *super::lock(&reader) = Some(view.clone());
            assert!(view.get_named("first").is_some());
            if finish_with_error {
                let Err(error) = frame.finish::<()>(Err(Error::custom("test_primary", "原始错误")))
                else {
                    return Err(Error::custom(
                        "test_primary_missing",
                        "清理失败必须保留原始错误",
                    ));
                };
                let cause = error
                    .source()
                    .and_then(|source| source.downcast_ref::<TaskExecutionFailure>());
                assert!(
                    cause.is_some_and(|cause| cause.primary.code() == "test_primary"
                        && cause.cleanup.code() == "resource_task_interrupted")
                );
            } else {
                drop(frame);
            }
            assert_eq!(drops.load(Ordering::Acquire), 2);
            assert!(view.snapshot().is_empty());
        }
        Ok(())
    }

    #[test]
    fn explicit_null_is_present_without_default_or_index_fallback() {
        let mut values = ParameterOverrides::new();
        assert!(values.is_empty());
        values.insert(ParameterKey::name("port"), ParameterValue::null());
        values.insert(ParameterKey::Index(0), ParameterValue::new(80_u16));
        assert!(
            values
                .get(&ParameterKey::name("port"))
                .is_some_and(ParameterValue::is_null)
        );
        assert!(values.get(&ParameterKey::name("missing")).is_none());
        assert!(values.get(&ParameterKey::name("0")).is_none());
        assert_eq!(
            ParameterValue::null()
                .downcast::<u16>()
                .err()
                .map(|e| e.code().to_owned()),
            Some("container_parameter_null".to_owned())
        );
    }

    #[test]
    fn duplicate_keys_keep_first_position_and_mixed_keys_stay_distinct() -> Result<()> {
        let mut values = ParameterOverrides::new();
        let name = ParameterKey::name("0");
        assert!(
            values
                .insert(name.clone(), ParameterValue::new(1_u8))
                .is_none()
        );
        values.insert(ParameterKey::Index(0), ParameterValue::new(2_u8));
        values.insert(ParameterKey::Index(-1), ParameterValue::new(3_u8));
        let previous = values.insert(name.clone(), ParameterValue::new(4_u8));
        assert_eq!(
            *previous
                .ok_or_else(|| crate::Error::custom("missing", "缺少旧参数"))?
                .downcast::<u8>()?,
            1
        );
        assert_eq!(values.len(), 3);
        assert_eq!(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), *value.downcast::<u8>()?)))
                .collect::<Result<Vec<_>>>()?,
            vec![
                (name.clone(), 4),
                (ParameterKey::Index(0), 2),
                (ParameterKey::Index(-1), 3)
            ]
        );
        assert_eq!(
            *values
                .get(&name)
                .ok_or_else(|| crate::Error::custom("missing", "缺少参数"))?
                .downcast::<u8>()?,
            4
        );
        Ok(())
    }

    #[test]
    fn non_clone_objects_and_trait_values_share_exact_arc_without_conversion() -> Result<()> {
        struct Object(u64);
        trait Service: Send + Sync {
            fn value(&self) -> u64;
        }
        impl Service for Object {
            fn value(&self) -> u64 {
                self.0
            }
        }
        let object = Arc::new(Object(7));
        let value = ParameterValue::from_arc(Arc::clone(&object));
        assert!(Arc::ptr_eq(&object, &value.downcast::<Object>()?));
        let service: Arc<dyn Service> = object;
        let erased = ParameterValue::new(Arc::clone(&service));
        let restored = erased.downcast::<Arc<dyn Service>>()?;
        assert!(Arc::ptr_eq(&service, restored.as_ref()));
        assert_eq!(restored.value(), 7);
        assert_eq!(
            value.downcast::<u64>().err().map(|e| e.code().to_owned()),
            Some("container_parameter_type_mismatch".to_owned())
        );
        Ok(())
    }

    #[test]
    fn cloned_parameter_maps_do_not_mutate_each_others_entries_or_debug_values() -> Result<()> {
        let secret = "private-token-must-not-appear";
        let mut first = ParameterOverrides::new();
        first.insert(
            ParameterKey::name("credential"),
            ParameterValue::new(secret.to_owned()),
        );
        let mut second = first.clone();
        second.insert(ParameterKey::name("credential"), ParameterValue::null());
        assert!(
            second
                .get(&ParameterKey::name("credential"))
                .is_some_and(ParameterValue::is_null)
        );
        let original = first
            .get(&ParameterKey::name("credential"))
            .ok_or_else(|| crate::Error::custom("missing", "缺少参数"))?;
        assert_eq!(original.downcast::<String>()?.as_str(), secret);
        assert!(!format!("{first:?}").contains(secret));
        let mismatch = original.downcast::<u8>().err().ok_or_else(|| {
            crate::Error::custom("test_parameter_type_error_missing", "跨类型读取应失败")
        })?;
        assert_eq!(mismatch.code(), "container_parameter_type_mismatch");
        assert!(!format!("{mismatch} {mismatch:?}").contains(secret));
        Ok(())
    }
}

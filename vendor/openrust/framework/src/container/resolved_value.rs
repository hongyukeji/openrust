//! 保留解析值的 Rust 类型和同一对象的已编译视图。

use super::{ServiceKey, Value};
use std::any::{Any, TypeId};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct ResolvedValue {
    erased: Arc<dyn Any + Send + Sync>,
    key: Option<ServiceKey>,
    object_address: Option<usize>,
    origin: Option<Value>,
}

impl std::fmt::Debug for ResolvedValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedValue")
            .finish_non_exhaustive()
    }
}

impl ResolvedValue {
    pub(super) fn erased(&self) -> &Arc<dyn Any + Send + Sync> {
        &self.erased
    }

    #[cfg(test)]
    pub(super) fn into_erased(self) -> Arc<dyn Any + Send + Sync> {
        self.erased
    }

    pub(super) fn type_id(&self) -> TypeId {
        self.erased.as_ref().type_id()
    }

    pub(super) fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.erased.downcast_ref::<T>()
    }

    pub(super) fn key(&self) -> Option<&ServiceKey> {
        self.key.as_ref()
    }

    pub(super) fn origin(&self) -> Option<&Value> {
        self.origin.as_ref()
    }
}

pub(super) fn erase<T: ?Sized + Send + Sync + 'static>(value: Arc<T>) -> Value {
    service_value(value, None)
}

pub(super) fn plain<T: Any + Send + Sync>(value: T) -> Value {
    Arc::new(ResolvedValue {
        erased: Arc::new(value),
        key: None,
        object_address: None,
        origin: None,
    })
}

pub(super) fn projected<T: ?Sized + Send + Sync + 'static>(
    source: &Value,
    projected: Arc<T>,
) -> Value {
    service_value(projected, Some(Arc::clone(source)))
}

fn service_value<T: ?Sized + Send + Sync + 'static>(value: Arc<T>, origin: Option<Value>) -> Value {
    // 地址仅用于同时存活的 Arc 之间的身份比较。
    let object_address = Arc::as_ptr(&value).cast::<()>().addr();
    Arc::new(ResolvedValue {
        erased: Arc::new(value),
        key: Some(ServiceKey::typed::<T>()),
        object_address: Some(object_address),
        origin,
    })
}

pub(super) fn after_extension(previous: &Value, mut next: Value) -> Value {
    if previous.object_address.is_some()
        && previous.object_address == next.object_address
        && next.origin.is_none()
    {
        let origin = if previous.key == next.key {
            previous.origin.clone()
        } else {
            Some(Arc::clone(previous))
        };
        if let Some(origin) = origin {
            Arc::make_mut(&mut next).origin = Some(origin);
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::{after_extension, erase};
    use crate::Result;
    use crate::class_projection;
    use crate::container::{ServiceKey, container_error};
    use std::fmt::Display;
    use std::sync::Arc;

    #[test]
    fn extender_retains_concrete_view_only_for_same_object() -> Result<()> {
        let source = erase(Arc::new(String::from("同一对象")));
        let projection = class_projection!(String => dyn Display + Send + Sync);
        let view = projection.project(&source)?;
        let visible = view
            .downcast_ref::<Arc<dyn Display + Send + Sync>>()
            .ok_or_else(|| container_error("test_missing_view", "缺少 trait 视图"))?;

        let unchanged = after_extension(&view, erase(Arc::clone(visible)));
        assert_eq!(
            unchanged.key(),
            Some(&ServiceKey::typed::<dyn Display + Send + Sync>())
        );
        assert!(
            unchanged
                .origin()
                .is_some_and(|origin| Arc::ptr_eq(origin, &source))
        );

        let replacement: Arc<dyn Display + Send + Sync> = Arc::new(String::from("新对象"));
        let changed = after_extension(&view, erase(replacement));
        assert!(changed.origin().is_none());
        Ok(())
    }
}

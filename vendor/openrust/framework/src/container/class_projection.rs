//! 编译期生成的同实例Arc投影；不执行服务查找或新建缓存。

use super::ServiceKey;
#[cfg(test)]
use crate::core::Error;
use crate::core::Result;
use std::{any::Any, sync::Arc};

type ErasedValue = Arc<dyn Any + Send + Sync>;
type Project = dyn Fn(&ErasedValue) -> Result<ErasedValue> + Send + Sync;

/// 已编译类的一种同对象输出视图，由内部 `class_projection!` 声明。
///
/// 本身不登记trait绑定，也不按某个trait自动选择实现类。
#[derive(Clone)]
pub(super) struct ClassProjection {
    source: ServiceKey,
    target: ServiceKey,
    project: Arc<Project>,
}

impl ClassProjection {
    /// 返回需要匹配描述符具体类身份的源键。
    #[must_use]
    pub(super) fn source_key(&self) -> &ServiceKey {
        &self.source
    }

    /// 返回构造前可检查的输出视图键。
    #[must_use]
    pub(super) fn target_key(&self) -> &ServiceKey {
        &self.target
    }

    /// 内部宏的构造桥。手动调用时也必须保留同一对象。
    ///
    /// `convert` 正常路径仅作直接Arc coercion；替换对象会在发布视图前失败。
    /// 不将此隐藏桥作为任意业务转换的公开扩展契约。
    #[cfg(test)]
    pub(super) fn __from_coercion<C, T>(convert: fn(Arc<C>) -> Arc<T>) -> Self
    where
        C: Any + Send + Sync,
        T: ?Sized + Send + Sync + 'static,
    {
        Self {
            source: ServiceKey::typed::<C>(),
            target: ServiceKey::typed::<T>(),
            project: Arc::new(move |erased| {
                let concrete = erased
                    .downcast_ref::<Arc<C>>()
                    .ok_or_else(invalid_projection)?;
                let projected = convert(Arc::clone(concrete));
                if Arc::as_ptr(concrete).cast::<()>() != Arc::as_ptr(&projected).cast::<()>() {
                    return Err(invalid_projection());
                }
                Ok(Arc::new(projected))
            }),
        }
    }

    /// 在同一Resolver构造流程中产生视图，不创建新服务身份。
    pub(super) fn project(&self, source: &ErasedValue) -> Result<ErasedValue> {
        (self.project)(source)
    }
}

#[cfg(test)]
fn invalid_projection() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "container_class_projection_invalid",
        "编译类投影的类型或对象身份不匹配",
    )
}

/// 为同一具体对象声明trait视图；仅生成Rust直接Arc coercion。
#[cfg(test)]
macro_rules! class_projection {
    ($concrete:ty => $view:ty) => {
        $crate::container::class_projection::ClassProjection::__from_coercion::<$concrete, $view>(
            |source: ::std::sync::Arc<$concrete>| {
                let projected: ::std::sync::Arc<$view> = source;
                projected
            },
        )
    };
}

#[cfg(test)]
pub(super) use class_projection;

#[cfg(test)]
mod tests {
    use super::{ClassProjection, ErasedValue};
    use crate::{Result, container::ServiceKey};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    trait View: Send + Sync {
        fn value(&self) -> usize;
    }
    struct Concrete(Arc<AtomicUsize>);
    impl View for Concrete {
        fn value(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn coercion_preserves_pointer_mutations_and_metadata() -> Result<()> {
        let calls = Arc::new(AtomicUsize::new(3));
        let concrete = Arc::new(Concrete(Arc::clone(&calls)));
        let erased: ErasedValue = Arc::new(Arc::clone(&concrete));
        let projection = class_projection!(Concrete => dyn View);
        assert_eq!(projection.source_key(), &ServiceKey::typed::<Concrete>());
        assert_eq!(projection.target_key(), &ServiceKey::typed::<dyn View>());
        let projected = projection.project(&erased)?;
        let Some(view) = projected.downcast_ref::<Arc<dyn View>>() else {
            return Err(super::invalid_projection());
        };
        let expected: Arc<dyn View> = Arc::clone(&concrete) as Arc<dyn View>;
        assert!(Arc::ptr_eq(view, &expected));
        calls.store(8, Ordering::SeqCst);
        assert_eq!(view.value(), 8);
        assert_eq!(concrete.value(), 8);
        Ok(())
    }

    #[test]
    fn hidden_bridge_cannot_replace_the_concrete_object() {
        let original = Arc::new(Concrete(Arc::new(AtomicUsize::new(3))));
        let erased: ErasedValue = Arc::new(original);
        let projection = ClassProjection::__from_coercion::<Concrete, dyn View>(|_| {
            Arc::new(Concrete(Arc::new(AtomicUsize::new(99))))
        });
        let result = projection.project(&erased);
        assert!(
            matches!(result, Err(ref error) if error.code() == "container_class_projection_invalid")
        );
    }

    #[test]
    fn wrong_concrete_type_never_calls_the_coercion() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let projection = ClassProjection::__from_coercion::<Concrete, dyn View>(|source| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            source
        });
        let wrong: ErasedValue = Arc::new(Arc::new(String::from("wrong")));
        assert!(projection.project(&wrong).is_err());
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    }
}

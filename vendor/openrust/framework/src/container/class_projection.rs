//! A compiled view of an existing Arc allocation.

use std::{future::Future, pin::Pin, sync::Arc};

use crate::contracts::container::BindingResolutionError;

use super::{Container, ParameterValue, RegisteredBinding, ResolutionOptions, ServiceKey};

type Result<T> = std::result::Result<T, BindingResolutionError>;
type Project = dyn Fn(&ParameterValue) -> Result<ParameterValue> + Send + Sync;
type Resolve =
    fn(&Container, ServiceKey, RegisteredBinding, ResolutionOptions) -> Result<ParameterValue>;
type ResolveFuture<'a> = Pin<Box<dyn Future<Output = Result<ParameterValue>> + Send + 'a>>;
type ResolveAsync = for<'a> fn(
    &'a Container,
    ServiceKey,
    RegisteredBinding,
    ResolutionOptions,
) -> ResolveFuture<'a>;

/// A class's compiled coercion to another typed service view.
#[derive(Clone)]
pub struct ClassProjection {
    pub(super) source: ServiceKey,
    pub(super) target: ServiceKey,
    project: Arc<Project>,
    resolve: Resolve,
    resolve_async: ResolveAsync,
}

impl ClassProjection {
    #[doc(hidden)]
    pub fn __from_coercion<C, T>(convert: fn(Arc<C>) -> Arc<T>) -> Self
    where
        C: Send + Sync + 'static,
        T: ?Sized + Send + Sync + 'static,
    {
        let source = ServiceKey::typed::<C>();
        let target = ServiceKey::typed::<T>();
        Self {
            source: source.clone(),
            target: target.clone(),
            project: Arc::new(move |value| {
                let concrete = value.downcast::<Arc<C>>()?;
                let projected = convert((*concrete).clone());
                if !std::ptr::addr_eq(Arc::as_ptr(concrete.as_ref()), Arc::as_ptr(&projected)) {
                    return Err(BindingResolutionError::InvalidProjection {
                        source: source.clone(),
                        target: target.clone(),
                    });
                }
                Ok(ParameterValue::new(projected))
            }),
            resolve: |container, key, entry, options| {
                container
                    .resolve_entry::<C>(key, entry, options)
                    .map(ParameterValue::new)
            },
            resolve_async: |container, key, entry, options| {
                Box::pin(async move {
                    container
                        .resolve_entry_async::<C>(key, entry, options)
                        .await
                        .map(ParameterValue::new)
                })
            },
        }
    }

    pub(super) fn view<T: ?Sized + Send + Sync + 'static>(
        &self,
        value: &ParameterValue,
    ) -> Result<Arc<T>> {
        (self.project)(value)?
            .downcast::<Arc<T>>()
            .map(|value| (*value).clone())
    }

    pub(super) fn resolve<T: ?Sized + Send + Sync + 'static>(
        &self,
        container: &Container,
        key: ServiceKey,
        entry: RegisteredBinding,
        options: ResolutionOptions,
    ) -> Result<Arc<T>> {
        self.view(&(self.resolve)(container, key, entry, options)?)
    }

    pub(super) async fn resolve_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        container: &Container,
        key: ServiceKey,
        entry: RegisteredBinding,
        options: ResolutionOptions,
    ) -> Result<Arc<T>> {
        self.view(&(self.resolve_async)(container, key, entry, options).await?)
    }
}

impl Container {
    pub(super) fn class_projections(
        &self,
        source: &ServiceKey,
    ) -> Option<Arc<Vec<ClassProjection>>> {
        self.registry
            .read()
            .expect("container lock poisoned")
            .classes
            .get(source)
            .and_then(|descriptor| descriptor.projections.clone())
    }

    pub(super) fn projection(
        &self,
        source: &ServiceKey,
        target: &ServiceKey,
    ) -> Option<ClassProjection> {
        self.class_projections(source)?
            .iter()
            .find(|projection| projection.target == *target)
            .cloned()
    }

    pub(super) fn required_projection<C, T>(&self) -> Result<ClassProjection>
    where
        C: Send + Sync + 'static,
        T: ?Sized + Send + Sync + 'static,
    {
        let source = ServiceKey::typed::<C>();
        let target = ServiceKey::typed::<T>();
        self.projection(&source, &target)
            .ok_or(BindingResolutionError::InvalidProjection { source, target })
    }

    pub(super) fn projection_for<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        entry: &RegisteredBinding,
    ) -> Result<ClassProjection> {
        let source = ServiceKey::from_type_id(entry.output_type, entry.output);
        self.projection(&source, &ServiceKey::typed::<T>())
            .ok_or_else(|| BindingResolutionError::TypeMismatch {
                service: key.clone(),
                expected: std::any::type_name::<T>(),
                actual: entry.output,
            })
    }
}

/// Declares a compiler-checked Arc coercion from a class to a service view.
#[macro_export]
macro_rules! class_projection {
    ($concrete:ty => $view:ty) => {
        $crate::container::ClassProjection::__from_coercion::<$concrete, $view>(
            |source: ::std::sync::Arc<$concrete>| {
                let view: ::std::sync::Arc<$view> = source;
                view
            },
        )
    };
}

use std::{
    any::{Any, type_name},
    cell::RefCell,
    collections::HashMap,
    future::Future,
    sync::Arc,
};

use crate::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};

struct Cached {
    value: Arc<dyn Any + Send + Sync>,
    output: &'static str,
}

struct Context {
    container: Arc<Container>,
    resolved: RefCell<HashMap<ServiceKey, Cached>>,
}

tokio::task_local! { static APPLICATION: Context; }

/// Resolves a facade's underlying service from the current application.
#[allow(
    non_snake_case,
    non_upper_case_globals,
    reason = "Matches Laravel's Facade API"
)]
pub trait Facade {
    type Service: ?Sized + Send + Sync + 'static;
    const cached: bool = true;

    fn getFacadeAccessor() -> ServiceKey;

    fn getFacadeRoot() -> Result<Arc<Self::Service>, BindingResolutionError> {
        APPLICATION
            .try_with(|context| {
                let key = Self::getFacadeAccessor();
                let cached = context
                    .resolved
                    .borrow()
                    .get(&key)
                    .map(|cached| (cached.value.clone(), cached.output));
                if let Some((value, actual)) = cached {
                    return value
                        .downcast::<Arc<Self::Service>>()
                        .map(|value| (*value).clone())
                        .map_err(|_| BindingResolutionError::TypeMismatch {
                            service: key,
                            expected: type_name::<Self::Service>(),
                            actual,
                        });
                }
                let service = context.container.get::<Self::Service>(key.clone())?;
                if Self::cached {
                    let previous = context.resolved.borrow_mut().insert(
                        key,
                        Cached {
                            value: Arc::new(service.clone()),
                            output: type_name::<Self::Service>(),
                        },
                    );
                    drop(previous);
                }
                Ok(service)
            })
            .map_err(|_| BindingResolutionError::FacadeNotSet)?
    }

    fn clearResolvedInstance(name: Option<ServiceKey>) -> Result<(), BindingResolutionError> {
        APPLICATION
            .try_with(|context| {
                let previous = context
                    .resolved
                    .borrow_mut()
                    .remove(&name.unwrap_or_else(Self::getFacadeAccessor));
                drop(previous);
            })
            .map_err(|_| BindingResolutionError::FacadeNotSet)
    }

    fn clearResolvedInstances() -> Result<(), BindingResolutionError> {
        APPLICATION
            .try_with(|context| {
                let previous = std::mem::take(&mut *context.resolved.borrow_mut());
                drop(previous);
            })
            .map_err(|_| BindingResolutionError::FacadeNotSet)
    }

    fn swap(instance: Arc<Self::Service>) -> Result<(), BindingResolutionError> {
        APPLICATION
            .try_with(|context| {
                let key = Self::getFacadeAccessor();
                let previous = context.resolved.borrow_mut().insert(
                    key.clone(),
                    Cached {
                        value: Arc::new(instance.clone()),
                        output: type_name::<Self::Service>(),
                    },
                );
                drop(previous);
                context.container.instance_key(key, instance)
            })
            .map_err(|_| BindingResolutionError::FacadeNotSet)?
    }
}

pub(crate) async fn scope<F: Future>(container: Arc<Container>, future: F) -> F::Output {
    let scoped = container.clone();
    scoped
        .scope(async move {
            if APPLICATION
                .try_with(|context| Arc::ptr_eq(&context.container, &container))
                .unwrap_or(false)
            {
                future.await
            } else {
                APPLICATION
                    .scope(
                        Context {
                            container,
                            resolved: RefCell::default(),
                        },
                        future,
                    )
                    .await
            }
        })
        .await
}

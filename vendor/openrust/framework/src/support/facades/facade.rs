use crate::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
    testing::FacadeSpy,
};
use std::{
    any::{Any, type_name},
    collections::HashMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

struct Cached {
    value: Arc<dyn Any + Send + Sync>,
    output: &'static str,
    provisional: Option<ServiceKey>,
}
struct Context {
    identity: Arc<()>,
    container: Arc<Container>,
    resolved: Mutex<HashMap<ServiceKey, Cached>>,
    closed: AtomicBool,
    database: crate::database::ConnectionScope,
    changed: tokio::sync::watch::Sender<bool>,
}
tokio::task_local! {static APPLICATION:Option<Arc<Context>>;}
impl Context {
    fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.changed.send_replace(true);
            let retired =
                std::mem::take(&mut *self.resolved.lock().expect("facade cache poisoned"));
            APPLICATION.sync_scope(None, || drop(retired));
        }
    }
}
struct CloseContext(Arc<Context>);
impl Drop for CloseContext {
    fn drop(&mut self) {
        self.0.close();
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        self.close();
    }
}

fn with_context<R>(
    operation: impl FnOnce(&Context) -> Result<R, BindingResolutionError>,
) -> Result<R, BindingResolutionError> {
    let context = APPLICATION
        .try_with(Clone::clone)
        .map_err(|_| BindingResolutionError::FacadeNotSet)?
        .ok_or(BindingResolutionError::FacadeNotSet)?;
    if context.closed.load(Ordering::Acquire) {
        return Err(BindingResolutionError::FacadeNotSet);
    }
    operation(context.as_ref())
}
pub(crate) fn current_container() -> Option<Arc<Container>> {
    with_context(|context| Ok(context.container.clone())).ok()
}

pub(crate) fn database_routing(
    origin: &Arc<crate::database::ReadRouting>,
) -> Option<Arc<crate::database::ReadRouting>> {
    with_context(|context| Ok(context.database.state(origin))).ok()
}

#[derive(Clone)]
pub(crate) struct ContextSnapshot(Option<Arc<Context>>);
pub(crate) fn context_snapshot() -> ContextSnapshot {
    ContextSnapshot(APPLICATION.try_with(Clone::clone).ok().flatten())
}
/// 不延长请求／应用生命周期的模板对象身份凭据。
#[derive(Clone)]
pub(crate) struct WeakContextSnapshot(Option<std::sync::Weak<Context>>);
impl WeakContextSnapshot {
    pub(crate) fn is_current(&self) -> bool {
        match &self.0 {
            None => true,
            Some(context) => context.upgrade().is_some_and(|origin| {
                !origin.closed.load(Ordering::Acquire)
                    && APPLICATION
                        .try_with(|current| {
                            current
                                .as_ref()
                                .is_some_and(|current| Arc::ptr_eq(&origin, current))
                        })
                        .unwrap_or(false)
            }),
        }
    }
}

impl ContextSnapshot {
    pub(crate) fn downgrade(&self) -> WeakContextSnapshot {
        WeakContextSnapshot(self.0.as_ref().map(Arc::downgrade))
    }
    pub(crate) fn empty() -> Self {
        Self(None)
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|context| context.closed.load(Ordering::Acquire))
    }
    pub(crate) fn subscribe(&self) -> Option<tokio::sync::watch::Receiver<bool>> {
        self.0.as_ref().map(|context| context.changed.subscribe())
    }
    pub(crate) fn matches_current(&self) -> bool {
        match &self.0 {
            Some(origin) => APPLICATION
                .try_with(|current| {
                    current
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(origin, current))
                })
                .unwrap_or(false),
            None => true,
        }
    }
    pub(crate) fn run<R>(&self, operation: impl FnOnce() -> R) -> R {
        APPLICATION.sync_scope(
            self.0
                .clone()
                .filter(|context| !context.closed.load(Ordering::Acquire)),
            operation,
        )
    }
}
pub(crate) fn context_identity() -> Option<Arc<()>> {
    APPLICATION
        .try_with(|context| {
            context
                .as_ref()
                .filter(|context| !context.closed.load(Ordering::Acquire))
                .map(|context| context.identity.clone())
        })
        .ok()
        .flatten()
}

/// Resolves a facade's underlying service from the current application.
pub trait Facade {
    type Service: ?Sized + Send + Sync + 'static;
    const CACHED: bool = true;

    /// 在服务已解析时立即执行，并观察所属应用后续的实际解析。
    fn resolved(
        callback: impl Fn(Arc<Self::Service>, &Container) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        let container = with_context(|context| Ok(context.container.clone()))?;
        let key = Self::get_facade_accessor();
        let callback = Arc::new(callback);
        if container.resolved(key.clone())? {
            callback(Self::get_facade_root()?, &container)?;
        }
        container.after_resolving(Some(key), move |event, container| {
            let service = event.value::<Self::Service>().ok_or_else(|| {
                BindingResolutionError::TypeMismatch {
                    service: event.key().clone(),
                    expected: type_name::<Self::Service>(),
                    actual: "解析回调中的服务类型不匹配",
                }
            })?;
            callback(service, container)
        })
    }

    /// 读取当前作用域的应用；尚未进入或已清空应用作用域时返回 None。
    fn get_facade_application()
    -> Result<Option<Arc<crate::foundation::Application>>, BindingResolutionError> {
        current_container()
            .map(|_| <super::App as Facade>::get_facade_root())
            .transpose()
    }

    /// 在给定 Future 内切换应用，结束或取消后恢复外层作用域，不修改其他任务。
    fn set_facade_application<F: Future>(
        application: Option<&crate::foundation::Application>,
        operation: F,
    ) -> impl Future<Output = F::Output> {
        // 对外作用域Future保持有界，避免复杂消费者的内联布局耗尽编译深度或线程栈。
        let operation = Box::pin(operation);
        async move {
            match application {
                Some(application) => scope(application.container_handle(), operation).await,
                None => APPLICATION.scope(None, operation).await,
            }
        }
    }

    /// 只检查已缓存的门面替身，不触发服务解析；HTTP/Process 的桩注册不等于 Fake 根。
    fn is_fake() -> Result<bool, BindingResolutionError> {
        let cached = match with_context(|context| {
            Ok(context
                .resolved
                .lock()
                .expect("facade cache poisoned")
                .get(&Self::get_facade_accessor())
                .map(|cached| (cached.value.clone(), cached.output)))
        }) {
            Err(BindingResolutionError::FacadeNotSet) => return Ok(false),
            result => result?,
        };
        let Some((value, actual)) = cached else {
            return Ok(false);
        };
        let service = value.downcast::<Arc<Self::Service>>().map_err(|_| {
            BindingResolutionError::TypeMismatch {
                service: Self::get_facade_accessor(),
                expected: type_name::<Self::Service>(),
                actual,
            }
        })?;
        Ok(Self::is_fake_root(service.as_ref().as_ref()))
    }

    /// 具体门面据其实际服务识别 Fake；编译型替身可覆盖此类型安全钩子。
    #[doc(hidden)]
    fn is_fake_root(_service: &Self::Service) -> bool {
        false
    }

    fn get_facade_accessor() -> ServiceKey;
    fn get_facade_root() -> Result<Arc<Self::Service>, BindingResolutionError> {
        with_context(|context| {
            let key = Self::get_facade_accessor();
            if !Self::CACHED {
                return context.container.get::<Self::Service>(key);
            }
            let cached = context
                .resolved
                .lock()
                .expect("facade cache poisoned")
                .get(&key)
                .map(|cached| {
                    (
                        cached.value.clone(),
                        cached.output,
                        cached.provisional.clone(),
                    )
                });
            if let Some((value, actual, provisional)) = cached {
                if let Some(key) = provisional {
                    context.container.check_provisional_cache(&key)?;
                }
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
            let provisional = context.container.provisional_cache_key(&key)?;
            let previous = context
                .resolved
                .lock()
                .expect("facade cache poisoned")
                .insert(
                    key,
                    Cached {
                        value: Arc::new(service.clone()),
                        output: type_name::<Self::Service>(),
                        provisional,
                    },
                );
            drop(previous);
            Ok(service)
        })
    }
    /// Await the underlying service without overwriting a newer cache entry or scope.
    fn get_facade_root_async()
    -> impl Future<Output = Result<Arc<Self::Service>, BindingResolutionError>> + Send {
        async {
            let identity = context_identity().ok_or(BindingResolutionError::FacadeNotSet)?;
            let mut lookup = std::pin::pin!(async {
                let key = Self::get_facade_accessor();
                let (container, cached) = with_context(|context| {
                    let cached = if Self::CACHED {
                        context
                            .resolved
                            .lock()
                            .expect("facade cache poisoned")
                            .get(&key)
                            .map(|cached| {
                                (
                                    cached.value.clone(),
                                    cached.output,
                                    cached.provisional.clone(),
                                )
                            })
                    } else {
                        None
                    };
                    Ok((context.container.clone(), cached))
                })?;
                if let Some((value, actual, provisional)) = cached {
                    match provisional.map_or(Ok(()), |key| container.check_provisional_cache(&key))
                    {
                        Ok(()) => {
                            return value
                                .downcast::<Arc<Self::Service>>()
                                .map(|value| (*value).clone())
                                .map_err(|_| BindingResolutionError::TypeMismatch {
                                    service: key,
                                    expected: type_name::<Self::Service>(),
                                    actual,
                                });
                        }
                        Err(
                            BindingResolutionError::DeferredServiceRequiresAsync { .. }
                            | BindingResolutionError::AsyncResolutionRequired { .. },
                        ) => {}
                        Err(error) => return Err(error),
                    }
                }
                let service = container.get_async::<Self::Service>(key.clone()).await?;
                with_context(|context| {
                    if !Arc::ptr_eq(&container, &context.container) {
                        return Err(BindingResolutionError::FacadeNotSet);
                    }
                    if Self::CACHED {
                        let mut cache = context.resolved.lock().expect("facade cache poisoned");
                        cache.entry(key).or_insert_with(|| Cached {
                            value: Arc::new(service.clone()),
                            output: type_name::<Self::Service>(),
                            provisional: None,
                        });
                    }
                    Ok(service)
                })
            });
            std::future::poll_fn(move |cx| {
                if !context_identity().is_some_and(|current| Arc::ptr_eq(&identity, &current)) {
                    return std::task::Poll::Ready(Err(BindingResolutionError::FacadeNotSet));
                }
                lookup.as_mut().poll(cx)
            })
            .await
        }
    }
    fn clear_resolved_instance(name: Option<ServiceKey>) -> Result<(), BindingResolutionError> {
        with_context(|context| {
            let previous = context
                .resolved
                .lock()
                .expect("facade cache poisoned")
                .remove(&name.unwrap_or_else(Self::get_facade_accessor));
            drop(previous);
            Ok(())
        })
    }
    fn clear_resolved_instances() -> Result<(), BindingResolutionError> {
        with_context(|context| {
            let previous =
                std::mem::take(&mut *context.resolved.lock().expect("facade cache poisoned"));
            drop(previous);
            Ok(())
        })
    }
    fn swap(instance: Arc<Self::Service>) -> Result<(), BindingResolutionError> {
        with_context(|context| {
            let key = Self::get_facade_accessor();
            let provisional = context.container.provisional_cache_key(&key).ok().flatten();
            let previous = context
                .resolved
                .lock()
                .expect("facade cache poisoned")
                .insert(
                    key.clone(),
                    Cached {
                        value: Arc::new(instance.clone()),
                        output: type_name::<Self::Service>(),
                        provisional,
                    },
                );
            drop(previous);
            context.container.instance_key(key, instance)
        })
    }

    /// Installs a typed replacement that records calls on behalf of this facade.
    fn spy<A: Send + Sync + 'static>(
        replacement: impl FnOnce(FacadeSpy<A>) -> Arc<Self::Service>,
    ) -> Result<FacadeSpy<A>, BindingResolutionError> {
        with_context(|_| Ok(()))?;
        let spy = FacadeSpy::new();
        Self::swap(replacement(spy.clone()))?;
        Ok(spy)
    }
}
pub(crate) fn application_scope_sync<R>(
    container: Arc<Container>,
    operation: impl FnOnce() -> R,
) -> R {
    if APPLICATION
        .try_with(|current| {
            current.as_ref().is_some_and(|context| {
                !context.closed.load(Ordering::Acquire)
                    && Arc::ptr_eq(&context.container, &container)
            })
        })
        .unwrap_or(false)
    {
        operation()
    } else {
        let (context, _close) = open_context(container);
        APPLICATION.sync_scope(Some(context), operation)
    }
}

pub(crate) async fn application_scope<F: Future>(
    container: Arc<Container>,
    future: F,
) -> F::Output {
    if APPLICATION
        .try_with(|current| {
            current.as_ref().is_some_and(|context| {
                !context.closed.load(Ordering::Acquire)
                    && Arc::ptr_eq(&context.container, &container)
            })
        })
        .unwrap_or(false)
    {
        future.await
    } else {
        let (context, _close) = open_context(container);
        APPLICATION.scope(Some(context), future).await
    }
}

fn open_context(container: Arc<Container>) -> (Arc<Context>, CloseContext) {
    let context = Arc::new(Context {
        identity: Arc::new(()),
        container,
        resolved: Mutex::default(),
        closed: AtomicBool::new(false),
        database: Default::default(),
        changed: tokio::sync::watch::channel(false).0,
    });
    (context.clone(), CloseContext(context))
}

pub(crate) fn isolated_scope<F: Future>(
    container: Arc<Container>,
    future: F,
) -> impl Future<Output = F::Output> {
    // 在构造异步状态机前固定业务Future布局，避免嵌套作用域复制。
    let future = Box::pin(future);
    async move {
        let (context, _close) = open_context(container.clone());
        container
            .isolated_scope(APPLICATION.scope(Some(context), future))
            .await
    }
}

pub(crate) async fn scope<F: Future>(container: Arc<Container>, future: F) -> F::Output {
    container
        .scope(application_scope(container.clone(), future))
        .await
}

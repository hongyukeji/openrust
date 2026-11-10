//! Async factories use the same registry, cache states, decorators and resolution events.
use super::{Container, scope};
use crate::contracts::container::BindingResolutionError;
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::Arc,
    thread::{self, ThreadId},
};

pub type FactoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<Arc<T>, BindingResolutionError>> + Send + 'a>>;
pub(super) type AsyncFactory<T> =
    dyn for<'a> Fn(&'a Container) -> FactoryFuture<'a, T> + Send + Sync;

#[derive(Clone, Default)]
pub(super) struct AsyncOwner(Arc<Operation>);
#[derive(Default)]
struct Operation {
    failure: Mutex<Option<Arc<BindingResolutionError>>>,
}
impl PartialEq for AsyncOwner {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for AsyncOwner {}
#[derive(Debug)]
struct SharedError(Arc<BindingResolutionError>);
impl std::fmt::Display for SharedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for SharedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}
fn duplicate(error: &Arc<BindingResolutionError>) -> BindingResolutionError {
    use BindingResolutionError::*;
    match error.as_ref() {
        FacadeNotSet => FacadeNotSet,
        ScopeClosed => ScopeClosed,
        ParameterTypeMismatch { expected, actual } => ParameterTypeMismatch {
            expected,
            actual: *actual,
        },
        AsyncResolutionRequired { service } => AsyncResolutionRequired {
            service: service.clone(),
        },
        DeferredServiceRequiresAsync { service } => DeferredServiceRequiresAsync {
            service: service.clone(),
        },
        DeferredProviderFailed { provider, source } => DeferredProviderFailed {
            provider,
            source: source.clone(),
        },
        NotBound { service } => NotBound {
            service: service.clone(),
        },
        CircularDependency { services } => CircularDependency {
            services: services.clone(),
        },
        AliasToSelf { service } => AliasToSelf {
            service: service.clone(),
        },
        CircularAlias { services } => CircularAlias {
            services: services.clone(),
        },
        TypeMismatch {
            service,
            expected,
            actual,
        } => TypeMismatch {
            service: service.clone(),
            expected,
            actual,
        },
        FactoryFailed { service, .. } => FactoryFailed {
            service,
            source: Box::new(SharedError(error.clone())),
        },
    }
}
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Owner {
    Thread(ThreadId),
    Async(AsyncOwner),
}
// WaitEdge owns both operations while their immutable pointer keys are in the graph.
// The addresses therefore cannot be reused until the corresponding edges are removed.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum OwnerKey {
    Thread(ThreadId),
    Async(usize),
}
thread_local! { static OWNERS: RefCell<Vec<AsyncOwner>> = const { RefCell::new(Vec::new()) }; }
impl Owner {
    pub(super) fn key(&self) -> OwnerKey {
        match self {
            Self::Thread(id) => OwnerKey::Thread(*id),
            Self::Async(owner) => OwnerKey::Async(Arc::as_ptr(&owner.0).addr()),
        }
    }
    fn failed(&self) -> Option<BindingResolutionError> {
        match self {
            Self::Thread(_) => None,
            Self::Async(owner) => owner
                .0
                .failure
                .lock()
                .expect("async failure lock poisoned")
                .as_ref()
                .map(duplicate),
        }
    }
    fn fail(&self, error: BindingResolutionError) -> BindingResolutionError {
        let error = Arc::new(error);
        let result = duplicate(&error);
        if let Self::Async(owner) = self {
            *owner.0.failure.lock().expect("async failure lock poisoned") = Some(error);
        }
        result
    }
    pub(super) fn current_thread() -> Self {
        Self::Thread(thread::current().id())
    }
    pub(super) fn is_sync(&self) -> bool {
        matches!(self, Self::Thread(_))
    }
    pub(super) fn is_current(&self) -> bool {
        match self {
            Self::Thread(id) => *id == thread::current().id(),
            Self::Async(id) => OWNERS.with_borrow(|owners| owners.contains(id)),
        }
    }
}

use super::{
    Binding, RegisteredBinding, ResolutionEvent, ServiceKey, SingletonState, resolving_hooks::Phase,
};
use crate::contracts::container::Container as _;
use std::{future::poll_fn, sync::Mutex, task::Poll};

struct OwnerPoll;
impl OwnerPoll {
    fn enter(identity: AsyncOwner) -> Self {
        OWNERS.with_borrow_mut(|owners| owners.push(identity));
        Self
    }
}
impl Drop for OwnerPoll {
    fn drop(&mut self) {
        OWNERS.with_borrow_mut(|owners| {
            owners.pop();
        });
    }
}
struct WaitEdge<'a> {
    container: &'a Container,
    owner: Owner,
    target: Owner,
}
impl Drop for WaitEdge<'_> {
    fn drop(&mut self) {
        let mut waiting = self
            .container
            .waiting
            .lock()
            .expect("container wait graph poisoned");
        let key = (self.owner.key(), self.target.key());
        if let Some((count, _, _)) = waiting.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                waiting.remove(&key);
            }
        }
    }
}

struct Construction<'a, T: ?Sized> {
    container: &'a Container,
    state: &'a Mutex<SingletonState<T>>,
    ready: &'a std::sync::Condvar,
    changed: &'a tokio::sync::Notify,
    owner: Owner,
    output: &'static str,
}
impl<T: ?Sized> Drop for Construction<'_, T> {
    fn drop(&mut self) {
        let previous = {
            let mut state = self.state.lock().expect("singleton state poisoned");
            let next = match &*state {
                SingletonState::Running(owner) if *owner == self.owner => {
                    if self.owner.failed().is_none() {
                        self.owner.fail(BindingResolutionError::FactoryFailed {
                            service: self.output,
                            source: std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "asynchronous service initialization interrupted",
                            )
                            .into(),
                        });
                    }
                    Some(SingletonState::Empty)
                }
                SingletonState::Publishing(owner, value) if *owner == self.owner => {
                    Some(SingletonState::Ready(value.clone()))
                }
                _ => None,
            };
            next.map(|next| std::mem::replace(&mut *state, next))
        };
        let address = std::ptr::from_ref(self.state).addr();
        self.container
            .waiting
            .lock()
            .expect("container wait graph poisoned")
            .retain(|_, (_, _, binding)| *binding != address);
        self.ready.notify_all();
        self.changed.notify_waiters();
        drop(previous);
    }
}

impl Container {
    async fn with_owner<T>(&self, id: AsyncOwner, future: impl Future<Output = T> + Send) -> T {
        let mut future = std::pin::pin!(future);
        poll_fn(move |cx| {
            let _owner = OwnerPoll::enter(id.clone());
            future.as_mut().poll(cx)
        })
        .await
    }
    async fn with_resolution<T>(
        &self,
        key: &ServiceKey,
        recipe: usize,
        future: impl Future<Output = Result<T, BindingResolutionError>> + Send,
    ) -> Result<T, BindingResolutionError> {
        let mut future = std::pin::pin!(future);
        poll_fn(move |cx| {
            let _frame = match self.resolve(key, recipe) {
                Ok(frame) => frame,
                Err(error) => return Poll::Ready(Err(error)),
            };
            future.as_mut().poll(cx)
        })
        .await
    }
    pub(super) async fn get_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: ServiceKey,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.get_async_with_parameters(id, super::ParameterOverrides::default())
            .await
    }
    pub(super) async fn get_async_with_parameters<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: ServiceKey,
        parameters: super::ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        super::parameters::run_async(self, parameters, async move {
            scope::check_open(self)?;
            let key = match self.require_sync(id.clone()) {
                Ok(key) => key,
                Err(BindingResolutionError::DeferredServiceRequiresAsync { .. }) => {
                    scope::preserve(self, self.load_deferred(id)).await?
                }
                Err(error) => return Err(error),
            };
            let key = self.getAlias(key)?;
            self.fire_hooks(
                &ResolutionEvent::before_with(key.clone(), super::parameters::current(self)),
                Phase::Before,
            )?;
            let (key, entry, scoped, contextual) = self.select_entry(key, true)?;
            self.resolve_entry_async(key, entry, scoped, contextual)
                .await
        })
        .await
    }
    pub(super) async fn resolve_entry_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        mut entry: RegisteredBinding,
        scoped: bool,
        contextual: bool,
    ) -> Result<Arc<T>, BindingResolutionError> {
        if scoped && entry.shared && !entry.instance {
            entry = scope::select(self, &key, &entry);
        }
        let binding = entry.value.clone().downcast::<Binding<T>>().map_err(|_| {
            BindingResolutionError::TypeMismatch {
                service: key.clone(),
                expected: std::any::type_name::<T>(),
                actual: entry.output,
            }
        })?;
        if matches!(binding.as_ref(), Binding::Factory(_)) {
            return self.resolve_entry(key, entry, false, contextual);
        }
        if let Some((state, _, _)) = super::extensions::slot(&binding) {
            let state = state.lock().expect("singleton state poisoned");
            match &*state {
                SingletonState::Ready(value) => return Ok(value.clone()),
                SingletonState::Publishing(owner, value)
                | SingletonState::Extending(owner, value)
                    if owner.is_current() =>
                {
                    return Ok(value.clone());
                }
                _ => {}
            }
        }
        scope::preserve(self, async move {
            let id = AsyncOwner::default();
            let owner = Owner::Async(id.clone());
            let parent = OWNERS.with_borrow(|owners| owners.last().cloned());
            let _parent = if let Some(parent) = parent {
                let parent = Owner::Async(parent);
                self.wait_for(parent.clone(), owner.clone(), key.clone(), 0)?;
                Some(WaitEdge {
                    container: self,
                    owner: parent,
                    target: owner.clone(),
                })
            } else {
                None
            };
            self.with_owner(id, async {
                match binding.as_ref() {
                    Binding::Factory(_) => self.resolve_entry(key, entry, false, contextual),
                    Binding::AsyncFactory(factory) => {
                        let value = self
                            .with_resolution(&key, Arc::as_ptr(&entry.version).addr(), async {
                                self.check_extenders::<T>(&key)?;
                                let value = factory(self).await?;
                                scope::check_open(self)?;
                                self.decorate(&key, value, None)
                            })
                            .await?;
                        let current = self.current_recipe(&key, &entry, contextual);
                        self.fire_constructed(&key, value.clone())?;
                        if current {
                            self.registry
                                .write()
                                .expect("container lock poisoned")
                                .resolved
                                .insert(key);
                        }
                        Ok(value)
                    }
                    _ => {
                        let (state, ready, changed) =
                            super::extensions::slot(&binding).expect("shared cache slot");
                        loop {
                            let notification = changed.notified();
                            tokio::pin!(notification);
                            notification.as_mut().enable();
                            let wait = {
                                let mut state = state.lock().expect("singleton state poisoned");
                                match &*state {
                                    SingletonState::Ready(value) => return Ok(value.clone()),
                                    SingletonState::Publishing(active, value)
                                    | SingletonState::Extending(active, value)
                                        if active.is_current() =>
                                    {
                                        return Ok(value.clone());
                                    }
                                    SingletonState::Empty => {
                                        *state = SingletonState::Running(owner.clone());
                                        None
                                    }
                                    SingletonState::Running(active)
                                    | SingletonState::Publishing(active, _)
                                    | SingletonState::Extending(active, _) => {
                                        if active.is_current() {
                                            return Err(
                                                BindingResolutionError::CircularDependency {
                                                    services: vec![key.clone(), key.clone()],
                                                },
                                            );
                                        }
                                        Some(active.clone())
                                    }
                                }
                            };
                            if let Some(active) = wait {
                                self.wait_for(
                                    owner.clone(),
                                    active.clone(),
                                    key.clone(),
                                    std::ptr::from_ref(state).addr(),
                                )?;
                                let _edge = WaitEdge {
                                    container: self,
                                    owner: owner.clone(),
                                    target: active.clone(),
                                };
                                notification.await;
                                if let Some(error) = active.failed() {
                                    return Err(error);
                                }
                                continue;
                            }
                            break;
                        }
                        let _construction = Construction {
                            container: self,
                            state,
                            ready,
                            changed,
                            owner: owner.clone(),
                            output: entry.output,
                        };
                        let value = self
                            .with_resolution(&key, Arc::as_ptr(&entry.version).addr(), async {
                                self.check_extenders::<T>(&key)?;
                                let value = match binding.as_ref() {
                                    Binding::Singleton { factory, .. } => factory(self)?,
                                    Binding::AsyncSingleton { factory, .. } => {
                                        factory(self).await?
                                    }
                                    _ => unreachable!("an explicit instance cannot be empty"),
                                };
                                scope::check_open(self)?;
                                self.decorate(&key, value, Some((state, &entry, owner.clone())))
                            })
                            .await
                            .map_err(|error| owner.fail(error))?;
                        let current = self.current_recipe(&key, &entry, contextual);
                        self.fire_constructed(&key, value.clone())?;
                        if current {
                            self.registry
                                .write()
                                .expect("container lock poisoned")
                                .resolved
                                .insert(key);
                        }
                        Ok(value)
                    }
                }
            })
            .await
        })
        .await
    }
    pub(super) async fn register_async<T, F>(
        &self,
        key: ServiceKey,
        factory: F,
        shared: bool,
        scoped: bool,
        conditional: bool,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        let factory: Arc<AsyncFactory<T>> = Arc::new(factory);
        let binding = if shared {
            Binding::AsyncSingleton {
                factory,
                state: Mutex::new(SingletonState::Empty),
                ready: std::sync::Condvar::new(),
                changed: tokio::sync::Notify::new(),
            }
        } else {
            Binding::AsyncFactory(factory)
        };
        let mutation = if scoped {
            self.commit_scoped::<T>(key, Self::entry(binding), conditional)
        } else if conditional {
            self.commit_if_absent(key, binding)
        } else {
            self.commit(key, binding)
        };
        mutation.dispatch_async(self).await
    }
}

impl Container {
    pub(super) async fn await_cache<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        binding: &Binding<T>,
    ) -> Result<(), BindingResolutionError> {
        let (state, _, changed) = super::extensions::slot(binding).expect("shared cache slot");
        let owner = Owner::Async(AsyncOwner::default());
        loop {
            let notification = changed.notified();
            tokio::pin!(notification);
            notification.as_mut().enable();
            let active = {
                let state = state.lock().expect("singleton state poisoned");
                match &*state {
                    SingletonState::Ready(_) | SingletonState::Empty => return Ok(()),
                    SingletonState::Publishing(owner, _) | SingletonState::Extending(owner, _)
                        if owner.is_current() =>
                    {
                        return Ok(());
                    }
                    SingletonState::Running(active)
                    | SingletonState::Publishing(active, _)
                    | SingletonState::Extending(active, _) => active.clone(),
                }
            };
            self.wait_for(
                owner.clone(),
                active.clone(),
                key.clone(),
                std::ptr::from_ref(state).addr(),
            )?;
            let _edge = WaitEdge {
                container: self,
                owner: owner.clone(),
                target: active.clone(),
            };
            notification.await;
            if let Some(error) = active.failed() {
                return Err(error);
            }
        }
    }
}

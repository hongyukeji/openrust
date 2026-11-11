use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{Container, RegisteredBinding, ServiceKey};

#[derive(Clone, Default)]
pub(super) struct Identity(Arc<()>);
impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Identity {}
impl Hash for Identity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Default)]
struct Cache {
    entries: HashMap<ServiceKey, RegisteredBinding>,
    overrides: HashMap<ServiceKey, RegisteredBinding>,
    removed_aliases: HashSet<ServiceKey>,
}

struct Context {
    cache: Mutex<Cache>,
    closed: AtomicBool,
    changed: tokio::sync::watch::Sender<bool>,
}
impl Context {
    fn close(&self, container: &Container) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.changed.send_replace(true);
            let retired = std::mem::take(&mut *self.cache.lock().expect("scope cache poisoned"));
            retire(container, retired);
        }
    }
}
/// Weak records let a global container flush detach live request caches without closing requests.
#[derive(Default)]
pub(super) struct Lifecycles(Mutex<HashMap<usize, Weak<Context>>>);
pub(super) struct FlushedScopes<'a> {
    container: &'a Container,
    _contexts: Vec<Arc<Context>>,
    _caches: Vec<Cache>,
}
impl Lifecycles {
    fn register(&self, context: &Arc<Context>) {
        self.0
            .lock()
            .expect("scope records poisoned")
            .insert(Arc::as_ptr(context).addr(), Arc::downgrade(context));
    }
    pub(super) fn detach<'a>(&self, container: &'a Container) -> FlushedScopes<'a> {
        let mut records = self.0.lock().expect("scope records poisoned");
        let mut retired = FlushedScopes {
            container,
            _contexts: Vec::new(),
            _caches: Vec::new(),
        };
        records.retain(|_, record| {
            if let Some(context) = record.upgrade() {
                let cache =
                    std::mem::take(&mut *context.cache.lock().expect("scope cache poisoned"));
                retired._caches.push(cache);
                retired._contexts.push(context);
                true
            } else {
                false
            }
        });
        retired
    }
}
impl Drop for FlushedScopes<'_> {
    fn drop(&mut self) {
        let values = (
            std::mem::take(&mut self._contexts),
            std::mem::take(&mut self._caches),
        );
        retire(self.container, values);
    }
}
// This guard closes the request before retained Arc snapshots can outlive it.
struct CloseContext<'a> {
    context: Arc<Context>,
    container: &'a Container,
}
struct RemoveRecord<'a> {
    records: &'a Lifecycles,
    key: usize,
}
impl Drop for RemoveRecord<'_> {
    fn drop(&mut self) {
        self.records
            .0
            .lock()
            .expect("scope records poisoned")
            .remove(&self.key);
    }
}
impl Drop for CloseContext<'_> {
    fn drop(&mut self) {
        let _remove = RemoveRecord {
            records: &self.container.scopes,
            key: Arc::as_ptr(&self.context).addr(),
        };
        self.context.close(self.container);
    }
}
fn retire<T>(container: &Container, values: T) {
    super::parameters::with_snapshot(
        container.identity.clone(),
        super::ParameterOverrides::new(),
        || {
            crate::support::facades::ContextSnapshot::empty().run(|| {
                CURRENT.sync_scope(Arc::default(), || {
                    super::call_context::neutral(container, || drop(values))
                })
            })
        },
    );
}

tokio::task_local! { static CURRENT: Arc<HashMap<Identity, Arc<Context>>>; }

fn with<R>(container: &Container, operation: impl FnOnce(&Context) -> R) -> Option<R> {
    CURRENT
        .try_with(|contexts| {
            contexts
                .get(&container.identity)
                .map(|context| operation(context))
        })
        .ok()
        .flatten()
}

pub(super) fn active(container: &Container) -> bool {
    with(container, |context| !context.closed.load(Ordering::Acquire)).unwrap_or(false)
}

pub(super) fn check_open(
    container: &Container,
) -> Result<(), crate::contracts::container::BindingResolutionError> {
    if with(container, |context| context.closed.load(Ordering::Acquire)).unwrap_or(false) {
        return Err(crate::contracts::container::BindingResolutionError::ScopeClosed);
    }
    Ok(())
}

pub(super) fn alias_removed(container: &Container, key: &ServiceKey) -> bool {
    with(container, |context| {
        context
            .cache
            .lock()
            .expect("scope cache poisoned")
            .removed_aliases
            .contains(key)
    })
    .unwrap_or(false)
}

pub(super) fn restore_alias(container: &Container, key: &ServiceKey) {
    with(container, |context| {
        context
            .cache
            .lock()
            .expect("scope cache poisoned")
            .removed_aliases
            .remove(key)
    });
}

pub(super) fn lookup(container: &Container, key: &ServiceKey) -> Option<RegisteredBinding> {
    with(container, |context| {
        context
            .cache
            .lock()
            .expect("scope cache poisoned")
            .overrides
            .get(key)
            .cloned()
    })
    .flatten()
}

pub(super) fn put(container: &Container, key: ServiceKey, entry: RegisteredBinding) {
    let previous = with(container, |context| {
        let mut cache = context.cache.lock().expect("scope cache poisoned");
        cache.removed_aliases.insert(key.clone());
        let previous = cache.entries.remove(&key);
        (previous, cache.overrides.insert(key, entry))
    });
    drop(previous);
}

pub(super) fn select(
    container: &Container,
    key: &ServiceKey,
    root: &RegisteredBinding,
) -> RegisteredBinding {
    let (entry, retired) = select_retained(container, key, root);
    drop(retired);
    entry
}

// Callers holding a registry guard must defer the retired entry's destruction.
pub(super) fn select_retained(
    container: &Container,
    key: &ServiceKey,
    root: &RegisteredBinding,
) -> (RegisteredBinding, Option<RegisteredBinding>) {
    with(container, |context| {
        let mut cache = context.cache.lock().expect("scope cache poisoned");
        if let Some(entry) = cache.entries.get(key)
            && Arc::ptr_eq(&entry.version, &root.version)
        {
            return (entry.clone(), None);
        }
        let entry = root.fresh.as_ref().expect("scoped factories are shared")()
            .with_version(root.version.clone());
        let retired = cache.entries.insert(key.clone(), entry.clone());
        (entry, retired)
    })
    .unwrap_or_else(|| (root.clone(), None))
}

pub(super) fn cached_entry(
    container: &Container,
    key: &ServiceKey,
    root: &RegisteredBinding,
) -> Option<RegisteredBinding> {
    with(container, |context| {
        context
            .cache
            .lock()
            .expect("scope cache poisoned")
            .entries
            .get(key)
            .filter(|entry| Arc::ptr_eq(&entry.version, &root.version))
            .cloned()
    })
    .flatten()
}

pub(super) fn is_published(
    container: &Container,
    key: &ServiceKey,
    root: &RegisteredBinding,
) -> bool {
    with(container, |context| {
        context
            .cache
            .lock()
            .expect("scope cache poisoned")
            .entries
            .get(key)
            .is_some_and(|entry| Arc::ptr_eq(&entry.version, &root.version) && entry.is_published())
    })
    .unwrap_or(false)
}

pub(super) fn remove(container: &Container, key: &ServiceKey) {
    let previous = with(container, |context| {
        let mut cache = context.cache.lock().expect("scope cache poisoned");
        (cache.entries.remove(key), cache.overrides.remove(key))
    });
    drop(previous);
}

pub(super) fn clear(container: &Container) {
    let previous = with(container, |context| {
        let mut cache = context.cache.lock().expect("scope cache poisoned");
        (
            std::mem::take(&mut cache.entries),
            std::mem::take(&mut cache.overrides),
        )
    });
    drop(previous);
}

impl Container {
    /// Runs a request or command lifecycle with an isolated scoped-instance cache.
    /// Nested execution for the same container shares the current lifecycle.
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        if active(self) {
            future.await
        } else {
            let mut contexts = CURRENT
                .try_with(|contexts| contexts.as_ref().clone())
                .unwrap_or_default();
            let context = Arc::new(Context {
                cache: Mutex::default(),
                closed: AtomicBool::new(false),
                changed: tokio::sync::watch::channel(false).0,
            });
            self.scopes.register(&context);
            contexts.insert(self.identity.clone(), context.clone());
            let _close = CloseContext {
                context,
                container: self,
            };
            CURRENT.scope(Arc::new(contexts), future).await
        }
    }
}

struct Retained<F> {
    parameters: super::ParameterOverrides,
    identity: Identity,
    future: Option<std::pin::Pin<Box<F>>>,
    contexts: Arc<HashMap<Identity, Arc<Context>>>,
    active: Option<Arc<Context>>,
    facade: crate::support::facades::ContextSnapshot,
    cancelled: Option<std::pin::Pin<Box<dyn Future<Output = ()> + Send>>>,
}
impl<F> Retained<F> {
    fn is_closed(&self) -> bool {
        self.facade.is_closed()
            || self
                .active
                .as_ref()
                .is_some_and(|context| context.closed.load(Ordering::Acquire))
    }
    fn run<R>(&self, operation: impl FnOnce() -> R) -> R {
        super::parameters::with_snapshot(self.identity.clone(), self.parameters.clone(), || {
            self.facade
                .run(|| CURRENT.sync_scope(self.contexts.clone(), operation))
        })
    }
}
impl<F, R> Future for Retained<F>
where
    F: Future<Output = Result<R, crate::contracts::container::BindingResolutionError>>,
{
    type Output = F::Output;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if this.is_closed() {
            return std::task::Poll::Ready(Err(
                crate::contracts::container::BindingResolutionError::ScopeClosed,
            ));
        }
        if !this.facade.matches_current() {
            return std::task::Poll::Ready(Err(
                crate::contracts::container::BindingResolutionError::FacadeNotSet,
            ));
        }
        if let Some(cancelled) = &mut this.cancelled
            && cancelled.as_mut().poll(cx).is_ready()
        {
            return std::task::Poll::Ready(Err(
                crate::contracts::container::BindingResolutionError::ScopeClosed,
            ));
        }
        let result = super::parameters::with_snapshot(
            this.identity.clone(),
            this.parameters.clone(),
            || {
                this.facade.run(|| {
                    CURRENT.sync_scope(this.contexts.clone(), || {
                        this.future
                            .as_mut()
                            .expect("retained future")
                            .as_mut()
                            .poll(cx)
                    })
                })
            },
        );
        if this.is_closed() {
            this.run(|| drop(result));
            return std::task::Poll::Ready(Err(
                crate::contracts::container::BindingResolutionError::ScopeClosed,
            ));
        }
        result
    }
}
impl<F> Drop for Retained<F> {
    fn drop(&mut self) {
        let future = self.future.take();
        self.run(|| drop(future));
    }
}
async fn wait_closed(mut receiver: tokio::sync::watch::Receiver<bool>) {
    let _ = receiver.wait_for(|closed| *closed).await;
}
/// Cold or waiting resolutions own snapshots; closing a lifecycle clears its caches and wakes them.
pub(super) async fn preserve<R, F>(
    container: &Container,
    future: F,
) -> Result<R, crate::contracts::container::BindingResolutionError>
where
    F: Future<Output = Result<R, crate::contracts::container::BindingResolutionError>> + Send,
{
    let contexts = CURRENT.try_with(Clone::clone).unwrap_or_default();
    let active = contexts.get(&container.identity).cloned();
    let facade = crate::support::facades::context_snapshot();
    let cancelled = match (
        active.as_ref().map(|context| context.changed.subscribe()),
        facade.subscribe(),
    ) {
        (Some(a), Some(b)) => Some(Box::pin(async move {
            tokio::select! {()=wait_closed(a)=>{},()=wait_closed(b)=>{}}
        })
            as std::pin::Pin<Box<dyn Future<Output = ()> + Send>>),
        (Some(a), None) | (None, Some(a)) => {
            Some(Box::pin(wait_closed(a)) as std::pin::Pin<Box<dyn Future<Output = ()> + Send>>)
        }
        (None, None) => None,
    };
    Retained {
        parameters: super::parameters::current(container),
        identity: container.identity.clone(),
        future: Some(Box::pin(future)),
        contexts,
        active,
        facade,
        cancelled,
    }
    .await
}

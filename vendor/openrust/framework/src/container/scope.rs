use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
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
}

tokio::task_local! { static CURRENT: HashMap<Identity, Arc<Context>>; }

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
    with(container, |_| ()).is_some()
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
            let mut contexts = CURRENT.try_with(Clone::clone).unwrap_or_default();
            contexts.insert(
                self.identity.clone(),
                Arc::new(Context {
                    cache: Mutex::default(),
                }),
            );
            CURRENT.scope(contexts, future).await
        }
    }
}

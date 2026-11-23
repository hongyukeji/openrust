use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    thread::{self, ThreadId},
};

pub(crate) struct DriverRegistry<T: ?Sized> {
    state: Mutex<State<T>>,
    changed: Condvar,
}

struct State<T: ?Sized> {
    stores: HashMap<String, Entry<T>>,
    waiting: HashMap<ThreadId, (String, Arc<ThreadId>)>,
}

enum Entry<T: ?Sized> {
    Resolving(Arc<ThreadId>),
    Ready(Arc<T>),
}

impl<T: ?Sized> Default for DriverRegistry<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                stores: HashMap::new(),
                waiting: HashMap::new(),
            }),
            changed: Condvar::new(),
        }
    }
}

impl<T: ?Sized> State<T> {
    fn resolving(&self, name: &str, owner: &Arc<ThreadId>) -> bool {
        matches!(self.stores.get(name), Some(Entry::Resolving(active)) if Arc::ptr_eq(active, owner))
    }

    fn would_cycle(&self, requester: ThreadId, mut owner: ThreadId) -> bool {
        loop {
            if owner == requester {
                return true;
            }
            let Some((name, next)) = self.waiting.get(&owner) else {
                return false;
            };
            if !self.resolving(name, next) {
                return false;
            }
            owner = **next;
        }
    }
}

impl<T: ?Sized> DriverRegistry<T> {
    pub(crate) fn get_or_resolve<E>(
        &self,
        name: &str,
        resolve: impl FnOnce() -> Result<Arc<T>, E>,
        cycle: E,
    ) -> Result<Arc<T>, E> {
        let requester = thread::current().id();
        let mut state = self.lock();
        let owner = loop {
            match state.stores.get(name) {
                Some(Entry::Ready(store)) => return Ok(Arc::clone(store)),
                Some(Entry::Resolving(owner)) => {
                    if state.would_cycle(requester, **owner) {
                        return Err(cycle);
                    }
                    let owner = Arc::clone(owner);
                    state.waiting.insert(requester, (name.to_owned(), owner));
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                    state.waiting.remove(&requester);
                }
                None => {
                    let owner = Arc::new(requester);
                    state
                        .stores
                        .insert(name.to_owned(), Entry::Resolving(Arc::clone(&owner)));
                    break owner;
                }
            }
        };
        drop(state);

        let pending = Resolution {
            registry: self,
            name,
            owner,
        };
        let store = resolve()?;
        let mut state = self.lock();
        if state.resolving(name, &pending.owner) {
            state
                .stores
                .insert(name.to_owned(), Entry::Ready(Arc::clone(&store)));
        }
        Ok(store)
    }

    pub(crate) fn forget(&self, name: &str) {
        let removed = self.lock().stores.remove(name);
        self.changed.notify_all();
        drop(removed);
    }

    pub(crate) fn resolved(&self) -> Vec<Arc<T>> {
        self.lock()
            .stores
            .values()
            .filter_map(|entry| match entry {
                Entry::Ready(repository) => Some(repository.clone()),
                Entry::Resolving(_) => None,
            })
            .collect()
    }

    pub(crate) fn entries(&self) -> HashMap<String, Arc<T>> {
        self.lock()
            .stores
            .iter()
            .filter_map(|(name, entry)| match entry {
                Entry::Ready(driver) => Some((name.clone(), driver.clone())),
                Entry::Resolving(_) => None,
            })
            .collect()
    }

    pub(crate) fn forget_all(&self) {
        let removed = std::mem::take(&mut self.lock().stores);
        self.changed.notify_all();
        drop(removed);
    }

    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Resolution<'a, T: ?Sized> {
    registry: &'a DriverRegistry<T>,
    name: &'a str,
    owner: Arc<ThreadId>,
}

impl<T: ?Sized> Drop for Resolution<'_, T> {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        if state.resolving(self.name, &self.owner) {
            state.stores.remove(self.name);
        }
        self.registry.changed.notify_all();
    }
}

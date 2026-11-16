use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    thread::{self, ThreadId},
};

use super::{CacheError, CacheResult, Repository};

#[derive(Default)]
pub(super) struct StoreRegistry {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    stores: HashMap<String, Entry>,
    waiting: HashMap<ThreadId, (String, Arc<ThreadId>)>,
}

enum Entry {
    Resolving(Arc<ThreadId>),
    Ready(Arc<Repository>),
}

impl State {
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

impl StoreRegistry {
    pub(super) fn get_or_resolve(
        &self,
        name: &str,
        resolve: impl FnOnce() -> CacheResult<Repository>,
    ) -> CacheResult<Arc<Repository>> {
        let requester = thread::current().id();
        let mut state = self.lock();
        let owner = loop {
            match state.stores.get(name) {
                Some(Entry::Ready(store)) => return Ok(Arc::clone(store)),
                Some(Entry::Resolving(owner)) => {
                    if state.would_cycle(requester, **owner) {
                        return Err(CacheError::StoreResolutionCycle);
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
        let store = Arc::new(resolve()?);
        let mut state = self.lock();
        if state.resolving(name, &pending.owner) {
            state
                .stores
                .insert(name.to_owned(), Entry::Ready(Arc::clone(&store)));
        }
        Ok(store)
    }

    pub(super) fn forget(&self, name: &str) {
        let removed = self.lock().stores.remove(name);
        self.changed.notify_all();
        drop(removed);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Resolution<'a> {
    registry: &'a StoreRegistry,
    name: &'a str,
    owner: Arc<ThreadId>,
}

impl Drop for Resolution<'_> {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        if state.resolving(self.name, &self.owner) {
            state.stores.remove(self.name);
        }
        self.registry.changed.notify_all();
    }
}

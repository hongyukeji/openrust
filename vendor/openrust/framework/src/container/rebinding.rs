//! Committed values and callback lists are snapshots; delivery never holds registry locks.
use super::{
    Container, RegisteredBinding, ResolutionEvent, ServiceKey, resolving_hooks::Phase, scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{any::type_name, cell::RefCell, sync::Arc};
type Callback = dyn Fn(&Container, &ResolutionEvent, &'static str) -> Result<(), BindingResolutionError>
    + Send
    + Sync;
#[derive(Clone)]
pub(super) struct RegisteredRebinding(Arc<Callback>);
thread_local! { static REBOUNDING: RefCell<Vec<(usize, ServiceKey)>> = const { RefCell::new(Vec::new()) }; }
struct Rebound;
impl Rebound {
    fn enter(container: &Container, key: &ServiceKey) -> Result<Self, BindingResolutionError> {
        let owner = std::ptr::from_ref(container).addr();
        REBOUNDING.with_borrow_mut(|stack| {
            if stack
                .iter()
                .any(|(known, name)| *known == owner && name == key)
            {
                return Err(BindingResolutionError::CircularDependency {
                    services: vec![key.clone(), key.clone()],
                });
            }
            stack.push((owner, key.clone()));
            Ok(Self)
        })
    }
}
impl Drop for Rebound {
    fn drop(&mut self) {
        REBOUNDING.with_borrow_mut(|stack| {
            stack.pop();
        });
    }
}
impl Container {
    pub(super) fn register_rebinding<T, F>(
        &self,
        abstract_key: impl Into<ServiceKey>,
        callback: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        let key = self.getAlias(abstract_key)?;
        let registered = RegisteredRebinding(Arc::new(move |app, event, actual| {
            let value = event
                .value::<T>()
                .ok_or_else(|| BindingResolutionError::TypeMismatch {
                    service: event.key().clone(),
                    expected: type_name::<T>(),
                    actual,
                })?;
            callback(app, value)
        }));
        self.registry
            .write()
            .expect("container lock poisoned")
            .rebound_callbacks
            .entry(key.clone())
            .or_default()
            .push(registered);
        if self.has(key.clone()) {
            self.get(key).map(Some)
        } else {
            Ok(None)
        }
    }
    pub(super) fn rebound_callbacks(&self, key: &ServiceKey) -> Vec<RegisteredRebinding> {
        self.registry
            .read()
            .expect("container lock poisoned")
            .rebound_callbacks
            .get(key)
            .cloned()
            .unwrap_or_default()
    }
    fn deliver_rebound<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        value: Arc<T>,
        callbacks: Vec<RegisteredRebinding>,
    ) -> Result<(), BindingResolutionError> {
        let event = ResolutionEvent::constructed(key, value);
        for callback in callbacks {
            (callback.0)(self, &event, type_name::<T>())?;
        }
        Ok(())
    }
    pub(super) fn dispatch_rebound_value<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        value: Arc<T>,
        callbacks: Vec<RegisteredRebinding>,
    ) -> Result<(), BindingResolutionError> {
        if callbacks.is_empty() {
            return Ok(());
        }
        let _rebound = Rebound::enter(self, &key)?;
        self.fire_hooks(&ResolutionEvent::before(key.clone()), Phase::Before)?;
        self.deliver_rebound(key, value, callbacks)
    }
    pub(super) fn dispatch_rebound<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        mut entry: RegisteredBinding,
        callbacks: Vec<RegisteredRebinding>,
    ) -> Result<(), BindingResolutionError> {
        if callbacks.is_empty() {
            return Ok(());
        }
        let _rebound = Rebound::enter(self, &key)?;
        self.fire_hooks(&ResolutionEvent::before(key.clone()), Phase::Before)?;
        let retired = if entry.shared && !entry.instance && scope::active(self) {
            let registry = self.registry.read().expect("container lock poisoned");
            if registry.scoped.contains(&key) {
                let current = registry
                    .bindings
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(&current.value, &entry.value));
                if current {
                    let (selected, retired) = scope::select_retained(self, &key, &entry);
                    entry = selected;
                    retired
                } else {
                    entry = entry.fresh.as_ref().expect("shared recipe")()
                        .with_version(entry.version.clone());
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        drop(retired);
        // Use this mutation's committed recipe, never a later same-name replacement.
        let value = self.resolve_entry::<T>(key.clone(), entry, false, false)?;
        self.deliver_rebound(key, value, callbacks)
    }
}

//! Ordered callback snapshots. User callbacks execute without the registry lock.
use super::{Container, ServiceKey};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    sync::{Arc, atomic::Ordering},
};

/// The canonical service identity and, after construction, its existing typed value.
/// Retaining an event retains its service value, but not its container.
#[derive(Clone)]
pub struct ResolutionEvent {
    key: ServiceKey,
    value: Option<Arc<dyn Any + Send + Sync>>,
    output: Option<TypeId>,
}
impl ResolutionEvent {
    pub fn key(&self) -> &ServiceKey {
        &self.key
    }
    /// Returns the registered output contract. Before construction there is no value.
    pub fn value<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.value.as_ref()?.downcast_ref::<Arc<T>>().cloned()
    }
    pub(super) fn before(key: ServiceKey) -> Self {
        Self {
            key,
            value: None,
            output: None,
        }
    }
    pub(super) fn constructed<T: ?Sized + Send + Sync + 'static>(
        key: ServiceKey,
        value: Arc<T>,
    ) -> Self {
        Self {
            key,
            value: Some(Arc::new(value)),
            output: Some(TypeId::of::<T>()),
        }
    }
}

type Callback =
    dyn Fn(&ResolutionEvent, &Container) -> Result<(), BindingResolutionError> + Send + Sync;
type Callbacks = Vec<Arc<Callback>>;
#[derive(Default)]
pub(super) struct HookGroup {
    global: Callbacks,
    keyed: Vec<(ServiceKey, Callbacks)>,
}
impl HookGroup {
    fn register(&mut self, key: Option<ServiceKey>, callback: Arc<Callback>) {
        if let Some(key) = key {
            if let Some((_, callbacks)) = self.keyed.iter_mut().find(|(known, _)| *known == key) {
                callbacks.push(callback);
            } else {
                self.keyed.push((key, vec![callback]));
            }
        } else {
            self.global.push(callback);
        }
    }
    fn matching(&self, event: &ResolutionEvent) -> Callbacks {
        self.keyed
            .iter()
            .filter(|(key, _)| {
                key == event.key() || (key.type_id().is_some() && key.type_id() == event.output)
            })
            .flat_map(|(_, hooks)| hooks.iter().cloned())
            .collect()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Before,
    Resolving,
    After,
}
impl Phase {
    fn mask(self) -> u8 {
        match self {
            Self::Before => 1,
            Self::Resolving => 2,
            Self::After => 4,
        }
    }
}

#[derive(Default)]
pub(super) struct ResolveHooks {
    before: HookGroup,
    resolving: HookGroup,
    after: HookGroup,
}
impl ResolveHooks {
    fn group(&self, phase: Phase) -> &HookGroup {
        match phase {
            Phase::Before => &self.before,
            Phase::Resolving => &self.resolving,
            Phase::After => &self.after,
        }
    }
    fn group_mut(&mut self, phase: Phase) -> &mut HookGroup {
        match phase {
            Phase::Before => &mut self.before,
            Phase::Resolving => &mut self.resolving,
            Phase::After => &mut self.after,
        }
    }
}
thread_local! { static DISPATCHING: RefCell<Vec<(usize, ServiceKey, Phase)>> = const { RefCell::new(Vec::new()) }; }
struct Dispatch;
impl Dispatch {
    fn enter(
        container: &Container,
        event: &ResolutionEvent,
        phase: Phase,
    ) -> Result<Self, BindingResolutionError> {
        let owner = std::ptr::from_ref(container).addr();
        DISPATCHING.with_borrow_mut(|stack| {
            if stack.iter().any(|(known, key, active)| {
                *known == owner && key == event.key() && *active == phase
            }) {
                return Err(BindingResolutionError::CircularDependency {
                    services: vec![event.key().clone(), event.key().clone()],
                });
            }
            stack.push((owner, event.key().clone(), phase));
            Ok(Self)
        })
    }
}
impl Drop for Dispatch {
    fn drop(&mut self) {
        DISPATCHING.with_borrow_mut(|stack| {
            stack.pop();
        });
    }
}
impl Container {
    pub(super) fn has_hooks(&self, phase: Phase) -> bool {
        self.hook_phases.load(Ordering::Acquire) & phase.mask() != 0
    }

    pub(super) fn register_hook(
        &self,
        selector: Option<ServiceKey>,
        phase: Phase,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        let selector = selector.map(|key| self.getAlias(key)).transpose()?;
        let callback: Arc<Callback> = Arc::new(callback);
        self.registry
            .write()
            .expect("container lock poisoned")
            .hooks
            .group_mut(phase)
            .register(selector, callback);
        self.hook_phases.fetch_or(phase.mask(), Ordering::Release);
        Ok(())
    }
    pub(super) fn fire_hooks(
        &self,
        event: &ResolutionEvent,
        phase: Phase,
    ) -> Result<(), BindingResolutionError> {
        if !self.has_hooks(phase) {
            return Ok(());
        }
        let global = {
            let registry = self.registry.read().expect("container lock poisoned");
            let group = registry.hooks.group(phase);
            if group.global.is_empty() && group.keyed.is_empty() {
                return Ok(());
            }
            group.global.clone()
        };
        // Snapshot each stage separately: a global callback can register a keyed callback
        // for the same lookup, matching Laravel's dispatch order.
        let _dispatch = Dispatch::enter(self, event, phase)?;
        for callback in global {
            callback(event, self)?;
        }
        let keyed = self
            .registry
            .read()
            .expect("container lock poisoned")
            .hooks
            .group(phase)
            .matching(event);
        for callback in keyed {
            callback(event, self)?;
        }
        Ok(())
    }
    pub(super) fn fire_constructed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        value: Arc<T>,
    ) -> Result<(), BindingResolutionError> {
        if !self.has_hooks(Phase::Resolving) && !self.has_hooks(Phase::After) {
            return Ok(());
        }
        let event = ResolutionEvent::constructed(key.clone(), value);
        self.fire_hooks(&event, Phase::Resolving)?;
        self.fire_hooks(&event, Phase::After)
    }
}

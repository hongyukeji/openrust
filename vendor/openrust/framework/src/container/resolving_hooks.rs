//! Ordered callback snapshots. User callbacks execute without the registry lock.
use super::{ClassProjection, Container, ParameterValue, ServiceKey};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    cell::RefCell,
    sync::{Arc, atomic::Ordering},
};

#[derive(Clone)]
struct ResolvedObject {
    key: ServiceKey,
    value: ParameterValue,
    views: Option<Arc<Vec<ClassProjection>>>,
}
impl ResolvedObject {
    fn from_value<T: ?Sized + Send + Sync + 'static>(value: Arc<T>) -> Self {
        Self {
            key: ServiceKey::typed::<T>(),
            value: ParameterValue::new(value),
            views: None,
        }
    }

    fn value<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        if let Ok(value) = self.value.downcast::<Arc<T>>() {
            return Some((*value).clone());
        }
        let target = ServiceKey::typed::<T>();
        self.views
            .as_ref()?
            .iter()
            .find(|view| view.target == target)?
            .view(&self.value)
            .ok()
    }

    fn matches(&self, key: &ServiceKey) -> bool {
        self.key == *key
            || self
                .views
                .as_ref()
                .is_some_and(|views| views.iter().any(|view| view.target == *key))
    }
}

pub(super) struct FactoryOutput<T: ?Sized> {
    pub(super) value: Arc<T>,
    forwarded: Option<ResolvedObject>,
}
impl<T: ?Sized + Send + Sync + 'static> FactoryOutput<T> {
    pub(super) fn plain(value: Arc<T>) -> Self {
        Self {
            value,
            forwarded: None,
        }
    }

    pub(super) fn forwarded<C: Send + Sync + 'static>(value: Arc<T>, concrete: Arc<C>) -> Self {
        let forwarded = (ServiceKey::typed::<T>() != ServiceKey::typed::<C>()
            && std::ptr::addr_eq(Arc::as_ptr(&value), Arc::as_ptr(&concrete)))
        .then(|| ResolvedObject::from_value(concrete));
        Self { value, forwarded }
    }

    pub(super) fn decorate(
        self,
        decorate: impl FnOnce(Arc<T>) -> Result<Arc<T>, BindingResolutionError>,
    ) -> Result<Self, BindingResolutionError> {
        let address = Arc::as_ptr(&self.value);
        let value = decorate(self.value)?;
        // The forwarded Arc keeps the original allocation live while its identity is checked.
        let forwarded = self
            .forwarded
            .filter(|_| std::ptr::addr_eq(address, Arc::as_ptr(&value)));
        Ok(Self { value, forwarded })
    }
}

/// The canonical service identity and, after construction, its typed values.
/// Retaining an event retains its service value, but not its container.
#[derive(Clone)]
pub struct ResolutionEvent {
    key: ServiceKey,
    object: Option<ResolvedObject>,
    forwarded: Option<ResolvedObject>,
    parameters: super::ParameterOverrides,
}
impl ResolutionEvent {
    pub fn parameters(&self) -> &super::ParameterOverrides {
        &self.parameters
    }
    pub fn key(&self) -> &ServiceKey {
        &self.key
    }
    /// Returns the resolved object, its forwarded concrete type, or a declared trait view.
    pub fn value<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.object
            .as_ref()?
            .value::<T>()
            .or_else(|| self.forwarded.as_ref()?.value::<T>())
    }
    pub(super) fn before(key: ServiceKey) -> Self {
        Self::before_with(key, super::ParameterOverrides::default())
    }
    pub(super) fn before_with(key: ServiceKey, parameters: super::ParameterOverrides) -> Self {
        Self {
            key,
            object: None,
            forwarded: None,
            parameters,
        }
    }
    pub(super) fn constructed<T: ?Sized + Send + Sync + 'static>(
        key: ServiceKey,
        output: &FactoryOutput<T>,
        container: &Container,
    ) -> Self {
        let mut object = ResolvedObject::from_value(output.value.clone());
        object.views = container.class_projections(&object.key);
        let forwarded = output.forwarded.clone().map(|mut object| {
            object.views = container.class_projections(&object.key);
            object
        });
        Self {
            key,
            object: Some(object),
            forwarded,
            parameters: super::ParameterOverrides::default(),
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
    fn matching(&self, event: &ResolutionEvent, registry: &super::Registry) -> Callbacks {
        self.keyed
            .iter()
            .filter(|(key, _)| {
                key == event.key()
                    || if let Some(object) = &event.object {
                        object.matches(key)
                            || event
                                .forwarded
                                .as_ref()
                                .is_some_and(|object| object.matches(key))
                    } else {
                        registry
                            .classes
                            .get(event.key())
                            .and_then(|descriptor| descriptor.projections.as_ref())
                            .is_some_and(|views| views.iter().any(|view| view.target == *key))
                    }
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
        let keyed = {
            let registry = self.registry.read().expect("container lock poisoned");
            registry.hooks.group(phase).matching(event, &registry)
        };
        for callback in keyed {
            callback(event, self)?;
        }
        Ok(())
    }
    pub(super) fn fire_constructed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        value: &FactoryOutput<T>,
    ) -> Result<(), BindingResolutionError> {
        if !self.has_hooks(Phase::Resolving) && !self.has_hooks(Phase::After) {
            return Ok(());
        }
        let event = ResolutionEvent::constructed(key.clone(), value, self);
        self.fire_hooks(&event, Phase::Resolving)?;
        self.fire_hooks(&event, Phase::After)
    }
}

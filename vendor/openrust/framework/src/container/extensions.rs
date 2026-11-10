//! Binding decorators share the container's cache slots and wait graph.
use super::{
    Binding, Container, ErasedBinding, RegisteredBinding, ServiceKey, SingletonState, scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    any::type_name,
    cell::RefCell,
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex, atomic::Ordering},
    thread::{self, ThreadId},
};
type Closure<T> =
    dyn Fn(Arc<T>, &Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
struct Extender<T: ?Sized>(Arc<Closure<T>>);
#[derive(Clone)]
pub(super) struct RegisteredExtender {
    identity: Arc<()>,
    value: ErasedBinding,
    output: &'static str,
}
impl RegisteredExtender {
    fn check<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
    ) -> Result<(), BindingResolutionError> {
        if self.value.is::<Extender<T>>() {
            Ok(())
        } else {
            Err(BindingResolutionError::TypeMismatch {
                service: key.clone(),
                expected: type_name::<T>(),
                actual: self.output,
            })
        }
    }
    fn apply<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        value: Arc<T>,
        container: &Container,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.check::<T>(key)?;
        (self
            .value
            .downcast_ref::<Extender<T>>()
            .expect("checked extender contract")
            .0)(value, container)
    }
}
thread_local! { static EXTENDING: RefCell<Vec<(usize, ServiceKey)>> = const { RefCell::new(Vec::new()) }; }
struct Extension;
impl Extension {
    fn enter(container: &Container, key: &ServiceKey) -> Result<Self, BindingResolutionError> {
        let owner = std::ptr::from_ref(container).addr();
        EXTENDING.with_borrow_mut(|stack| {
            if stack
                .iter()
                .any(|(known, service)| *known == owner && service == key)
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
impl Drop for Extension {
    fn drop(&mut self) {
        EXTENDING.with_borrow_mut(|stack| {
            stack.pop();
        });
    }
}
fn slot<T: ?Sized>(binding: &Binding<T>) -> Option<(&Mutex<SingletonState<T>>, &Condvar)> {
    match binding {
        Binding::Instance { state, ready } | Binding::Singleton { state, ready, .. } => {
            Some((state, ready))
        }
        Binding::Factory(_) => None,
    }
}
enum Admission<T: ?Sized> {
    Cached {
        binding: Arc<Binding<T>>,
        old: Arc<T>,
        publishing: bool,
    },
    Wait(Arc<Binding<T>>),
}
type Publication<'a, T> = (
    &'a Mutex<SingletonState<T>>,
    &'a RegisteredBinding,
    ThreadId,
);
impl Container {
    fn release_waiters<T: ?Sized>(&self, state: &Mutex<SingletonState<T>>, ready: &Condvar) {
        let address = std::ptr::from_ref(state).addr();
        self.waiting
            .lock()
            .expect("container wait graph poisoned")
            .retain(|_, (_, _, waiting_for)| *waiting_for != address);
        ready.notify_all();
    }
    pub(super) fn read_instance<T: ?Sized>(
        &self,
        key: &ServiceKey,
        state: &Mutex<SingletonState<T>>,
        ready: &Condvar,
    ) -> Result<Arc<T>, BindingResolutionError> {
        let caller = thread::current().id();
        let mut cached = state.lock().expect("singleton state poisoned");
        loop {
            match &*cached {
                SingletonState::Ready(value) => return Ok(value.clone()),
                SingletonState::Extending(owner, value)
                | SingletonState::Publishing(owner, value)
                    if *owner == caller =>
                {
                    return Ok(value.clone());
                }
                SingletonState::Running(owner)
                | SingletonState::Extending(owner, _)
                | SingletonState::Publishing(owner, _) => {
                    self.wait_for(
                        caller,
                        *owner,
                        key.clone(),
                        std::ptr::from_ref(state).addr(),
                    )?;
                    cached = ready.wait(cached).expect("singleton state poisoned");
                    self.waiting
                        .lock()
                        .expect("container wait graph poisoned")
                        .remove(&caller);
                }
                SingletonState::Empty => unreachable!("explicit instances always contain a value"),
            }
        }
    }
    pub(super) fn check_extenders<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
    ) -> Result<(), BindingResolutionError> {
        if !self.has_extenders.load(Ordering::Acquire) {
            return Ok(());
        }
        let registry = self.registry.read().expect("container lock poisoned");
        for extender in registry.extenders.get(key).into_iter().flatten() {
            extender.check::<T>(key)?;
        }
        Ok(())
    }
    pub(super) fn decorate<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ServiceKey,
        mut value: Arc<T>,
        publication: Option<Publication<'_, T>>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        if publication.is_none() && !self.has_extenders.load(Ordering::Acquire) {
            return Ok(value);
        }
        let mut applied: Vec<Arc<()>> = Vec::new();
        let mut extension = None;
        loop {
            let pending = {
                let registry = self.registry.read().expect("container lock poisoned");
                let pending: Vec<_> = registry
                    .extenders
                    .get(key)
                    .into_iter()
                    .flatten()
                    .filter(|next| !applied.iter().any(|done| Arc::ptr_eq(done, &next.identity)))
                    .cloned()
                    .collect();
                if pending.is_empty() {
                    if let Some((state, entry, caller)) = publication {
                        // Registry -> slot is also the extension admission lock order.
                        // An arriving extension sees either this publication or a pending constructor.
                        *state.lock().expect("singleton state poisoned") =
                            SingletonState::Publishing(caller, value.clone());
                        entry
                            .published
                            .as_ref()
                            .expect("shared cache marker")
                            .store(true, Ordering::Release);
                    }
                    return Ok(value);
                }
                pending
            };
            if extension.is_none() {
                extension = Some(Extension::enter(self, key)?);
            }
            for extender in pending {
                value = extender.apply(key, value, self)?;
                applied.push(extender.identity.clone());
            }
        }
    }
    pub(super) fn extend_value<T, F>(
        &self,
        abstract_key: impl Into<ServiceKey>,
        closure: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        let key = self.getAlias(abstract_key)?;
        let _extension = Extension::enter(self, &key)?;
        let extender = RegisteredExtender {
            identity: Arc::new(()),
            value: Arc::new(Extender(Arc::new(closure) as Arc<Closure<T>>)),
            output: type_name::<T>(),
        };
        loop {
            let admission = {
                let mut registry = self.registry.write().expect("container lock poisoned");
                let root = if registry.root_instance_visible(self, &key) {
                    registry
                        .instances
                        .get(&key)
                        .or_else(|| registry.bindings.get(&key))
                } else {
                    registry.bindings.get(&key)
                };
                let mut entry = scope::lookup(self, &key).or_else(|| root.cloned());
                if let Some(known) = &entry {
                    // Validate even if a scoped constructor has not been selected yet.
                    if !known.value.is::<Binding<T>>() {
                        return Err(BindingResolutionError::TypeMismatch {
                            service: key.clone(),
                            expected: type_name::<T>(),
                            actual: known.output,
                        });
                    }
                    if scope::active(self) && registry.scoped.contains(&key) && !known.instance {
                        entry = scope::cached_entry(self, &key, known);
                    }
                }
                let binding = entry.map(|entry| {
                    entry
                        .value
                        .downcast::<Binding<T>>()
                        .expect("checked binding contract")
                });
                let admission = binding.and_then(|binding| {
                    let (state, _) = slot(&binding)?;
                    let mut cached = state.lock().expect("singleton state poisoned");
                    match &*cached {
                        SingletonState::Ready(value) => {
                            let old = value.clone();
                            *cached =
                                SingletonState::Extending(thread::current().id(), old.clone());
                            drop(cached);
                            Some(Admission::Cached {
                                binding,
                                old,
                                publishing: false,
                            })
                        }
                        SingletonState::Publishing(owner, value)
                            if *owner == thread::current().id() =>
                        {
                            let old = value.clone();
                            *cached = SingletonState::Extending(*owner, old.clone());
                            drop(cached);
                            Some(Admission::Cached {
                                binding,
                                old,
                                publishing: true,
                            })
                        }
                        SingletonState::Publishing(_, _) | SingletonState::Extending(_, _) => {
                            drop(cached);
                            Some(Admission::Wait(binding))
                        }
                        SingletonState::Empty | SingletonState::Running(_) => None,
                    }
                });
                match admission {
                    Some(admission) => admission,
                    None => {
                        registry
                            .extenders
                            .entry(key.clone())
                            .or_default()
                            .push(extender.clone());
                        self.has_extenders.store(true, Ordering::Release);
                        return Ok(());
                    }
                }
            };
            match admission {
                Admission::Wait(binding) => {
                    let (state, ready) = slot(&binding).expect("cached binding");
                    // Wait without holding the registry, then reselect after any rebind/forget.
                    let _ = self.read_instance(&key, state, ready)?;
                }
                Admission::Cached {
                    binding,
                    old,
                    publishing,
                } => {
                    let (state, ready) = slot(&binding).expect("cached binding");
                    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                        extender.apply(&key, old.clone(), self)
                    }));
                    let replacement = match &outcome {
                        Ok(Ok(value)) => value.clone(),
                        _ => old.clone(),
                    };
                    let previous = {
                        let mut cached = state.lock().expect("singleton state poisoned");
                        std::mem::replace(
                            &mut *cached,
                            if publishing {
                                SingletonState::Publishing(thread::current().id(), replacement)
                            } else {
                                SingletonState::Ready(replacement)
                            },
                        )
                    };
                    self.release_waiters(state, ready);
                    drop(previous);
                    return outcome
                        .unwrap_or_else(|payload| panic::resume_unwind(payload))
                        .map(|_| ());
                }
            }
        }
    }
    pub(super) fn forget_extenders(
        &self,
        abstract_key: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError> {
        let key = self.getAlias(abstract_key)?;
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let previous = registry.extenders.remove(&key);
            self.has_extenders
                .store(!registry.extenders.is_empty(), Ordering::Release);
            previous
        };
        drop(previous);
        Ok(())
    }
}

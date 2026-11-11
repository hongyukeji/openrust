//! Binding decorators share the container's cache slots and wait graph.
use super::{
    Binding, Container, ErasedBinding, RegisteredBinding, RegisteredRebinding, ServiceKey,
    SingletonState, async_resolution::Owner, scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    any::type_name,
    cell::RefCell,
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex, atomic::Ordering},
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
pub(super) fn slot<T: ?Sized>(
    binding: &Binding<T>,
) -> Option<(&Mutex<SingletonState<T>>, &Condvar, &tokio::sync::Notify)> {
    match binding {
        Binding::Instance {
            state,
            ready,
            changed,
        }
        | Binding::Singleton {
            state,
            ready,
            changed,
            ..
        }
        | Binding::AsyncSingleton {
            state,
            ready,
            changed,
            ..
        } => Some((state, ready, changed)),
        Binding::Factory(_) | Binding::AsyncFactory(_) => None,
    }
}
enum ExtensionChange<T: ?Sized> {
    None,
    Recipe(RegisteredBinding, Vec<RegisteredRebinding>),
    Value(Arc<T>, Vec<RegisteredRebinding>),
    Wait(Arc<Binding<T>>),
}
enum Admission<T: ?Sized> {
    Cached {
        binding: Arc<Binding<T>>,
        old: Arc<T>,
        publishing: Option<Owner>,
    },
    Wait(Arc<Binding<T>>),
    Pending(Option<RegisteredBinding>, Vec<RegisteredRebinding>),
}
type Publication<'a, T> = (&'a Mutex<SingletonState<T>>, &'a RegisteredBinding, Owner);
impl Container {
    fn release_waiters<T: ?Sized>(
        &self,
        state: &Mutex<SingletonState<T>>,
        ready: &Condvar,
        changed: &tokio::sync::Notify,
    ) {
        let address = std::ptr::from_ref(state).addr();
        self.waiting
            .lock()
            .expect("container wait graph poisoned")
            .retain(|_, (_, _, waiting_for)| *waiting_for != address);
        ready.notify_all();
        changed.notify_waiters();
    }
    pub(super) fn read_instance<T: ?Sized>(
        &self,
        key: &ServiceKey,
        state: &Mutex<SingletonState<T>>,
        ready: &Condvar,
    ) -> Result<Arc<T>, BindingResolutionError> {
        let caller = Owner::current_thread();
        let mut cached = state.lock().expect("singleton state poisoned");
        loop {
            match &*cached {
                SingletonState::Ready(value) => return Ok(value.clone()),
                SingletonState::Extending(owner, value)
                | SingletonState::Publishing(owner, value)
                    if owner.is_current() =>
                {
                    return Ok(value.clone());
                }
                SingletonState::Running(owner)
                | SingletonState::Extending(owner, _)
                | SingletonState::Publishing(owner, _) => {
                    self.wait_for(
                        caller.clone(),
                        owner.clone(),
                        key.clone(),
                        std::ptr::from_ref(state).addr(),
                    )?;
                    cached = ready.wait(cached).expect("singleton state poisoned");
                    self.waiting
                        .lock()
                        .expect("container wait graph poisoned")
                        .retain(|(from, _), _| *from != caller.key());
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
                    if let Some((state, entry, ref caller)) = publication {
                        // Registry -> slot is also the extension admission lock order.
                        // An arriving extension sees either this publication or a pending constructor.
                        *state.lock().expect("singleton state poisoned") =
                            SingletonState::Publishing(caller.clone(), value.clone());
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
    fn extender<T, F>(closure: F) -> RegisteredExtender
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        RegisteredExtender {
            identity: Arc::new(()),
            value: Arc::new(Extender(Arc::new(closure) as Arc<Closure<T>>)),
            output: type_name::<T>(),
        }
    }
    pub(super) fn extend_value<T, F>(
        &self,
        key: impl Into<ServiceKey>,
        closure: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        let key = self.getAlias(key)?;
        let extender = Self::extender(closure);
        loop {
            match self.extend_change::<T>(key.clone(), extender.clone(), false)? {
                ExtensionChange::None => return Ok(()),
                ExtensionChange::Recipe(entry, callbacks) => {
                    return self.dispatch_rebound::<T>(key, entry, callbacks);
                }
                ExtensionChange::Value(value, callbacks) => {
                    return self.dispatch_rebound_value(key, value, callbacks);
                }
                ExtensionChange::Wait(binding) => {
                    let (state, ready, _) = slot(&binding).expect("cached binding");
                    self.read_instance(&key, state, ready)?;
                }
            }
        }
    }
    pub(super) async fn extend_async_value<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        closure: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        let key = self.getAlias(key)?;
        let extender = Self::extender(closure);
        loop {
            match self.extend_change::<T>(key.clone(), extender.clone(), true)? {
                ExtensionChange::None => return Ok(()),
                ExtensionChange::Recipe(entry, callbacks) => {
                    return self
                        .dispatch_rebound_async::<T>(key, entry, callbacks)
                        .await;
                }
                ExtensionChange::Value(value, callbacks) => {
                    return self.dispatch_rebound_value(key, value, callbacks);
                }
                ExtensionChange::Wait(binding) => self.await_cache(&key, &binding).await?,
            }
        }
    }
    fn extend_change<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        extender: RegisteredExtender,
        asynchronous: bool,
    ) -> Result<ExtensionChange<T>, BindingResolutionError> {
        let _extension = Extension::enter(self, &key)?;

        let admission = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let root = if registry.root_instance_visible(self, &key) {
                registry
                    .instances
                    .get(&key)
                    .or_else(|| registry.bindings.get(&key))
            } else {
                registry.bindings.get(&key)
            }
            .or_else(|| {
                registry
                    .classes
                    .get(&key)
                    .map(|record| record.entry(self, asynchronous))
            });
            let original = scope::lookup(self, &key).or_else(|| root.cloned());
            let mut entry = original.clone();
            let notify = registry.resolved.contains(&key)
                || original
                    .as_ref()
                    .is_some_and(|entry| entry.instance || entry.is_published());
            if let Some(known) = &entry {
                // Validate even if a scoped constructor has not been selected yet.
                if !known.value.is::<Binding<T>>() {
                    return Err(BindingResolutionError::TypeMismatch {
                        service: key.clone(),
                        expected: type_name::<T>(),
                        actual: known.output,
                    });
                }
                if scope::active(self) && registry.is_scoped(&key) && !known.instance {
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
                let (state, _, _) = slot(&binding)?;
                let mut cached = state.lock().expect("singleton state poisoned");
                match &*cached {
                    SingletonState::Ready(value) => {
                        let old = value.clone();
                        *cached = SingletonState::Extending(Owner::current_thread(), old.clone());
                        drop(cached);
                        Some(Admission::Cached {
                            binding,
                            old,
                            publishing: None,
                        })
                    }
                    SingletonState::Publishing(owner, value) if owner.is_current() => {
                        let old = value.clone();
                        let publishing_owner = owner.clone();
                        *cached = SingletonState::Extending(owner.clone(), old.clone());
                        drop(cached);
                        Some(Admission::Cached {
                            binding,
                            old,
                            publishing: Some(publishing_owner),
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
                    let callbacks = if notify {
                        registry
                            .rebound_callbacks
                            .get(&key)
                            .cloned()
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    Admission::Pending(original, callbacks)
                }
            }
        };
        match admission {
            Admission::Pending(entry, callbacks) => {
                // Registration is finished. A rebound construction enters its own decorator guard.
                drop(_extension);
                drop(extender);
                if callbacks.is_empty() {
                    return Ok(ExtensionChange::None);
                }
                match entry {
                    Some(entry) => Ok(ExtensionChange::Recipe(entry, callbacks)),
                    None => Err(BindingResolutionError::NotBound { service: key }),
                }
            }
            Admission::Wait(binding) => Ok(ExtensionChange::Wait(binding)),
            Admission::Cached {
                binding,
                old,
                publishing,
            } => {
                let (state, ready, changed) = slot(&binding).expect("cached binding");
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
                        match publishing {
                            Some(owner) => SingletonState::Publishing(owner, replacement),
                            None => SingletonState::Ready(replacement),
                        },
                    )
                };
                self.release_waiters(state, ready, changed);
                drop(previous);
                let value = outcome.unwrap_or_else(|payload| panic::resume_unwind(payload))?;
                Ok(ExtensionChange::Value(value, self.rebound_callbacks(&key)))
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

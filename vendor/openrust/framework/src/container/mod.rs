//! The framework's service container.

use std::{
    any::{Any, type_name},
    cell::RefCell,
    collections::{HashMap, HashSet},
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex, RwLock},
    thread::{self, ThreadId},
};

use crate::contracts::container::{self, BindingResolutionError};

mod service_key;
pub use service_key::ServiceKey;

type Factory<T> = dyn Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
type ErasedBinding = Arc<dyn Any + Send + Sync>;
type Wait = (ThreadId, ServiceKey, usize);

/// Owns the application's service bindings and shared instances.
#[derive(Default)]
pub struct Container {
    registry: RwLock<Registry>,
    waiting: Mutex<HashMap<ThreadId, Wait>>,
}

#[derive(Default)]
struct Registry {
    bindings: HashMap<ServiceKey, RegisteredBinding>,
    aliases: HashMap<ServiceKey, ServiceKey>,
}

#[derive(Clone)]
struct RegisteredBinding {
    value: ErasedBinding,
    output: &'static str,
}

impl Registry {
    fn get_alias(&self, key: ServiceKey) -> Result<ServiceKey, BindingResolutionError> {
        let mut current = key;
        let mut seen = HashSet::new();
        let mut path = Vec::new();
        while let Some(next) = self.aliases.get(&current) {
            path.push(current.clone());
            if !seen.insert(current.clone()) {
                return Err(BindingResolutionError::CircularAlias { services: path });
            }
            current = next.clone();
        }
        Ok(current)
    }
}

enum Binding<T: ?Sized> {
    Factory(Box<Factory<T>>),
    Singleton {
        factory: Box<Factory<T>>,
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
    },
    Instance(Arc<T>),
}

enum SingletonState<T: ?Sized> {
    Empty,
    Running(ThreadId),
    Ready(Arc<T>),
}

thread_local! {
    static RESOLVING: RefCell<Vec<(usize, ServiceKey)>> = const { RefCell::new(Vec::new()) };
}

struct Resolution;

impl Drop for Resolution {
    fn drop(&mut self) {
        RESOLVING.with_borrow_mut(|stack| {
            stack.pop();
        });
    }
}

impl Container {
    pub(crate) fn instance_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        instance: Arc<T>,
    ) {
        self.insert(key, Binding::Instance(instance));
    }

    fn insert<T: ?Sized + Send + Sync + 'static>(&self, key: ServiceKey, binding: Binding<T>) {
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            registry.aliases.remove(&key);
            registry.bindings.insert(
                key,
                RegisteredBinding {
                    value: Arc::new(binding),
                    output: type_name::<T>(),
                },
            )
        };
        drop(previous);
    }

    fn resolve(&self, key: &ServiceKey) -> Result<Resolution, BindingResolutionError> {
        let container = std::ptr::from_ref(self).addr();
        RESOLVING.with_borrow_mut(|stack| {
            if let Some(start) = stack
                .iter()
                .position(|(owner, binding)| *owner == container && binding == key)
            {
                let services = stack[start..]
                    .iter()
                    .map(|(_, name)| name.clone())
                    .chain(std::iter::once(key.clone()))
                    .collect();
                return Err(BindingResolutionError::CircularDependency { services });
            }
            stack.push((container, key.clone()));
            Ok(Resolution)
        })
    }

    fn wait_for(
        &self,
        caller: ThreadId,
        owner: ThreadId,
        service: ServiceKey,
        binding: usize,
    ) -> Result<(), BindingResolutionError> {
        let mut waiting = self.waiting.lock().expect("container wait graph poisoned");
        let mut thread = owner;
        let mut services = vec![service.clone()];
        while let Some((next, name, _)) = waiting.get(&thread) {
            services.push(name.clone());
            thread = *next;
            if thread == caller {
                services.push(service);
                return Err(BindingResolutionError::CircularDependency { services });
            }
        }
        waiting.insert(caller, (owner, service, binding));
        Ok(())
    }
}

impl container::Container for Container {
    fn bind<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(
            ServiceKey::typed::<T>(),
            Binding::Factory(Box::new(factory)),
        );
    }

    fn singleton<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(
            ServiceKey::typed::<T>(),
            Binding::Singleton {
                factory: Box::new(factory),
                state: Mutex::new(SingletonState::Empty),
                ready: Condvar::new(),
            },
        );
    }

    fn instance<T: ?Sized + Send + Sync + 'static>(&self, service: Arc<T>) {
        self.insert(ServiceKey::typed::<T>(), Binding::Instance(service));
    }

    #[allow(non_snake_case, reason = "Named overload for Rust's typed bind")]
    fn bindNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(ServiceKey::named(name), Binding::Factory(Box::new(factory)));
    }

    #[allow(non_snake_case, reason = "Named overload for Rust's typed singleton")]
    fn singletonNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(
            ServiceKey::named(name),
            Binding::Singleton {
                factory: Box::new(factory),
                state: Mutex::new(SingletonState::Empty),
                ready: Condvar::new(),
            },
        );
    }

    #[allow(non_snake_case, reason = "Named overload for Rust's typed instance")]
    fn instanceNamed<T: ?Sized + Send + Sync + 'static>(&self, name: &str, service: Arc<T>) {
        self.insert(ServiceKey::named(name), Binding::Instance(service));
    }

    fn alias(
        &self,
        r#abstract: impl Into<ServiceKey>,
        alias: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError> {
        let (target, alias) = (r#abstract.into(), alias.into());
        if target == alias {
            return Err(BindingResolutionError::AliasToSelf { service: target });
        }
        self.registry
            .write()
            .expect("container lock poisoned")
            .aliases
            .insert(alias, target);
        Ok(())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isAlias(&self, name: impl Into<ServiceKey>) -> bool {
        self.registry
            .read()
            .expect("container lock poisoned")
            .aliases
            .contains_key(&name.into())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn getAlias(&self, name: impl Into<ServiceKey>) -> Result<ServiceKey, BindingResolutionError> {
        self.registry
            .read()
            .expect("container lock poisoned")
            .get_alias(name.into())
    }

    fn has(&self, id: impl Into<ServiceKey>) -> bool {
        let id = id.into();
        let registry = self.registry.read().expect("container lock poisoned");
        registry.bindings.contains_key(&id) || registry.aliases.contains_key(&id)
    }

    fn bound<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.has(ServiceKey::typed::<T>())
    }

    fn make<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError> {
        self.get(ServiceKey::typed::<T>())
    }

    fn get<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        let (key, entry) = {
            let registry = self.registry.read().expect("container lock poisoned");
            let key = registry.get_alias(id.into())?;
            let binding = registry.bindings.get(&key).cloned().ok_or_else(|| {
                BindingResolutionError::NotBound {
                    service: key.clone(),
                }
            })?;
            (key, binding)
        };
        let binding = entry.value.downcast::<Binding<T>>().map_err(|_| {
            BindingResolutionError::TypeMismatch {
                service: key.clone(),
                expected: type_name::<T>(),
                actual: entry.output,
            }
        })?;
        let _resolution = self.resolve(&key)?;
        match binding.as_ref() {
            Binding::Factory(factory) => factory(self),
            Binding::Instance(instance) => Ok(Arc::clone(instance)),
            Binding::Singleton {
                factory,
                state,
                ready,
            } => {
                let caller = thread::current().id();
                let binding = std::ptr::from_ref(state).addr();
                let mut slot = state.lock().expect("singleton state poisoned");
                loop {
                    match &*slot {
                        SingletonState::Ready(instance) => return Ok(Arc::clone(instance)),
                        SingletonState::Empty => {
                            *slot = SingletonState::Running(caller);
                            break;
                        }
                        SingletonState::Running(owner) => {
                            self.wait_for(caller, *owner, key.clone(), binding)?;
                            slot = ready.wait(slot).expect("singleton state poisoned");
                            self.waiting
                                .lock()
                                .expect("container wait graph poisoned")
                                .remove(&caller);
                        }
                    }
                }
                drop(slot);
                let result = panic::catch_unwind(AssertUnwindSafe(|| factory(self)));
                let mut slot = state.lock().expect("singleton state poisoned");
                *slot = match &result {
                    Ok(Ok(instance)) => SingletonState::Ready(Arc::clone(instance)),
                    _ => SingletonState::Empty,
                };
                self.waiting
                    .lock()
                    .expect("container wait graph poisoned")
                    .retain(|_, (_, _, waiting_for)| *waiting_for != binding);
                ready.notify_all();
                drop(slot);
                result.unwrap_or_else(|payload| panic::resume_unwind(payload))
            }
        }
    }
}

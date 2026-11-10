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
    instances: HashMap<ServiceKey, RegisteredBinding>,
    resolved: HashSet<ServiceKey>,
}

#[derive(Clone)]
struct RegisteredBinding {
    value: ErasedBinding,
    output: &'static str,
    instance: bool,
    shared: bool,
    fresh: Option<Arc<dyn Fn() -> ErasedBinding + Send + Sync>>,
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
    Factory(Arc<Factory<T>>),
    Singleton {
        factory: Arc<Factory<T>>,
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

    fn entry<T: ?Sized + Send + Sync + 'static>(binding: Binding<T>) -> RegisteredBinding {
        let instance = matches!(binding, Binding::Instance(_));
        let shared = !matches!(binding, Binding::Factory(_));
        let fresh = match &binding {
            Binding::Singleton { factory, .. } => {
                let factory = factory.clone();
                Some(Arc::new(move || {
                    Arc::new(Binding::Singleton {
                        factory: factory.clone(),
                        state: Mutex::new(SingletonState::Empty),
                        ready: Condvar::new(),
                    }) as ErasedBinding
                })
                    as Arc<dyn Fn() -> ErasedBinding + Send + Sync>)
            }
            _ => None,
        };
        RegisteredBinding {
            value: Arc::new(binding),
            output: type_name::<T>(),
            instance,
            shared,
            fresh,
        }
    }

    fn insert<T: ?Sized + Send + Sync + 'static>(&self, key: ServiceKey, binding: Binding<T>) {
        let entry = Self::entry(binding);
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            registry.aliases.remove(&key);
            if entry.instance {
                (registry.instances.insert(key, entry), None)
            } else {
                let instance = registry.instances.remove(&key);
                (registry.bindings.insert(key, entry), instance)
            }
        };
        drop(previous);
    }

    fn insert_if_absent<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        binding: Binding<T>,
    ) {
        let entry = Self::entry(binding);
        let rejected = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            if registry.bindings.contains_key(&key)
                || registry.instances.contains_key(&key)
                || registry.aliases.contains_key(&key)
            {
                Some(entry)
            } else {
                registry.bindings.insert(key, entry);
                None
            }
        };
        drop(rejected);
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
            Binding::Factory(Arc::new(factory)),
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
                factory: Arc::new(factory),
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
        self.insert(ServiceKey::named(name), Binding::Factory(Arc::new(factory)));
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
                factory: Arc::new(factory),
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
        registry.bindings.contains_key(&id)
            || registry.instances.contains_key(&id)
            || registry.aliases.contains_key(&id)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isShared(&self, id: impl Into<ServiceKey>) -> bool {
        let id = id.into();
        let registry = self.registry.read().expect("container lock poisoned");
        registry.instances.contains_key(&id)
            || registry
                .bindings
                .get(&id)
                .is_some_and(|binding| binding.shared)
    }

    fn resolved(&self, id: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        let registry = self.registry.read().expect("container lock poisoned");
        let key = registry.get_alias(id.into())?;
        Ok(registry.resolved.contains(&key) || registry.instances.contains_key(&key))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetInstance(&self, id: impl Into<ServiceKey>) {
        let key = id.into();
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let instance = registry.instances.remove(&key);
            let cached = registry.bindings.get_mut(&key).and_then(|binding| {
                binding
                    .fresh
                    .as_ref()
                    .map(|fresh| std::mem::replace(&mut binding.value, fresh()))
            });
            (instance, cached)
        };
        drop(previous);
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetInstances(&self) {
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let instances = std::mem::take(&mut registry.instances);
            let cached = registry
                .bindings
                .values_mut()
                .filter_map(|binding| {
                    binding
                        .fresh
                        .as_ref()
                        .map(|fresh| std::mem::replace(&mut binding.value, fresh()))
                })
                .collect::<Vec<_>>();
            (instances, cached)
        };
        drop(previous);
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn bindIf<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_if_absent(
            ServiceKey::typed::<T>(),
            Binding::Factory(Arc::new(factory)),
        );
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIf<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_if_absent(
            ServiceKey::typed::<T>(),
            Binding::Singleton {
                factory: Arc::new(factory),
                state: Mutex::new(SingletonState::Empty),
                ready: Condvar::new(),
            },
        );
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn bindIfNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_if_absent(ServiceKey::named(name), Binding::Factory(Arc::new(factory)));
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIfNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_if_absent(
            ServiceKey::named(name),
            Binding::Singleton {
                factory: Arc::new(factory),
                state: Mutex::new(SingletonState::Empty),
                ready: Condvar::new(),
            },
        );
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
            let binding = registry
                .instances
                .get(&key)
                .or_else(|| registry.bindings.get(&key))
                .cloned()
                .ok_or_else(|| BindingResolutionError::NotBound {
                    service: key.clone(),
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
        let result = match binding.as_ref() {
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
                if matches!(&result, Ok(Ok(_))) {
                    self.registry
                        .write()
                        .expect("container lock poisoned")
                        .resolved
                        .insert(key.clone());
                }
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
        };
        if result.is_ok() && !entry.instance {
            self.registry
                .write()
                .expect("container lock poisoned")
                .resolved
                .insert(key);
        }
        result
    }
}

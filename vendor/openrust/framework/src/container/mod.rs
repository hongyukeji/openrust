//! The framework's service container.

use std::{
    any::{Any, type_name},
    cell::RefCell,
    collections::{HashMap, HashSet},
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc, Condvar, Mutex, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use crate::contracts::container::{self, BindingResolutionError, Container as _};

mod contextual_binding_builder;
pub use contextual_binding_builder::{ContextualBinding, ContextualBindingBuilder};
mod tagged;
pub use tagged::{Tagged, TaggedIter};
mod resolving_hooks;
pub use resolving_hooks::ResolutionEvent;
use resolving_hooks::{Phase, ResolveHooks};
mod rebinding;
use rebinding::RegisteredRebinding;
mod extensions;
use extensions::RegisteredExtender;
mod async_resolution;
pub(crate) mod deferred;
pub use async_resolution::FactoryFuture;
use async_resolution::{AsyncFactory, Owner, OwnerKey};
mod bindings;
pub use bindings::{BindingDefinition, Bindings, FactoryHandle};
mod call;
mod call_context;
mod class_descriptor;
pub use call::{CallArguments, CallFuture, Callable, Callback};
mod method_call;
pub use method_call::{MethodCall, StaticCall};
mod variadic;
pub use class_descriptor::{ClassDescriptor, ConstructorParam, ResolvedArgs};
mod parameters;
mod scope;
pub use parameters::{ParameterKey, ParameterOverrides, ParameterValue};
mod service_key;
pub use service_key::ServiceKey;

type Factory<T> = dyn Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
type ErasedBinding = Arc<dyn Any + Send + Sync>;
type Wait = (usize, ServiceKey, usize);

/// Owns the application's service bindings and shared instances.
#[derive(Default)]
pub struct Container {
    identity: scope::Identity,
    scopes: scope::Lifecycles,
    deferred: OnceLock<Arc<dyn deferred::DeferredServices>>,
    registry: RwLock<Registry>,
    waiting: Mutex<HashMap<(OwnerKey, OwnerKey), Wait>>,
    hook_phases: AtomicU8,
    has_extenders: AtomicBool,
}

#[derive(Default)]
struct Registry {
    methods: HashMap<(ServiceKey, Arc<str>), method_call::RegisteredMethod>,
    method_bindings: HashMap<(ServiceKey, Arc<str>), method_call::RegisteredMethod>,
    classes: HashMap<ServiceKey, ClassDescriptor>,
    bindings: HashMap<ServiceKey, RegisteredBinding>,
    binding_order: Vec<ServiceKey>,
    aliases: HashMap<ServiceKey, ServiceKey>,
    abstract_aliases: HashMap<ServiceKey, Vec<ServiceKey>>,
    instances: HashMap<ServiceKey, RegisteredBinding>,
    resolved: HashSet<ServiceKey>,
    scoped: HashSet<ServiceKey>,
    contextual: HashMap<ServiceKey, HashMap<ServiceKey, RegisteredBinding>>,
    tags: HashMap<String, Vec<ServiceKey>>,
    hooks: ResolveHooks,
    extenders: HashMap<ServiceKey, Vec<RegisteredExtender>>,
    rebound_callbacks: HashMap<ServiceKey, Vec<RegisteredRebinding>>,
}

#[derive(Clone)]
struct RegisteredBinding {
    services: Option<Arc<Vec<ServiceKey>>>,
    value: ErasedBinding,
    output: &'static str,
    version: Arc<()>,
    instance: bool,
    shared: bool,
    fresh: Option<Arc<dyn Fn() -> RegisteredBinding + Send + Sync>>,
    published: Option<Arc<AtomicBool>>,
}

impl RegisteredBinding {
    fn with_version(mut self, version: Arc<()>) -> Self {
        self.version = version;
        self
    }
    fn is_published(&self) -> bool {
        self.published
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
}

struct Mutation<T: ?Sized> {
    key: ServiceKey,
    entry: RegisteredBinding,
    supplied: Option<Arc<T>>,
    callbacks: Vec<RegisteredRebinding>,
}
impl<T: ?Sized + Send + Sync + 'static> Mutation<T> {
    fn dispatch(self, container: &Container) -> Result<(), BindingResolutionError> {
        match self.supplied {
            Some(value) => container.dispatch_rebound_value(self.key, value, self.callbacks),
            None => container.dispatch_rebound::<T>(self.key, self.entry, self.callbacks),
        }
    }
    async fn dispatch_async(self, container: &Container) -> Result<(), BindingResolutionError> {
        match self.supplied {
            Some(value) => container.dispatch_rebound_value(self.key, value, self.callbacks),
            None => {
                container
                    .dispatch_rebound_async::<T>(self.key, self.entry, self.callbacks)
                    .await
            }
        }
    }
}

impl Registry {
    fn remove_alias(&mut self, key: &ServiceKey) {
        if let Some(target) = self.aliases.remove(key)
            && let Some(aliases) = self.abstract_aliases.get_mut(&target)
        {
            aliases.retain(|alias| alias != key);
        }
    }

    fn contextual_binding(
        &self,
        container: &Container,
        consumer: &ServiceKey,
        key: &ServiceKey,
    ) -> Option<RegisteredBinding> {
        let bindings = self.contextual.get(consumer)?;
        bindings
            .get(key)
            .or_else(|| {
                self.abstract_aliases.get(key)?.iter().find_map(|alias| {
                    (!scope::alias_removed(container, alias))
                        .then(|| bindings.get(alias))
                        .flatten()
                })
            })
            .cloned()
    }

    fn resolved_raw(&self, container: &Container, key: &ServiceKey) -> bool {
        let cached = self.bindings.get(key).is_some_and(|entry| {
            if self.scoped.contains(key) && scope::active(container) {
                scope::is_published(container, key, entry)
            } else {
                entry.is_published()
            }
        });
        self.resolved.contains(key)
            || cached
            || scope::lookup(container, key).is_some()
            || (self.instances.contains_key(key) && self.root_instance_visible(container, key))
    }

    fn root_instance_visible(&self, container: &Container, key: &ServiceKey) -> bool {
        !(scope::active(container)
            && (self.scoped.contains(key) || scope::alias_removed(container, key)))
    }

    fn get_alias(
        &self,
        key: ServiceKey,
        blocked: impl Fn(&ServiceKey) -> bool,
    ) -> Result<ServiceKey, BindingResolutionError> {
        let mut current = key;
        let mut seen = HashSet::new();
        let mut path = Vec::new();
        while let Some(next) = self.aliases.get(&current) {
            if blocked(&current) {
                break;
            }
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
    AsyncFactory(Arc<AsyncFactory<T>>),
    AsyncSingleton {
        factory: Arc<AsyncFactory<T>>,
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
        changed: tokio::sync::Notify,
    },
    Singleton {
        factory: Arc<Factory<T>>,
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
        changed: tokio::sync::Notify,
    },
    Instance {
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
        changed: tokio::sync::Notify,
    },
}

enum SingletonState<T: ?Sized> {
    Empty,
    Running(Owner),
    Publishing(Owner, Arc<T>),
    Extending(Owner, Arc<T>),
    Ready(Arc<T>),
}

thread_local! {
    static RESOLVING: RefCell<Vec<ResolutionFrame>> = const { RefCell::new(Vec::new()) };
}

struct ResolutionFrame {
    container: usize,
    key: ServiceKey,
    recipe: usize,
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
    ) -> Result<(), BindingResolutionError> {
        self.insert(key, Self::provided(instance))
    }

    fn provided<T: ?Sized>(value: Arc<T>) -> Binding<T> {
        Binding::Instance {
            state: Mutex::new(SingletonState::Ready(value)),
            ready: Condvar::new(),
            changed: tokio::sync::Notify::new(),
        }
    }

    fn entry<T: ?Sized + Send + Sync + 'static>(binding: Binding<T>) -> RegisteredBinding {
        let instance = matches!(binding, Binding::Instance { .. });
        let shared = !matches!(binding, Binding::Factory(_) | Binding::AsyncFactory(_));
        let fresh = match &binding {
            Binding::Singleton { factory, .. } => {
                let factory = factory.clone();
                Some(Arc::new(move || {
                    Self::entry(Binding::Singleton {
                        factory: factory.clone(),
                        state: Mutex::new(SingletonState::Empty),
                        ready: Condvar::new(),
                        changed: tokio::sync::Notify::new(),
                    })
                })
                    as Arc<dyn Fn() -> RegisteredBinding + Send + Sync>)
            }
            Binding::AsyncSingleton { factory, .. } => {
                let factory = factory.clone();
                Some(Arc::new(move || {
                    Self::entry(Binding::AsyncSingleton {
                        factory: factory.clone(),
                        state: Mutex::new(SingletonState::Empty),
                        ready: Condvar::new(),
                        changed: tokio::sync::Notify::new(),
                    })
                })
                    as Arc<dyn Fn() -> RegisteredBinding + Send + Sync>)
            }
            _ => None,
        };
        RegisteredBinding {
            services: None,
            value: Arc::new(binding),
            output: type_name::<T>(),
            version: Arc::new(()),
            instance,
            shared,
            fresh,
            published: (shared && !instance).then(|| Arc::new(AtomicBool::new(false))),
        }
    }

    fn insert<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        binding: Binding<T>,
    ) -> Result<(), BindingResolutionError> {
        self.commit(key, binding).dispatch(self)
    }

    fn commit<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        binding: Binding<T>,
    ) -> Mutation<T> {
        let supplied = match &binding {
            Binding::Instance { state, .. } => match &*state.lock().expect("new instance state") {
                SingletonState::Ready(value) => Some(value.clone()),
                _ => unreachable!("new explicit instance"),
            },
            _ => None,
        };
        let entry = Self::entry(binding);
        if entry.instance && scope::active(self) && self.scoped_identity(&key) {
            let notify = self.has(key.clone());
            scope::put(self, key.clone(), entry.clone());
            let callbacks = if notify {
                self.rebound_callbacks(&key)
            } else {
                Vec::new()
            };
            return Mutation {
                key,
                entry,
                supplied,
                callbacks,
            };
        }
        let local = (!entry.instance).then(|| key.clone());
        let (previous, callbacks) = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let notify = if entry.instance {
                registry.bindings.contains_key(&key)
                    || registry.instances.contains_key(&key)
                    || registry.aliases.contains_key(&key)
            } else {
                registry.resolved_raw(self, &key)
            };
            let callbacks = if notify {
                registry
                    .rebound_callbacks
                    .get(&key)
                    .cloned()
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            registry.remove_alias(&key);
            let previous = if entry.instance {
                let cached = registry.bindings.get_mut(&key).and_then(|binding| {
                    binding.fresh.clone().map(|fresh| {
                        std::mem::replace(binding, fresh().with_version(binding.version.clone()))
                    })
                });
                (
                    registry.instances.insert(key.clone(), entry.clone()),
                    None,
                    cached,
                )
            } else {
                let instance = registry.instances.remove(&key);
                (
                    registry.insert_binding(key.clone(), entry.clone()),
                    instance,
                    None,
                )
            };
            (previous, callbacks)
        };
        if let Some(key) = local {
            scope::remove(self, &key);
        }
        drop(previous);
        Mutation {
            key,
            entry,
            supplied,
            callbacks,
        }
    }

    fn insert_if_absent<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        binding: Binding<T>,
    ) -> Result<(), BindingResolutionError> {
        self.commit_if_absent(key, binding).dispatch(self)
    }

    fn commit_if_absent<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        binding: Binding<T>,
    ) -> Mutation<T> {
        let entry = Self::entry(binding);
        let mut callbacks = Vec::new();
        let rejected = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            if registry.bindings.contains_key(&key)
                || (registry.instances.contains_key(&key)
                    && registry.root_instance_visible(self, &key))
                || (!scope::alias_removed(self, &key) && registry.aliases.contains_key(&key))
                || scope::lookup(self, &key).is_some()
                || self
                    .deferred
                    .get()
                    .is_some_and(|loader| loader.contains(&key))
            {
                Some(entry.clone())
            } else {
                if registry.resolved_raw(self, &key) {
                    callbacks = registry
                        .rebound_callbacks
                        .get(&key)
                        .cloned()
                        .unwrap_or_default();
                }
                registry.insert_binding(key.clone(), entry.clone());
                None
            }
        };
        drop(rejected);
        Mutation {
            key,
            entry,
            callbacks,
            supplied: None,
        }
    }

    fn scoped_identity(&self, key: &ServiceKey) -> bool {
        if scope::lookup(self, key).is_some() {
            return true;
        }
        let registry = self.registry.read().expect("container lock poisoned");
        registry.scoped.contains(key)
            || registry
                .get_alias(key.clone(), |_| false)
                .is_ok_and(|key| registry.scoped.contains(&key))
    }

    fn insert_scoped<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        factory: Arc<Factory<T>>,
        conditional: bool,
    ) -> Result<(), BindingResolutionError> {
        let entry = Self::entry(Binding::Singleton {
            factory,
            state: Mutex::new(SingletonState::Empty),
            ready: Condvar::new(),
            changed: tokio::sync::Notify::new(),
        });
        self.commit_scoped::<T>(key, entry, conditional)
            .dispatch(self)
    }

    fn commit_scoped<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        entry: RegisteredBinding,
        conditional: bool,
    ) -> Mutation<T> {
        let reset = key.clone();
        let mut callbacks = Vec::new();
        let committed = entry.clone();
        let mut applied = false;
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            if conditional
                && (registry.bindings.contains_key(&key)
                    || (registry.instances.contains_key(&key)
                        && registry.root_instance_visible(self, &key))
                    || (!scope::alias_removed(self, &key) && registry.aliases.contains_key(&key))
                    || scope::lookup(self, &key).is_some()
                    || self
                        .deferred
                        .get()
                        .is_some_and(|loader| loader.contains(&key)))
            {
                (Some(entry), None)
            } else {
                applied = true;
                if registry.resolved_raw(self, &key) {
                    callbacks = registry
                        .rebound_callbacks
                        .get(&key)
                        .cloned()
                        .unwrap_or_default();
                }
                registry.scoped.insert(key.clone());
                registry.remove_alias(&key);
                let instance = registry.instances.remove(&key);
                (registry.insert_binding(key, entry), instance)
            }
        };
        if applied {
            scope::remove(self, &reset);
        }
        drop(previous);
        Mutation {
            key: reset,
            entry: committed,
            callbacks,
            supplied: None,
        }
    }

    fn consumer(&self) -> Option<ServiceKey> {
        if let Some(consumer) = call_context::consumer(self) {
            return consumer;
        }
        let owner = std::ptr::from_ref(self).addr();
        RESOLVING.with_borrow(|stack| {
            stack
                .iter()
                .rev()
                .find(|frame| frame.container == owner)
                .map(|frame| frame.key.clone())
        })
    }

    fn resolve(
        &self,
        key: &ServiceKey,
        recipe: usize,
    ) -> Result<Resolution, BindingResolutionError> {
        let container = std::ptr::from_ref(self).addr();
        RESOLVING.with_borrow_mut(|stack| {
            if let Some(start) = stack.iter().position(|frame| {
                frame.container == container && frame.key == *key && frame.recipe == recipe
            }) {
                let services = stack[start..]
                    .iter()
                    .map(|frame| frame.key.clone())
                    .chain(std::iter::once(key.clone()))
                    .collect();
                return Err(BindingResolutionError::CircularDependency { services });
            }
            stack.push(ResolutionFrame {
                container,
                key: key.clone(),
                recipe,
            });
            Ok(Resolution)
        })
    }

    fn wait_for(
        &self,
        caller: Owner,
        owner: Owner,
        service: ServiceKey,
        binding: usize,
    ) -> Result<(), BindingResolutionError> {
        if caller.is_sync() && !owner.is_sync() {
            return Err(BindingResolutionError::AsyncResolutionRequired { service });
        }
        let mut waiting = self.waiting.lock().expect("container wait graph poisoned");
        let caller = caller.key();
        let owner = owner.key();
        let mut pending = vec![(owner, vec![service.clone()])];
        let mut seen = HashSet::new();
        while let Some((next, path)) = pending.pop() {
            if next == caller {
                let mut services = path;
                services.push(service);
                return Err(BindingResolutionError::CircularDependency { services });
            }
            if seen.insert(next) {
                for ((from, to), (_, name, _)) in waiting.iter() {
                    if *from == next {
                        let mut path = path.clone();
                        path.push(name.clone());
                        pending.push((*to, path));
                    }
                }
            }
        }
        let edge = waiting
            .entry((caller, owner))
            .or_insert((0, service, binding));
        edge.0 += 1;
        Ok(())
    }
}

impl container::Container for Container {
    fn bind<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(
            ServiceKey::typed::<T>(),
            Binding::Factory(Arc::new(factory)),
        )
    }

    fn singleton<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
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
                changed: tokio::sync::Notify::new(),
            },
        )
    }

    fn instance<T: ?Sized + Send + Sync + 'static>(
        &self,
        service: Arc<T>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        let returned = service.clone();
        self.insert(ServiceKey::typed::<T>(), Self::provided(service))?;
        Ok(returned)
    }

    #[allow(non_snake_case, reason = "Named overload for Rust's typed bind")]
    fn bindNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert(ServiceKey::named(name), Binding::Factory(Arc::new(factory)))
    }

    #[allow(non_snake_case, reason = "Named overload for Rust's typed singleton")]
    fn singletonNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
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
                changed: tokio::sync::Notify::new(),
            },
        )
    }

    #[allow(non_snake_case, reason = "Named overload for Rust's typed instance")]
    fn instanceNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        name: &str,
        service: Arc<T>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        let returned = service.clone();
        self.insert(ServiceKey::named(name), Self::provided(service))?;
        Ok(returned)
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
        {
            let mut registry = self.registry.write().expect("container lock poisoned");
            registry.remove_alias(&alias);
            registry
                .abstract_aliases
                .entry(target.clone())
                .or_default()
                .push(alias.clone());
            registry.aliases.insert(alias.clone(), target);
        }
        scope::restore_alias(self, &alias);
        Ok(())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isAlias(&self, name: impl Into<ServiceKey>) -> bool {
        let key = name.into();
        !scope::alias_removed(self, &key)
            && self
                .registry
                .read()
                .expect("container lock poisoned")
                .aliases
                .contains_key(&key)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn getAlias(&self, name: impl Into<ServiceKey>) -> Result<ServiceKey, BindingResolutionError> {
        self.registry
            .read()
            .expect("container lock poisoned")
            .get_alias(name.into(), |key| scope::alias_removed(self, key))
    }

    fn has(&self, id: impl Into<ServiceKey>) -> bool {
        let id = id.into();
        if scope::lookup(self, &id).is_some() {
            return true;
        }
        let registry = self.registry.read().expect("container lock poisoned");
        let bound = registry.bindings.contains_key(&id)
            || (registry.instances.contains_key(&id) && registry.root_instance_visible(self, &id))
            || (!scope::alias_removed(self, &id) && registry.aliases.contains_key(&id));
        drop(registry);
        bound
            || self
                .deferred
                .get()
                .is_some_and(|loader| loader.contains(&id))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isShared(&self, id: impl Into<ServiceKey>) -> bool {
        let id = id.into();
        if scope::lookup(self, &id).is_some() {
            return true;
        }
        let registry = self.registry.read().expect("container lock poisoned");
        (registry.instances.contains_key(&id) && registry.root_instance_visible(self, &id))
            || registry
                .bindings
                .get(&id)
                .is_some_and(|binding| binding.shared)
    }

    fn resolved(&self, id: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        let registry = self.registry.read().expect("container lock poisoned");
        let key = registry.get_alias(id.into(), |key| scope::alias_removed(self, key))?;
        Ok(registry.resolved_raw(self, &key))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetInstance(&self, id: impl Into<ServiceKey>) {
        let key = id.into();
        if scope::active(self)
            && (scope::lookup(self, &key).is_some()
                || self
                    .registry
                    .read()
                    .expect("container lock poisoned")
                    .scoped
                    .contains(&key))
        {
            scope::remove(self, &key);
            return;
        }
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let instance = registry.instances.remove(&key);
            let cached = registry.bindings.get_mut(&key).and_then(|binding| {
                binding.fresh.clone().map(|fresh| {
                    std::mem::replace(binding, fresh().with_version(binding.version.clone()))
                })
            });
            (instance, cached)
        };
        drop(previous);
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetInstances(&self) {
        scope::clear(self);
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let instances = std::mem::take(&mut registry.instances);
            let cached = registry
                .bindings
                .values_mut()
                .filter_map(|binding| {
                    binding.fresh.clone().map(|fresh| {
                        std::mem::replace(binding, fresh().with_version(binding.version.clone()))
                    })
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
    fn bindIf<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_if_absent(
            ServiceKey::typed::<T>(),
            Binding::Factory(Arc::new(factory)),
        )
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIf<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
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
                changed: tokio::sync::Notify::new(),
            },
        )
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn bindIfNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_if_absent(ServiceKey::named(name), Binding::Factory(Arc::new(factory)))
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIfNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
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
                changed: tokio::sync::Notify::new(),
            },
        )
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetScopedInstances(&self) {
        if scope::active(self) {
            scope::clear(self);
        } else {
            let keys = self
                .registry
                .read()
                .expect("container lock poisoned")
                .scoped
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            for key in keys {
                self.forgetInstance(key);
            }
        }
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scoped<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::typed::<T>(), Arc::new(factory), false)
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIf<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::typed::<T>(), Arc::new(factory), true)
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::named(name), Arc::new(factory), false)
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIfNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::named(name), Arc::new(factory), true)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn addContextualBinding<T, F>(
        &self,
        concrete: impl Into<ServiceKey>,
        r#abstract: impl Into<ServiceKey>,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        let key = self.getAlias(r#abstract)?;
        let entry = Self::entry(Binding::Factory(Arc::new(factory)));
        let previous = self
            .registry
            .write()
            .expect("container lock poisoned")
            .contextual
            .entry(concrete.into())
            .or_default()
            .insert(key, entry);
        drop(previous);
        Ok(())
    }

    fn tag(
        &self,
        abstracts: impl IntoIterator<Item = impl Into<ServiceKey>>,
        tags: impl IntoIterator<Item = impl Into<String>>,
    ) {
        let services: Vec<ServiceKey> = abstracts.into_iter().map(Into::into).collect();
        let tags: Vec<String> = tags.into_iter().map(Into::into).collect();
        let mut registry = self.registry.write().expect("container lock poisoned");
        for tag in tags {
            registry
                .tags
                .entry(tag)
                .or_default()
                .extend(services.iter().cloned());
        }
    }

    fn tagged<T: ?Sized + Send + Sync + 'static>(&self, tag: &str) -> Tagged<'_, Self, T> {
        let (tag, count) = self
            .registry
            .read()
            .expect("container lock poisoned")
            .tags
            .get(tag)
            .map_or((None, 0), |keys| (Some(tag.to_owned()), keys.len()));
        Tagged::new(self, count, move || {
            tag.as_ref()
                .and_then(|tag| {
                    self.registry
                        .read()
                        .expect("container lock poisoned")
                        .tags
                        .get(tag)
                        .cloned()
                })
                .unwrap_or_default()
        })
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container callbacks")]
    fn beforeResolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        self.register_hook(selector, Phase::Before, callback)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container callbacks")]
    fn resolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        self.register_hook(selector, Phase::Resolving, callback)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container callbacks")]
    fn afterResolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        self.register_hook(selector, Phase::After, callback)
    }

    fn rebinding<T, F>(
        &self,
        r#abstract: impl Into<ServiceKey>,
        callback: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        self.register_rebinding(r#abstract, callback)
    }

    fn extend<T, F>(
        &self,
        r#abstract: impl Into<ServiceKey>,
        closure: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.extend_value(r#abstract, closure)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetExtenders(
        &self,
        r#abstract: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError> {
        self.forget_extenders(r#abstract)
    }

    #[allow(
        non_snake_case,
        reason = "Async companion to Laravel container resolution"
    )]
    async fn getAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey> + Send,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.get_async(id.into()).await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn bindAsync<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::typed::<T>(), factory, false, false, false)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn bindAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::named(name), factory, false, false, false)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn bindIfAsync<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::typed::<T>(), factory, false, false, true)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn bindIfAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::named(name), factory, false, false, true)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn singletonAsync<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::typed::<T>(), factory, true, false, false)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn singletonAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::named(name), factory, true, false, false)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn singletonIfAsync<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::typed::<T>(), factory, true, false, true)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn singletonIfAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::named(name), factory, true, false, true)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn scopedAsync<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::typed::<T>(), factory, true, true, false)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn scopedAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::named(name), factory, true, true, false)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn scopedIfAsync<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::typed::<T>(), factory, true, true, true)
            .await
    }

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    async fn scopedIfAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(ServiceKey::named(name), factory, true, true, true)
            .await
    }

    #[allow(non_snake_case, reason = "Async companion to Laravel rebinding")]
    async fn rebindingAsync<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        callback: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        self.register_rebinding_async(key, callback).await
    }

    #[allow(non_snake_case, reason = "Async companion to Laravel extend")]
    async fn extendAsync<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        closure: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.extend_async_value(key, closure).await
    }

    #[allow(non_snake_case, reason = "Async contextual factory adapter")]
    fn addContextualBindingAsync<T, F>(
        &self,
        consumer: impl Into<ServiceKey>,
        need: impl Into<ServiceKey>,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        let need = self.getAlias(need)?;
        let entry = Self::entry(Binding::AsyncFactory(Arc::new(factory)));
        let previous = self
            .registry
            .write()
            .expect("container lock poisoned")
            .contextual
            .entry(consumer.into())
            .or_default()
            .insert(need, entry);
        drop(previous);
        Ok(())
    }

    #[allow(non_snake_case, reason = "Typed Rust contextual array adapter")]
    fn addContextualServices<T: ?Sized + Send + Sync + 'static>(
        &self,
        consumer: impl Into<ServiceKey>,
        need: impl Into<ServiceKey>,
        services: Vec<ServiceKey>,
    ) -> Result<(), BindingResolutionError> {
        self.contextual_services::<T>(consumer.into(), need.into(), services)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn registerMethod<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        signature: Vec<ConstructorParam>,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(Arc<C>, CallArguments, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        self.declare_method::<C, R, F>(name, signature, callback)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn registerStaticMethod<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        signature: Vec<ConstructorParam>,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(CallArguments, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        self.declare_static::<C, R, F>(name, signature, callback)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn bindMethod<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(Arc<C>, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        self.bind_method::<C, R, F>(name, callback)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn bindStaticMethod<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(ServiceKey, &'a Self) -> CallFuture<'a, R> + Send + Sync + 'static,
    {
        self.bind_static::<C, R, F>(name, callback)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn hasMethodBinding<C: ?Sized + 'static>(&self, name: &str) -> bool {
        self.has_method::<C>(name)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn callMethodBinding<'a, C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &'a self,
        name: &'a str,
        receiver: Arc<C>,
    ) -> impl std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a {
        self.invoke_binding(name, receiver)
    }
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn callStaticMethodBinding<'a, C: ?Sized + 'static, R: Send + 'static>(
        &'a self,
        name: &'a str,
    ) -> impl std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a {
        self.invoke_static_binding::<C, R>(name)
    }
    #[allow(non_snake_case, reason = "Matches Laravel's binding directory API")]
    fn getBindings(&self) -> Bindings {
        self.bindings_snapshot()
    }
    fn flush(&self) {
        self.flush_bindings();
    }
    fn factory<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Fn() -> Result<Arc<T>, BindingResolutionError> + Send + Sync {
        move || self.make::<T>()
    }
    #[allow(non_snake_case, reason = "Named Rust factory overload")]
    fn factoryNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
    ) -> impl Fn() -> Result<Arc<T>, BindingResolutionError> + Send + Sync {
        move || self.get::<T>(key.clone())
    }
    #[allow(non_snake_case, reason = "Async Rust factory overload")]
    fn factoryAsync<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
    ) -> impl Fn() -> FactoryFuture<'a, T> + Send + Sync {
        move || Box::pin(self.makeAsync::<T>())
    }
    #[allow(non_snake_case, reason = "Async named Rust factory overload")]
    fn factoryAsyncNamed<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
        key: ServiceKey,
    ) -> impl Fn() -> FactoryFuture<'a, T> + Send + Sync {
        move || Box::pin(self.getAsync::<T>(key.clone()))
    }
    fn wrap<'a, R, F, Fut>(
        &'a self,
        callback: Callback<'a, F>,
        parameters: ParameterOverrides,
    ) -> impl Fn() -> CallFuture<'a, R> + Send + Sync + 'a
    where
        R: Send + 'a,
        F: Fn(CallArguments) -> Fut + Send + Sync + 'a,
        Fut: std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a,
    {
        self.wrap_callable(callback, parameters)
    }
    async fn call<'a, R: Send + 'a, Target: Callable<'a, R> + 'a>(
        &'a self,
        target: Target,
        parameters: ParameterOverrides,
    ) -> Result<R, BindingResolutionError> {
        scope::check_open(self)?;
        target.invoke(self, parameters).await
    }
    fn bound<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.has(ServiceKey::typed::<T>())
    }

    fn make<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError> {
        self.get(ServiceKey::typed::<T>())
    }

    #[allow(non_snake_case, reason = "Static Rust reflection adapter")]
    fn registerClass(&self, descriptor: ClassDescriptor) -> Result<(), BindingResolutionError> {
        self.register_class(descriptor)
    }
    fn build<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError> {
        self.build_class()
    }
    #[allow(non_snake_case, reason = "Asynchronous Rust build adapter")]
    async fn buildAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.build_class_async().await
    }

    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    fn getLastParameterOverride(&self) -> ParameterOverrides {
        parameters::current(self)
    }
    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    fn makeWith<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.make_with_parameters(ServiceKey::typed::<T>(), parameters)
    }
    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    fn makeWithNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey>,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.make_with_parameters(key.into(), parameters)
    }
    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    async fn makeWithAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.get_async_with_parameters(ServiceKey::typed::<T>(), parameters)
            .await
    }
    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    async fn makeWithAsyncNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.get_async_with_parameters(key.into(), parameters).await
    }

    fn get<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        self.make_with_parameters(id.into(), ParameterOverrides::default())
    }
}

impl Container {
    fn make_with_parameters<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: ServiceKey,
        values: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        parameters::run(self, values, || {
            scope::check_open(self)?;
            let id = self.require_sync(id)?;
            let (id, canonical) = if self.has_hooks(Phase::Before) {
                let key = self.getAlias(id)?;
                self.fire_hooks(
                    &ResolutionEvent::before_with(key.clone(), parameters::current(self)),
                    Phase::Before,
                )?;
                (key, true)
            } else {
                (id, false)
            };
            let (key, entry, scoped, contextual) = self.select_entry(id, canonical, false)?;
            self.resolve_entry(key, entry, scoped, contextual)
        })
    }

    fn current_recipe(
        &self,
        key: &ServiceKey,
        entry: &RegisteredBinding,
        contextual: bool,
    ) -> bool {
        if contextual {
            return false;
        }
        let local = scope::lookup(self, key);
        let cached = scope::cached_entry(self, key, entry);
        let registry = self.registry.read().expect("container lock poisoned");
        let root = if registry.root_instance_visible(self, key) {
            registry
                .instances
                .get(key)
                .or_else(|| registry.bindings.get(key))
        } else {
            registry.bindings.get(key)
        };
        let current = if let Some(local) = &local {
            Arc::ptr_eq(&local.value, &entry.value)
        } else if entry.shared
            && scope::active(self)
            && registry.scoped.contains(key)
            && root.is_some_and(|root| !root.instance)
        {
            root.is_some_and(|root| Arc::ptr_eq(&root.version, &entry.version))
                && cached
                    .as_ref()
                    .is_some_and(|cached| Arc::ptr_eq(&cached.value, &entry.value))
        } else if let Some(root) = root {
            Arc::ptr_eq(&root.value, &entry.value)
        } else {
            registry.classes.get(key).is_some_and(|record| {
                Arc::ptr_eq(&record.sync.version, &entry.version)
                    && (Arc::ptr_eq(&record.sync.value, &entry.value)
                        || Arc::ptr_eq(&record.asynchronous.value, &entry.value))
            })
        };
        drop(registry);
        current
    }

    fn select_entry(
        &self,
        id: ServiceKey,
        canonical: bool,
        asynchronous: bool,
    ) -> Result<(ServiceKey, RegisteredBinding, bool, bool), BindingResolutionError> {
        let consumer = self.consumer();
        let parameters = parameters::current(self);
        let parameterized = !parameters.is_empty();
        let selected = {
            let registry = self.registry.read().expect("container lock poisoned");
            let key = if canonical {
                id
            } else {
                registry.get_alias(id, |key| scope::alias_removed(self, key))?
            };
            let scoped = registry.scoped.contains(&key);
            let provided = if parameterized {
                None
            } else {
                scope::lookup(self, &key)
            };
            let root = if parameterized || !registry.root_instance_visible(self, &key) {
                registry.bindings.get(&key)
            } else {
                registry
                    .instances
                    .get(&key)
                    .or_else(|| registry.bindings.get(&key))
            };
            let contextual = consumer
                .as_ref()
                .and_then(|consumer| registry.contextual_binding(self, consumer, &key));
            let has_contextual = contextual.is_some();
            let mut entry = contextual
                .or(provided)
                .or_else(|| root.cloned())
                .or_else(|| {
                    registry.classes.get(&key).map(|record| {
                        if asynchronous {
                            record.asynchronous.clone()
                        } else {
                            record.sync.clone()
                        }
                    })
                })
                .ok_or_else(|| BindingResolutionError::NotBound {
                    service: key.clone(),
                })?;
            if parameterized {
                if let Some(fresh) = &entry.fresh {
                    entry = fresh();
                }
                entry.version = parameters.identity().expect("nonempty parameters");
            }
            (
                key,
                entry,
                scoped && !parameterized,
                has_contextual || parameterized,
            )
        };
        Ok(selected)
    }

    fn resolve_entry<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        mut entry: RegisteredBinding,
        scoped: bool,
        contextual: bool,
    ) -> Result<Arc<T>, BindingResolutionError> {
        if scoped && entry.shared && !entry.instance {
            entry = scope::select(self, &key, &entry);
        }
        let binding = entry.value.downcast_ref::<Binding<T>>().ok_or_else(|| {
            BindingResolutionError::TypeMismatch {
                service: key.clone(),
                expected: type_name::<T>(),
                actual: entry.output,
            }
        })?;
        if let Binding::Instance { state, ready, .. } = binding {
            return self.read_instance(&key, state, ready);
        }
        let resolution = self.resolve(&key, Arc::as_ptr(&entry.version).addr())?;
        match binding {
            Binding::AsyncFactory(_) => {
                Err(BindingResolutionError::AsyncResolutionRequired { service: key })
            }
            Binding::AsyncSingleton { state, ready, .. } => {
                if matches!(
                    &*state.lock().expect("singleton state poisoned"),
                    SingletonState::Empty
                ) {
                    return Err(BindingResolutionError::AsyncResolutionRequired { service: key });
                }
                self.read_instance(&key, state, ready)
            }
            Binding::Factory(factory) => {
                self.check_extenders::<T>(&key)?;
                let value = self.decorate(&key, factory(self)?, None)?;
                drop(resolution);
                let current = self.current_recipe(&key, &entry, contextual);
                self.fire_constructed(&key, value.clone())?;
                if current {
                    self.registry
                        .write()
                        .expect("container lock poisoned")
                        .resolved
                        .insert(key);
                }
                Ok(value)
            }
            Binding::Instance { state, ready, .. } => self.read_instance(&key, state, ready),
            Binding::Singleton {
                factory,
                state,
                ready,
                changed,
            } => {
                let caller = Owner::current_thread();
                let binding = std::ptr::from_ref(state).addr();
                let mut slot = state.lock().expect("singleton state poisoned");
                loop {
                    match &*slot {
                        SingletonState::Ready(instance) => return Ok(Arc::clone(instance)),
                        SingletonState::Publishing(owner, instance)
                        | SingletonState::Extending(owner, instance)
                            if owner.is_current() =>
                        {
                            return Ok(Arc::clone(instance));
                        }
                        SingletonState::Empty => {
                            *slot = SingletonState::Running(caller.clone());
                            break;
                        }
                        SingletonState::Running(owner)
                        | SingletonState::Publishing(owner, _)
                        | SingletonState::Extending(owner, _) => {
                            self.wait_for(caller.clone(), owner.clone(), key.clone(), binding)?;
                            slot = ready.wait(slot).expect("singleton state poisoned");
                            self.waiting
                                .lock()
                                .expect("container wait graph poisoned")
                                .retain(|(from, _), _| *from != caller.key());
                        }
                    }
                }
                drop(slot);
                let constructed = panic::catch_unwind(AssertUnwindSafe(|| {
                    self.check_extenders::<T>(&key)?;
                    self.decorate(&key, factory(self)?, Some((state, &entry, caller.clone())))
                }));
                drop(resolution);
                let mut value = None;
                let current = self.current_recipe(&key, &entry, contextual);
                let result = match constructed {
                    Ok(Ok(instance)) => {
                        value = Some(instance.clone());
                        panic::catch_unwind(AssertUnwindSafe(|| {
                            self.fire_constructed(&key, instance.clone())
                                .map(|()| instance)
                        }))
                    }
                    other => other,
                };
                if current && matches!(&result, Ok(Ok(_))) {
                    self.registry
                        .write()
                        .expect("container lock poisoned")
                        .resolved
                        .insert(key);
                }
                {
                    let mut slot = state.lock().expect("singleton state poisoned");
                    let cached = match &*slot {
                        SingletonState::Publishing(_, instance) => Some(instance.clone()),
                        _ => value,
                    };
                    *slot = cached.map_or(SingletonState::Empty, SingletonState::Ready);
                }
                self.waiting
                    .lock()
                    .expect("container wait graph poisoned")
                    .retain(|_, (_, _, waiting_for)| *waiting_for != binding);
                ready.notify_all();
                changed.notify_waiters();
                result.unwrap_or_else(|payload| panic::resume_unwind(payload))
            }
        }
    }
}

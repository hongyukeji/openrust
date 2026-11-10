//! The framework's service container.

use std::{
    any::{Any, type_name},
    cell::RefCell,
    collections::{HashMap, HashSet},
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc, Condvar, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::{self, ThreadId},
};

use crate::contracts::container::{self, BindingResolutionError};

mod contextual_binding_builder;
pub use contextual_binding_builder::{ContextualBinding, ContextualBindingBuilder};
mod tagged;
pub use tagged::{Tagged, TaggedIter};
mod resolving_hooks;
pub use resolving_hooks::ResolutionEvent;
use resolving_hooks::{Phase, ResolveHooks};
mod extensions;
use extensions::RegisteredExtender;
mod scope;
mod service_key;
pub use service_key::ServiceKey;

type Factory<T> = dyn Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
type ErasedBinding = Arc<dyn Any + Send + Sync>;
type Wait = (ThreadId, ServiceKey, usize);

/// Owns the application's service bindings and shared instances.
#[derive(Default)]
pub struct Container {
    identity: scope::Identity,
    registry: RwLock<Registry>,
    waiting: Mutex<HashMap<ThreadId, Wait>>,
    hook_phases: AtomicU8,
    has_extenders: AtomicBool,
}

#[derive(Default)]
struct Registry {
    bindings: HashMap<ServiceKey, RegisteredBinding>,
    aliases: HashMap<ServiceKey, ServiceKey>,
    abstract_aliases: HashMap<ServiceKey, Vec<ServiceKey>>,
    instances: HashMap<ServiceKey, RegisteredBinding>,
    resolved: HashSet<ServiceKey>,
    scoped: HashSet<ServiceKey>,
    contextual: HashMap<ServiceKey, HashMap<ServiceKey, RegisteredBinding>>,
    tags: HashMap<String, Vec<ServiceKey>>,
    hooks: ResolveHooks,
    extenders: HashMap<ServiceKey, Vec<RegisteredExtender>>,
}

#[derive(Clone)]
struct RegisteredBinding {
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
    Singleton {
        factory: Arc<Factory<T>>,
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
    },
    Instance {
        state: Mutex<SingletonState<T>>,
        ready: Condvar,
    },
}

enum SingletonState<T: ?Sized> {
    Empty,
    Running(ThreadId),
    Publishing(ThreadId, Arc<T>),
    Extending(ThreadId, Arc<T>),
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
    ) {
        self.insert(key, Self::provided(instance));
    }

    fn provided<T: ?Sized>(value: Arc<T>) -> Binding<T> {
        Binding::Instance {
            state: Mutex::new(SingletonState::Ready(value)),
            ready: Condvar::new(),
        }
    }

    fn entry<T: ?Sized + Send + Sync + 'static>(binding: Binding<T>) -> RegisteredBinding {
        let instance = matches!(binding, Binding::Instance { .. });
        let shared = !matches!(binding, Binding::Factory(_));
        let fresh = match &binding {
            Binding::Singleton { factory, .. } => {
                let factory = factory.clone();
                Some(Arc::new(move || {
                    Self::entry(Binding::Singleton {
                        factory: factory.clone(),
                        state: Mutex::new(SingletonState::Empty),
                        ready: Condvar::new(),
                    })
                })
                    as Arc<dyn Fn() -> RegisteredBinding + Send + Sync>)
            }
            _ => None,
        };
        RegisteredBinding {
            value: Arc::new(binding),
            output: type_name::<T>(),
            version: Arc::new(()),
            instance,
            shared,
            fresh,
            published: (shared && !instance).then(|| Arc::new(AtomicBool::new(false))),
        }
    }

    fn insert<T: ?Sized + Send + Sync + 'static>(&self, key: ServiceKey, binding: Binding<T>) {
        let entry = Self::entry(binding);
        if entry.instance && scope::active(self) && self.scoped_identity(&key) {
            scope::put(self, key, entry);
            return;
        }
        let local = (!entry.instance).then(|| key.clone());
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            registry.remove_alias(&key);
            if entry.instance {
                let cached = registry.bindings.get_mut(&key).and_then(|binding| {
                    binding.fresh.clone().map(|fresh| {
                        std::mem::replace(binding, fresh().with_version(binding.version.clone()))
                    })
                });
                (registry.instances.insert(key, entry), None, cached)
            } else {
                let instance = registry.instances.remove(&key);
                (registry.bindings.insert(key, entry), instance, None)
            }
        };
        if let Some(key) = local {
            scope::remove(self, &key);
        }
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
                || (registry.instances.contains_key(&key)
                    && registry.root_instance_visible(self, &key))
                || (!scope::alias_removed(self, &key) && registry.aliases.contains_key(&key))
                || scope::lookup(self, &key).is_some()
            {
                Some(entry)
            } else {
                registry.bindings.insert(key, entry);
                None
            }
        };
        drop(rejected);
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
    ) {
        let entry = Self::entry(Binding::Singleton {
            factory,
            state: Mutex::new(SingletonState::Empty),
            ready: Condvar::new(),
        });
        let reset = key.clone();
        let mut applied = false;
        let previous = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            if conditional
                && (registry.bindings.contains_key(&key)
                    || (registry.instances.contains_key(&key)
                        && registry.root_instance_visible(self, &key))
                    || (!scope::alias_removed(self, &key) && registry.aliases.contains_key(&key))
                    || scope::lookup(self, &key).is_some())
            {
                (Some(entry), None)
            } else {
                applied = true;
                registry.scoped.insert(key.clone());
                registry.remove_alias(&key);
                let instance = registry.instances.remove(&key);
                (registry.bindings.insert(key, entry), instance)
            }
        };
        if applied {
            scope::remove(self, &reset);
        }
        drop(previous);
    }

    fn consumer(&self) -> Option<ServiceKey> {
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
        self.insert(ServiceKey::typed::<T>(), Self::provided(service));
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
        self.insert(ServiceKey::named(name), Self::provided(service));
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
        registry.bindings.contains_key(&id)
            || (registry.instances.contains_key(&id) && registry.root_instance_visible(self, &id))
            || (!scope::alias_removed(self, &id) && registry.aliases.contains_key(&id))
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
        let cached = registry.bindings.get(&key).is_some_and(|entry| {
            if registry.scoped.contains(&key) && scope::active(self) {
                scope::is_published(self, &key, entry)
            } else {
                entry.is_published()
            }
        });
        Ok(registry.resolved.contains(&key)
            || cached
            || scope::lookup(self, &key).is_some()
            || (registry.instances.contains_key(&key)
                && registry.root_instance_visible(self, &key)))
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
    fn scoped<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::typed::<T>(), Arc::new(factory), false);
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIf<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::typed::<T>(), Arc::new(factory), true);
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::named(name), Arc::new(factory), false);
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIfNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        self.insert_scoped(ServiceKey::named(name), Arc::new(factory), true);
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
        let (id, canonical) = if self.has_hooks(Phase::Before) {
            let key = self.getAlias(id)?;
            self.fire_hooks(&ResolutionEvent::before(key.clone()), Phase::Before)?;
            (key, true)
        } else {
            (id.into(), false)
        };
        let consumer = self.consumer();
        let (key, mut entry, scoped, contextual) = {
            let registry = self.registry.read().expect("container lock poisoned");
            let key = if canonical {
                id
            } else {
                registry.get_alias(id, |key| scope::alias_removed(self, key))?
            };
            let scoped = registry.scoped.contains(&key);
            let provided = scope::lookup(self, &key);
            let root = if !registry.root_instance_visible(self, &key) {
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
            let entry = contextual
                .or(provided)
                .or_else(|| root.cloned())
                .ok_or_else(|| BindingResolutionError::NotBound {
                    service: key.clone(),
                })?;
            (key, entry, scoped, has_contextual)
        };
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
        if let Binding::Instance { state, ready } = binding {
            return self.read_instance(&key, state, ready);
        }
        let resolution = self.resolve(&key, Arc::as_ptr(&entry.version).addr())?;
        match binding {
            Binding::Factory(factory) => {
                self.check_extenders::<T>(&key)?;
                let value = self.decorate(&key, factory(self)?, None)?;
                drop(resolution);
                self.fire_constructed(&key, value.clone())?;
                if !contextual {
                    self.registry
                        .write()
                        .expect("container lock poisoned")
                        .resolved
                        .insert(key);
                }
                Ok(value)
            }
            Binding::Instance { state, ready } => self.read_instance(&key, state, ready),
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
                        SingletonState::Publishing(owner, instance)
                        | SingletonState::Extending(owner, instance)
                            if *owner == caller =>
                        {
                            return Ok(Arc::clone(instance));
                        }
                        SingletonState::Empty => {
                            *slot = SingletonState::Running(caller);
                            break;
                        }
                        SingletonState::Running(owner)
                        | SingletonState::Publishing(owner, _)
                        | SingletonState::Extending(owner, _) => {
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
                let constructed = panic::catch_unwind(AssertUnwindSafe(|| {
                    self.check_extenders::<T>(&key)?;
                    self.decorate(&key, factory(self)?, Some((state, &entry, caller)))
                }));
                drop(resolution);
                let mut value = None;
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
                if matches!(&result, Ok(Ok(_))) {
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
                result.unwrap_or_else(|payload| panic::resume_unwind(payload))
            }
        }
    }
}

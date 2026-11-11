//! Ordered, independently owned snapshots of Laravel's concrete/shared binding definitions.
use super::{
    Binding, Container, ParameterOverrides, RegisteredBinding, Registry, ServiceKey, parameters,
    scope,
};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    any::type_name,
    fmt,
    sync::{Arc, Mutex},
};
type Result<T> = std::result::Result<T, BindingResolutionError>;

static INSTANCE: Mutex<Option<Arc<Container>>> = Mutex::new(None);

#[allow(
    non_snake_case,
    reason = "Matches Laravel's container instance and ArrayAccess APIs"
)]
impl Container {
    /// Returns the active application's container or the lazily created process default.
    pub fn getInstance() -> Arc<Self> {
        crate::support::facades::current_container().unwrap_or_else(|| {
            INSTANCE
                .lock()
                .expect("container instance lock poisoned")
                .get_or_insert_with(Arc::default)
                .clone()
        })
    }

    /// Replaces the process default and returns the assigned value.
    pub fn setInstance(container: Option<Arc<Self>>) -> Option<Arc<Self>> {
        let previous = std::mem::replace(
            &mut *INSTANCE.lock().expect("container instance lock poisoned"),
            container.clone(),
        );
        drop(previous);
        container
    }

    pub fn offsetExists(&self, key: impl Into<ServiceKey>) -> bool {
        self.has(key)
    }

    pub fn offsetGet<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey>,
    ) -> Result<Arc<T>> {
        self.get(key)
    }

    pub async fn offsetGetAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
    ) -> Result<Arc<T>> {
        self.getAsync(key).await
    }

    /// Binds a value through a transient factory that returns the supplied object.
    pub fn offsetSet<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey>,
        value: Arc<T>,
    ) -> Result<()> {
        self.offsetSetFactory(key, move |_| Ok(value.clone()))
    }

    pub fn offsetSetFactory<T, F>(&self, key: impl Into<ServiceKey>, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        self.insert(key.into(), Binding::Factory(Arc::new(factory)))
    }

    pub async fn offsetSetFactoryAsync<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        factory: F,
    ) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> super::FactoryFuture<'a, T> + Send + Sync + 'static,
    {
        self.register_async(key.into(), factory, false, false, false)
            .await
    }

    /// Removes the raw key's binding, instances, and resolution record.
    pub fn offsetUnset(&self, key: impl Into<ServiceKey>) {
        let key = key.into();
        let retired = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            let binding = registry.bindings.remove(&key);
            if binding.is_some() {
                registry.binding_order.retain(|entry| *entry != key);
            }
            registry.resolved.remove(&key);
            (
                binding,
                registry.instances.remove(&key),
                self.scopes.detach_key(self, &key),
            )
        };
        drop(retired);
    }
}

#[derive(Clone)]
pub struct FactoryHandle {
    key: ServiceKey,
    entry: RegisteredBinding,
}
impl fmt::Debug for FactoryHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FactoryHandle")
            .field("key", &self.key)
            .field("output", &self.entry.output)
            .finish_non_exhaustive()
    }
}
impl FactoryHandle {
    #[allow(
        non_snake_case,
        reason = "Rust identity adapter for original PHP closures"
    )]
    pub fn sameFactory(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.entry.version, &other.entry.version)
    }
    fn typed<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<Binding<T>>> {
        self.entry
            .value
            .clone()
            .downcast()
            .map_err(|_| BindingResolutionError::TypeMismatch {
                service: self.key.clone(),
                expected: type_name::<T>(),
                actual: self.entry.output,
            })
    }
    /// Call the retained original factory using the supplied container and explicit parameters.
    pub fn call<T: ?Sized + Send + Sync + 'static>(
        &self,
        container: &Container,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>> {
        parameters::run(container, parameters, || {
            scope::check_open(container)?;
            let binding = self.typed::<T>()?;
            match binding.as_ref() {
                Binding::Factory(factory) | Binding::Singleton { factory, .. } => {
                    factory(container)
                }
                Binding::AsyncFactory(_) | Binding::AsyncSingleton { .. } => {
                    Err(BindingResolutionError::AsyncResolutionRequired {
                        service: self.key.clone(),
                    })
                }
                Binding::Instance { .. } => {
                    unreachable!("binding snapshots exclude explicit instances")
                }
            }
        })
    }
    #[allow(non_snake_case, reason = "Async Rust original-factory overload")]
    pub async fn callAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
        container: &Container,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>> {
        parameters::run_async(
            container,
            parameters,
            scope::preserve(container, async {
                scope::check_open(container)?;
                let binding = self.typed::<T>()?;
                match binding.as_ref() {
                    Binding::Factory(factory) | Binding::Singleton { factory, .. } => {
                        factory(container)
                    }
                    Binding::AsyncFactory(factory) | Binding::AsyncSingleton { factory, .. } => {
                        factory(container).await
                    }
                    Binding::Instance { .. } => {
                        unreachable!("binding snapshots exclude explicit instances")
                    }
                }
            }),
        )
        .await
    }
}
#[derive(Clone, Debug)]
pub struct BindingDefinition {
    pub concrete: FactoryHandle,
    pub shared: bool,
}
/// PHP's insertion-ordered associative array, with exact ServiceKey lookup and owned definitions.
#[derive(Clone, Debug, Default)]
pub struct Bindings(Vec<(ServiceKey, BindingDefinition)>);
impl Bindings {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, key: &ServiceKey) -> Option<&BindingDefinition> {
        self.0
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, entry)| entry)
    }
    pub fn get_mut(&mut self, key: &ServiceKey) -> Option<&mut BindingDefinition> {
        self.0
            .iter_mut()
            .find(|(known, _)| known == key)
            .map(|(_, entry)| entry)
    }
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (&ServiceKey, &BindingDefinition)> + DoubleEndedIterator
    {
        self.0.iter().map(|(key, entry)| (key, entry))
    }
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &ServiceKey> + DoubleEndedIterator {
        self.0.iter().map(|(key, _)| key)
    }
}
impl IntoIterator for Bindings {
    type Item = (ServiceKey, BindingDefinition);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl Registry {
    pub(super) fn insert_binding(
        &mut self,
        key: ServiceKey,
        entry: RegisteredBinding,
    ) -> Option<RegisteredBinding> {
        if !self.bindings.contains_key(&key) {
            self.binding_order.push(key.clone());
        }
        self.bindings.insert(key, entry)
    }
}
impl Container {
    pub(super) fn bindings_snapshot(&self) -> Bindings {
        let registry = self.registry.read().expect("container lock poisoned");
        Bindings(
            registry
                .binding_order
                .iter()
                .filter_map(|key| {
                    let entry = registry.bindings.get(key)?;
                    let shared = entry.shared;
                    // Retain the recipe without retaining a warmed singleton's cached object.
                    let recipe = entry
                        .fresh
                        .as_ref()
                        .map(|fresh| fresh().with_version(entry.version.clone()))
                        .unwrap_or_else(|| entry.clone());
                    Some((
                        key.clone(),
                        BindingDefinition {
                            concrete: FactoryHandle {
                                key: key.clone(),
                                entry: recipe,
                            },
                            shared,
                        },
                    ))
                })
                .collect(),
        )
    }
    pub(super) fn flush_bindings(&self) {
        let retired = {
            let mut registry = self.registry.write().expect("container lock poisoned");
            registry.binding_order.clear();
            (
                std::mem::take(&mut registry.aliases),
                std::mem::take(&mut registry.abstract_aliases),
                std::mem::take(&mut registry.resolved),
                std::mem::take(&mut registry.bindings),
                std::mem::take(&mut registry.instances),
                std::mem::take(&mut registry.scoped),
                self.scopes.detach(self),
            )
        };
        drop(retired);
    }
}

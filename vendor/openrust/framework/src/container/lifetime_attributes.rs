//! Compiled Singleton and Scoped declarations.

use super::{Binding, Container, RegisteredBinding, Registry, ServiceKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifetime {
    Singleton,
    Scoped,
}

impl Registry {
    pub(super) fn declare_lifetime(
        &mut self,
        key: &ServiceKey,
        lifetime: Lifetime,
    ) -> Option<RegisteredBinding> {
        // Laravel chooses Singleton when both annotations are present.
        let active = self.lifetimes.entry(key.clone()).or_insert(lifetime);
        if lifetime == Lifetime::Singleton {
            *active = lifetime;
        }
        let entry = self.bindings.get_mut(key)?;
        (!entry.shared).then(|| {
            let replacement = entry.promoted();
            std::mem::replace(entry, replacement)
        })
    }

    pub(super) fn declared_recipe(
        &self,
        key: &ServiceKey,
        entry: RegisteredBinding,
    ) -> RegisteredBinding {
        if self.lifetimes.contains_key(key) && !entry.shared {
            entry.promoted()
        } else {
            entry
        }
    }

    pub(super) fn is_scoped(&self, key: &ServiceKey) -> bool {
        self.scoped.contains(key) || self.lifetimes.get(key) == Some(&Lifetime::Scoped)
    }

    pub(super) fn binding_is_bound(&self, container: &Container, key: &ServiceKey) -> bool {
        self.bindings.get(key).is_some_and(|entry| {
            !self.implicit_bindings.contains(key)
                || if self.is_scoped(key) && super::scope::active(container) {
                    super::scope::is_published(container, key, entry)
                } else {
                    entry.is_published()
                }
        })
    }
}

impl Container {
    /// Declares the cache lifetime of a compiled class or interface.
    #[allow(non_snake_case, reason = "Compiled Laravel lifetime declaration")]
    pub fn registerLifetime<T: ?Sized + Send + Sync + 'static>(&self, lifetime: Lifetime) {
        let retired = self
            .registry
            .write()
            .expect("container lock poisoned")
            .declare_lifetime(&ServiceKey::typed::<T>(), lifetime);
        self.has_lifetimes
            .store(true, std::sync::atomic::Ordering::Release);
        drop(retired);
    }

    pub(super) fn install_implicit_lifetime(&self, key: &ServiceKey) {
        {
            let registry = self.registry.read().expect("container lock poisoned");
            if registry.bindings.contains_key(key) || !registry.lifetimes.contains_key(key) {
                return;
            }
        }
        let mut registry = self.registry.write().expect("container lock poisoned");
        if registry.bindings.contains_key(key) || !registry.lifetimes.contains_key(key) {
            return;
        }
        if let Some(class) = registry.classes.get(key) {
            let recipe = class.shared_recipe();
            registry.insert_binding(key.clone(), recipe);
            registry.implicit_bindings.insert(key.clone());
        }
    }

    pub(super) fn promote_recipe<T: ?Sized + Send + Sync + 'static>(
        recipe: &RegisteredBinding,
    ) -> RegisteredBinding {
        use super::SingletonState;
        use std::sync::{Condvar, Mutex};

        let typed = recipe
            .value
            .downcast_ref::<Binding<T>>()
            .expect("typed binding recipe");
        let entry = match typed {
            Binding::Factory(factory) => {
                let async_factory = recipe.asynchronous_value.as_ref().and_then(|value| {
                    match value.downcast_ref::<Binding<T>>() {
                        Some(Binding::AsyncFactory(factory)) => Some(factory.clone()),
                        _ => None,
                    }
                });
                Binding::Singleton {
                    factory: factory.clone(),
                    async_factory,
                    state: Mutex::new(SingletonState::Empty),
                    ready: Condvar::new(),
                    changed: tokio::sync::Notify::new(),
                }
            }
            Binding::AsyncFactory(factory) => Binding::AsyncSingleton {
                factory: factory.clone(),
                state: Mutex::new(SingletonState::Empty),
                ready: Condvar::new(),
                changed: tokio::sync::Notify::new(),
            },
            _ => return recipe.clone(),
        };
        let mut entry = Self::entry(entry);
        entry.class_attributes = recipe.class_attributes.clone();
        entry
    }
}

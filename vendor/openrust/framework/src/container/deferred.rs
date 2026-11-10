//! Awaitable deferred loading; no executor blocking or background readiness shortcuts.
use std::{future::Future, pin::Pin};

use super::{Container, ServiceKey, scope};
use crate::contracts::container::{BindingResolutionError, Container as _};

pub(crate) type LoadFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), BindingResolutionError>> + Send + 'a>>;

pub(crate) trait DeferredServices: Send + Sync {
    fn contains(&self, key: &ServiceKey) -> bool;
    fn in_flight(&self, key: &ServiceKey) -> bool;

    /// In-flight providers must remain gated even after they publish an instance.
    fn needs_loading(&self, key: &ServiceKey, has_instance: bool) -> bool;
    fn load<'a>(&'a self, key: &'a ServiceKey) -> LoadFuture<'a>;
}

impl Container {
    pub(crate) fn install_deferred(&self, loader: std::sync::Arc<dyn DeferredServices>) {
        assert!(
            self.deferred.set(loader).is_ok(),
            "deferred provider registry already installed"
        );
    }

    pub(crate) fn provisional_cache_key(
        &self,
        key: &ServiceKey,
    ) -> Result<Option<ServiceKey>, BindingResolutionError> {
        let Some(loader) = self.deferred.get() else {
            return Ok(None);
        };
        let key = self.getAlias(key.clone())?;
        Ok(loader.in_flight(&key).then_some(key))
    }

    pub(crate) fn check_provisional_cache(
        &self,
        key: &ServiceKey,
    ) -> Result<(), BindingResolutionError> {
        if self
            .deferred
            .get()
            .is_some_and(|loader| loader.needs_loading(key, self.explicit_instance(key)))
        {
            return Err(BindingResolutionError::DeferredServiceRequiresAsync {
                service: key.clone(),
            });
        }
        Ok(())
    }

    fn explicit_instance(&self, key: &ServiceKey) -> bool {
        if scope::lookup(self, key).is_some_and(|entry| entry.instance) {
            return true;
        }
        let registry = self.registry.read().expect("container lock poisoned");
        registry.instances.contains_key(key) && registry.root_instance_visible(self, key)
    }

    pub(super) fn require_sync(
        &self,
        key: ServiceKey,
    ) -> Result<ServiceKey, BindingResolutionError> {
        let Some(loader) = self.deferred.get() else {
            return Ok(key);
        };
        let key = self.getAlias(key)?;
        if loader.needs_loading(&key, self.explicit_instance(&key)) {
            return Err(BindingResolutionError::DeferredServiceRequiresAsync { service: key });
        }
        Ok(key)
    }

    pub(super) async fn load_deferred(
        &self,
        key: ServiceKey,
    ) -> Result<ServiceKey, BindingResolutionError> {
        let Some(loader) = self.deferred.get() else {
            return Ok(key);
        };
        let key = self.getAlias(key)?;
        if loader.needs_loading(&key, self.explicit_instance(&key)) {
            loader.load(&key).await?;
        }
        Ok(key)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/deferred_resolution.rs"]
mod tests;

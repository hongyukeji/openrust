use super::{ProviderFactory, ServiceProvider};
use crate::{container::ServiceKey, contracts::support::DeferrableProvider};
use std::{any::TypeId, sync::Arc};

/// Adapts a compiled provider factory to the application's existing provider list.
pub struct DeferredProvider<P> {
    factory: Arc<dyn Fn() -> P + Send + Sync>,
}
impl<P: ServiceProvider + DeferrableProvider> DeferredProvider<P> {
    pub fn new(factory: impl Fn() -> P + Send + Sync + 'static) -> Self {
        Self {
            factory: Arc::new(factory),
        }
    }
}
impl<P: ServiceProvider + DeferrableProvider> ServiceProvider for DeferredProvider<P> {
    fn name(&self) -> &'static str {
        std::any::type_name::<P>()
    }
    fn is_deferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        <P as DeferrableProvider>::provides()
    }
    fn when(&self) -> Vec<&'static str> {
        <P as DeferrableProvider>::when()
    }
    fn provider_type(&self) -> TypeId {
        TypeId::of::<P>()
    }
    fn deferred_factory(&self) -> Option<Arc<ProviderFactory>> {
        let factory = self.factory.clone();
        Some(Arc::new(move || Box::new(factory())))
    }
}

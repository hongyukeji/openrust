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
    #[allow(non_snake_case, reason = "Matches Laravel provider metadata")]
    fn isDeferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        <P as DeferrableProvider>::provides()
    }
    #[allow(non_snake_case, reason = "Rust provider type identity adapter")]
    fn providerType(&self) -> TypeId {
        TypeId::of::<P>()
    }
    #[allow(non_snake_case, reason = "Rust compiled provider constructor adapter")]
    fn deferredFactory(&self) -> Option<Arc<ProviderFactory>> {
        let factory = self.factory.clone();
        Some(Arc::new(move || Box::new(factory())))
    }
}

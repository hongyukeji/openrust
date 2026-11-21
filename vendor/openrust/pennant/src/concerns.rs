use crate::{Feature, PendingScopedFeatureInteraction, Result, Scope};

/// Adds the native scoped feature entry point to Eloquent models.
pub trait HasFeatures: openrust::database::eloquent::Model {
    fn features(&self, store: Option<&str>) -> Result<PendingScopedFeatureInteraction> {
        Ok(Feature::store(store)?.r#for(Scope::fromModel(self)?))
    }
}
impl<M: openrust::database::eloquent::Model> HasFeatures for M {}

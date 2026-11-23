use crate::{
    FeatureManager, PendingScopedFeatureInteraction, Resolver, Result, Scope, drivers::Decorator,
};
use openrust::{container::ServiceKey, support::facades::Facade};
use serde_json::Value;
use std::sync::Arc;

pub struct Feature;
impl Facade for Feature {
    type Service = FeatureManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<FeatureManager>()
    }
}
impl Feature {
    pub fn manager() -> Result<Arc<FeatureManager>> {
        Self::get_facade_root().map_err(crate::Error::Binding)
    }
    pub fn store(name: Option<&str>) -> Result<Arc<Decorator>> {
        Self::manager()?.store(name)
    }
    pub fn r#for(scope: impl Into<Scope>) -> Result<PendingScopedFeatureInteraction> {
        Self::manager()?.r#for(scope)
    }
    pub async fn for_request(
        request: &mut openrust::http::Request,
    ) -> Result<PendingScopedFeatureInteraction> {
        Self::manager()?.for_request(request).await
    }
    pub fn globally() -> Result<PendingScopedFeatureInteraction> {
        Self::manager()?.globally()
    }
    pub fn define(feature: &str, resolver: Resolver) -> Result<()> {
        Self::store(None)?.define(feature, resolver)
    }
    pub fn define_value(feature: &str, value: Value) -> Result<()> {
        Self::store(None)?.define_value(feature, value)
    }
    pub fn define_class<F: crate::FeatureDefinition>(
        factory: impl Fn() -> crate::FeatureFuture<'static, Arc<F>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::store(None)?.define_class(factory)
    }
    pub fn name(feature: &str) -> Result<String> {
        Self::store(None)?.name(feature)
    }
    pub fn define_with_scope(
        feature: &str,
        resolver: Resolver,
        accepts_null_scope: bool,
    ) -> Result<()> {
        Self::store(None)?.define_with_scope(feature, resolver, accepts_null_scope)
    }
    pub fn define_lottery(feature: &str, lottery: openrust::support::Lottery) -> Result<()> {
        Self::store(None)?.define_lottery(feature, lottery)
    }
    pub async fn instance<F: crate::FeatureDefinition>() -> Result<Arc<F>> {
        Self::store(None)?.instance::<F>().await
    }
    pub fn name_map() -> Result<indexmap::IndexMap<String, crate::FeatureImplementation>> {
        Self::store(None)?.name_map()
    }
    pub fn get_driver() -> Result<Arc<dyn crate::Driver>> {
        Ok(Self::store(None)?.get_driver())
    }
    pub fn get_default_driver() -> Result<String> {
        Self::manager()?.get_default_driver()
    }
    pub fn set_default_driver(name: &str) -> Result<()> {
        Self::manager()?.set_default_driver(name)
    }
    pub fn forget_driver(name: Option<&str>) -> Result<()> {
        Self::manager()?.forget_driver(name)
    }
    pub fn forget_drivers() -> Result<()> {
        Self::manager()?.forget_drivers()
    }
    pub fn resolve_scope_using(
        resolver: impl Fn(&str) -> Result<Scope> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::manager()?.resolve_scope_using(resolver)
    }
    pub fn defined() -> Result<Vec<String>> {
        Self::store(None)?.defined()
    }
    pub fn flush_cache() -> Result<()> {
        Self::manager()?.flush_cache()
    }
    pub fn serialize_scope(scope: &Scope) -> Result<String> {
        Ok(Self::manager()?.serialize_scope(scope))
    }
    pub fn use_morph_map(value: bool) -> Result<()> {
        Self::manager()?.use_morph_map(value);
        Ok(())
    }
    pub async fn load(features: &[&str]) -> Result<crate::FeatureValues> {
        Self::manager()?.interaction()?.load(features).await
    }
    pub async fn load_missing(features: &[&str]) -> Result<crate::FeatureValues> {
        Self::manager()?.interaction()?.load_missing(features).await
    }
    pub async fn load_all() -> Result<crate::FeatureValues> {
        Self::manager()?.interaction()?.load_all().await
    }
    pub async fn stored() -> Result<Vec<String>> {
        Self::store(None)?.stored().await
    }
    pub async fn forget(features: &[&str]) -> Result<()> {
        Self::manager()?.interaction()?.forget(features).await
    }
    pub async fn activate_with_value(features: &[&str], value: Value) -> Result<()> {
        Self::manager()?
            .interaction()?
            .activate_with_value(features, value)
            .await
    }
    pub async fn when<T>(
        feature: &str,
        when_active: impl FnOnce(Value) -> T,
        when_inactive: impl FnOnce() -> T,
    ) -> Result<T> {
        Self::manager()?
            .interaction()?
            .when(feature, when_active, when_inactive)
            .await
    }
    pub async fn unless<T>(
        feature: &str,
        when_inactive: impl FnOnce() -> T,
        when_active: impl FnOnce(Value) -> T,
    ) -> Result<T> {
        Self::manager()?
            .interaction()?
            .unless(feature, when_inactive, when_active)
            .await
    }
    pub async fn value(feature: &str) -> Result<Value> {
        Self::manager()?.interaction()?.value(feature).await
    }
    pub async fn values(features: &[&str]) -> Result<indexmap::IndexMap<String, Value>> {
        Self::manager()?.interaction()?.values(features).await
    }
    pub async fn all() -> Result<indexmap::IndexMap<String, Value>> {
        Self::manager()?.interaction()?.all().await
    }
    pub async fn active(feature: &str) -> Result<bool> {
        Self::manager()?.interaction()?.active(feature).await
    }
    pub async fn inactive(feature: &str) -> Result<bool> {
        Self::manager()?.interaction()?.inactive(feature).await
    }
    pub async fn all_are_active(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .all_are_active(features)
            .await
    }
    pub async fn some_are_active(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .some_are_active(features)
            .await
    }
    pub async fn all_are_inactive(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .all_are_inactive(features)
            .await
    }
    pub async fn some_are_inactive(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .some_are_inactive(features)
            .await
    }
    pub async fn activate(feature: &str) -> Result<()> {
        Self::manager()?.interaction()?.activate(feature).await
    }
    pub async fn deactivate(feature: &str) -> Result<()> {
        Self::manager()?.interaction()?.deactivate(feature).await
    }
    pub async fn activate_for_everyone(feature: &str, value: Value) -> Result<()> {
        Self::store(None)?
            .activate_for_everyone(feature, value)
            .await
    }
    pub async fn deactivate_for_everyone(feature: &str) -> Result<()> {
        Self::store(None)?.deactivate_for_everyone(feature).await
    }
    pub async fn purge(features: Option<&[String]>) -> Result<()> {
        Self::store(None)?.purge(features).await
    }
}

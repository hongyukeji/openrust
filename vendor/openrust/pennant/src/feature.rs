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
    pub fn defineValue(feature: &str, value: Value) -> Result<()> {
        Self::store(None)?.defineValue(feature, value)
    }
    pub fn defineClass<F: crate::FeatureDefinition>(
        factory: impl Fn() -> crate::FeatureFuture<'static, Arc<F>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::store(None)?.defineClass(factory)
    }
    pub fn name(feature: &str) -> Result<String> {
        Self::store(None)?.name(feature)
    }
    pub fn defineWithScope(
        feature: &str,
        resolver: Resolver,
        acceptsNullScope: bool,
    ) -> Result<()> {
        Self::store(None)?.defineWithScope(feature, resolver, acceptsNullScope)
    }
    pub fn defineLottery(feature: &str, lottery: openrust::support::Lottery) -> Result<()> {
        Self::store(None)?.defineLottery(feature, lottery)
    }
    pub async fn instance<F: crate::FeatureDefinition>() -> Result<Arc<F>> {
        Self::store(None)?.instance::<F>().await
    }
    pub fn nameMap() -> Result<indexmap::IndexMap<String, crate::FeatureImplementation>> {
        Self::store(None)?.nameMap()
    }
    pub fn getDriver() -> Result<Arc<dyn crate::Driver>> {
        Ok(Self::store(None)?.getDriver())
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
    pub fn resolveScopeUsing(
        resolver: impl Fn(&str) -> Result<Scope> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::manager()?.resolveScopeUsing(resolver)
    }
    pub fn defined() -> Result<Vec<String>> {
        Self::store(None)?.defined()
    }
    pub fn flushCache() -> Result<()> {
        Self::manager()?.flushCache()
    }
    pub fn serializeScope(scope: &Scope) -> Result<String> {
        Ok(Self::manager()?.serializeScope(scope))
    }
    pub fn useMorphMap(value: bool) -> Result<()> {
        Self::manager()?.useMorphMap(value);
        Ok(())
    }
    pub async fn load(features: &[&str]) -> Result<crate::FeatureValues> {
        Self::manager()?.interaction()?.load(features).await
    }
    pub async fn load_missing(features: &[&str]) -> Result<crate::FeatureValues> {
        Self::manager()?.interaction()?.load_missing(features).await
    }
    pub async fn loadAll() -> Result<crate::FeatureValues> {
        Self::manager()?.interaction()?.loadAll().await
    }
    pub async fn stored() -> Result<Vec<String>> {
        Self::store(None)?.stored().await
    }
    pub async fn forget(features: &[&str]) -> Result<()> {
        Self::manager()?.interaction()?.forget(features).await
    }
    pub async fn activateWithValue(features: &[&str], value: Value) -> Result<()> {
        Self::manager()?
            .interaction()?
            .activateWithValue(features, value)
            .await
    }
    pub async fn when<T>(
        feature: &str,
        whenActive: impl FnOnce(Value) -> T,
        whenInactive: impl FnOnce() -> T,
    ) -> Result<T> {
        Self::manager()?
            .interaction()?
            .when(feature, whenActive, whenInactive)
            .await
    }
    pub async fn unless<T>(
        feature: &str,
        whenInactive: impl FnOnce() -> T,
        whenActive: impl FnOnce(Value) -> T,
    ) -> Result<T> {
        Self::manager()?
            .interaction()?
            .unless(feature, whenInactive, whenActive)
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
    pub async fn allAreActive(features: &[&str]) -> Result<bool> {
        Self::manager()?.interaction()?.allAreActive(features).await
    }
    pub async fn someAreActive(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .someAreActive(features)
            .await
    }
    pub async fn allAreInactive(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .allAreInactive(features)
            .await
    }
    pub async fn someAreInactive(features: &[&str]) -> Result<bool> {
        Self::manager()?
            .interaction()?
            .someAreInactive(features)
            .await
    }
    pub async fn activate(feature: &str) -> Result<()> {
        Self::manager()?.interaction()?.activate(feature).await
    }
    pub async fn deactivate(feature: &str) -> Result<()> {
        Self::manager()?.interaction()?.deactivate(feature).await
    }
    pub async fn activateForEveryone(feature: &str, value: Value) -> Result<()> {
        Self::store(None)?.activateForEveryone(feature, value).await
    }
    pub async fn deactivateForEveryone(feature: &str) -> Result<()> {
        Self::store(None)?.deactivateForEveryone(feature).await
    }
    pub async fn purge(features: Option<&[String]>) -> Result<()> {
        Self::store(None)?.purge(features).await
    }
}

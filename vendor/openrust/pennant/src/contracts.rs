use crate::{Result, Scope};
use serde_json::Value;
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

pub type FeatureFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type Resolver = Arc<dyn Fn(Scope) -> FeatureFuture<'static, Value> + Send + Sync>;
pub(crate) type Definitions = Arc<std::sync::RwLock<indexmap::IndexMap<String, Resolver>>>;
pub type FeatureScopes = indexmap::IndexMap<String, Vec<Scope>>;
pub type FeatureValues = indexmap::IndexMap<String, Vec<Value>>;

#[derive(Clone)]
pub struct FeatureValue {
    pub feature: String,
    pub scope: Scope,
    pub value: Value,
}

/// The pinned Pennant driver contract, expressed as owned async operations.
pub trait Driver: Send + Sync {
    fn define(&self, feature: &str, resolver: Resolver) -> Result<()>;
    fn defined(&self) -> Result<Vec<String>>;
    fn stored(&self) -> FeatureFuture<'_, Vec<String>> {
        Box::pin(async { Err(crate::Error::Unsupported("listing stored features")) })
    }
    fn definedFeaturesForScope(&self, _scope: &Scope) -> Result<Option<Vec<String>>> {
        Ok(None)
    }
    fn supportsSetAll(&self) -> bool {
        false
    }
    fn get<'a>(&'a self, feature: &'a str, scope: &'a Scope) -> FeatureFuture<'a, Value>;
    fn getAll<'a>(&'a self, features: &'a FeatureScopes) -> FeatureFuture<'a, FeatureValues>;
    fn set<'a>(&'a self, feature: &'a str, scope: &'a Scope, value: Value)
    -> FeatureFuture<'a, ()>;
    fn setAll<'a>(&'a self, features: &'a [FeatureValue]) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            for feature in features {
                self.set(&feature.feature, &feature.scope, feature.value.clone())
                    .await?;
            }
            Ok(())
        })
    }
    fn setForAllScopes<'a>(&'a self, feature: &'a str, value: Value) -> FeatureFuture<'a, ()>;
    fn delete<'a>(&'a self, feature: &'a str, scope: &'a Scope) -> FeatureFuture<'a, ()>;
    fn purge<'a>(&'a self, features: Option<&'a [String]>) -> FeatureFuture<'a, ()>;
    fn flushCache(&self) -> Result<()> {
        Ok(())
    }
}

/// A compiled feature class. The factory passed to `defineClass` may resolve dependencies
/// from the application's container; it is invoked only when a value or hook is needed.
pub trait FeatureDefinition: Send + Sync + 'static {
    fn name() -> &'static str {
        std::any::type_name::<Self>()
    }
    fn acceptsNullScope() -> bool {
        true
    }
    fn isValidForScope(scope: &Scope) -> bool {
        Self::acceptsNullScope() || scope != &Scope::Null
    }
    fn beforeAcceptsNullScope() -> bool {
        true
    }
    fn before<'a>(&'a self, _scope: &'a Scope) -> FeatureFuture<'a, Option<Value>> {
        Box::pin(async { Ok(None) })
    }
    fn resolve<'a>(&'a self, scope: &'a Scope) -> FeatureFuture<'a, Value>;
}
pub(crate) type BeforeHook =
    Arc<dyn Fn(Scope) -> FeatureFuture<'static, Option<Value>> + Send + Sync>;
#[derive(Default)]
pub(crate) struct FeatureClasses {
    pub implementations: std::sync::RwLock<indexmap::IndexMap<String, FeatureImplementation>>,
    pub names: std::sync::RwLock<BTreeMap<String, String>>,
    pub before: std::sync::RwLock<BTreeMap<String, BeforeHook>>,
    pub factories: std::sync::RwLock<BTreeMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
    pub valid: std::sync::RwLock<BTreeMap<String, ScopeValidator>>,
}

/// Rust replacement for PHP's runtime `FeatureScopeable` conversion.
pub trait FeatureScopeable {
    fn toFeatureIdentifier(&self, driver: &str) -> Scope;
}

pub(crate) type ScopeValidator = Arc<dyn Fn(&Scope) -> bool + Send + Sync>;

pub(crate) struct ClassFactory<F: FeatureDefinition>(
    pub Arc<dyn Fn() -> FeatureFuture<'static, Arc<F>> + Send + Sync>,
);

/// Rust representation of Pennant's heterogeneous feature implementation map.
#[derive(Clone)]
pub enum FeatureImplementation {
    Resolver(Resolver),
    Value(Value),
    Lottery(openrust::support::Lottery),
    Class(&'static str),
}

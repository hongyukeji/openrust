use crate::{
    Driver, Error, FeatureFuture, FeatureScopes, FeatureValue, FeatureValues, Resolver, Result,
    Scope,
};
use serde_json::Value;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub struct ArrayDriver {
    pub(crate) resolvers: crate::contracts::Definitions,
    pub(crate) use_morph_map: Arc<AtomicBool>,
    pub(crate) events: Option<Arc<openrust::events::Dispatcher>>,
    states: RwLock<indexmap::IndexMap<String, indexmap::IndexMap<String, Value>>>,
}
impl ArrayDriver {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Driver for ArrayDriver {
    fn supports_set_all(&self) -> bool {
        true
    }
    fn define(&self, feature: &str, resolver: Resolver) -> Result<()> {
        self.resolvers
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(feature.into(), resolver);
        Ok(())
    }
    fn defined(&self) -> Result<Vec<String>> {
        Ok(self
            .resolvers
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .keys()
            .cloned()
            .collect())
    }
    fn stored(&self) -> FeatureFuture<'_, Vec<String>> {
        Box::pin(async move {
            Ok(self
                .states
                .read()
                .map_err(|_| Error::StateUnavailable)?
                .keys()
                .cloned()
                .collect())
        })
    }
    fn get<'a>(&'a self, feature: &'a str, scope: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async move {
            if let Some(value) = self
                .states
                .read()
                .map_err(|_| Error::StateUnavailable)?
                .get(feature)
                .and_then(|scopes| {
                    scopes.get(&scope.serialize(self.use_morph_map.load(Ordering::Relaxed)))
                })
                .filter(|value| !value.is_null())
                .cloned()
            {
                return Ok(value);
            }
            let resolver = self
                .resolvers
                .read()
                .map_err(|_| Error::StateUnavailable)?
                .get(feature)
                .cloned();
            let Some(resolver) = resolver else {
                crate::events::dispatch(
                    self.events.as_ref(),
                    crate::events::UnknownFeatureResolved {
                        feature: feature.into(),
                        scope: scope.clone(),
                    },
                )
                .await?;
                return Ok(Value::Bool(false));
            };
            let value = resolver(scope.clone()).await?;
            self.set(feature, scope, value.clone()).await?;
            Ok(value)
        })
    }
    fn get_all<'a>(&'a self, features: &'a FeatureScopes) -> FeatureFuture<'a, FeatureValues> {
        Box::pin(async move {
            let mut values = FeatureValues::new();
            for (feature, scopes) in features {
                let mut resolved = Vec::new();
                for scope in scopes {
                    resolved.push(self.get(feature, scope).await?);
                }
                values.insert(feature.clone(), resolved);
            }
            Ok(values)
        })
    }
    fn set<'a>(
        &'a self,
        feature: &'a str,
        scope: &'a Scope,
        value: Value,
    ) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            self.states
                .write()
                .map_err(|_| Error::StateUnavailable)?
                .entry(feature.into())
                .or_default()
                .insert(
                    scope.serialize(self.use_morph_map.load(Ordering::Relaxed)),
                    value,
                );
            Ok(())
        })
    }
    fn set_all<'a>(&'a self, features: &'a [FeatureValue]) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            let mut states = self.states.write().map_err(|_| Error::StateUnavailable)?;
            for feature in features {
                states.entry(feature.feature.clone()).or_default().insert(
                    feature
                        .scope
                        .serialize(self.use_morph_map.load(Ordering::Relaxed)),
                    feature.value.clone(),
                );
            }
            Ok(())
        })
    }
    fn set_for_all_scopes<'a>(&'a self, feature: &'a str, value: Value) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            let mut states = self.states.write().map_err(|_| Error::StateUnavailable)?;
            for stored in states.entry(feature.into()).or_default().values_mut() {
                *stored = value.clone();
            }
            Ok(())
        })
    }
    fn delete<'a>(&'a self, feature: &'a str, scope: &'a Scope) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            if let Some(scopes) = self
                .states
                .write()
                .map_err(|_| Error::StateUnavailable)?
                .get_mut(feature)
            {
                scopes.shift_remove(&scope.serialize(self.use_morph_map.load(Ordering::Relaxed)));
            }
            Ok(())
        })
    }
    fn purge<'a>(&'a self, features: Option<&'a [String]>) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            let mut states = self.states.write().map_err(|_| Error::StateUnavailable)?;
            match features {
                None => states.clear(),
                Some(features) => {
                    for feature in features {
                        states.shift_remove(feature);
                    }
                }
            }
            Ok(())
        })
    }
    fn flush_cache(&self) -> Result<()> {
        self.states
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .clear();
        Ok(())
    }
}

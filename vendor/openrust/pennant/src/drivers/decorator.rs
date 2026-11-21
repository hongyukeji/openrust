use crate::{
    Driver, Error, FeatureScopes, FeatureValue, FeatureValues, PendingScopedFeatureInteraction,
    Resolver, Result, Scope,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Decorator {
    name: String,
    pub(crate) useMorphMap: Arc<AtomicBool>,
    pub(crate) events: Option<Arc<openrust::events::Dispatcher>>,
    driver: Arc<dyn Driver>,
    pub(crate) classes: Arc<crate::contracts::FeatureClasses>,
    cache: RwLock<BTreeMap<(String, String), Value>>,
}
impl Decorator {
    pub fn new(name: impl Into<String>, driver: Arc<dyn Driver>) -> Self {
        Self {
            name: name.into(),
            useMorphMap: Arc::new(AtomicBool::new(false)),
            events: None,
            driver,
            classes: Arc::default(),
            cache: RwLock::new(BTreeMap::new()),
        }
    }
    pub fn getDriver(&self) -> Arc<dyn Driver> {
        self.driver.clone()
    }
    pub fn storeName(&self) -> &str {
        &self.name
    }
    pub fn define(&self, feature: &str, resolver: Resolver) -> Result<()> {
        self.classes
            .factories
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(feature);
        self.classes
            .before
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(feature);
        self.defineResolver(feature, resolver, true)
    }
    /// Explicit nullability replaces PHP parameter reflection.
    pub fn defineWithScope(
        &self,
        feature: &str,
        resolver: Resolver,
        acceptsNullScope: bool,
    ) -> Result<()> {
        self.classes
            .factories
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(feature);
        self.classes
            .before
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(feature);
        self.defineResolver(feature, resolver, acceptsNullScope)
    }
    fn defineResolver(
        &self,
        feature: &str,
        resolver: Resolver,
        acceptsNullScope: bool,
    ) -> Result<()> {
        self.classes
            .valid
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                feature.into(),
                Arc::new(move |scope| acceptsNullScope || scope != &Scope::Null),
            );
        self.classes
            .implementations
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                feature.into(),
                crate::FeatureImplementation::Resolver(resolver.clone()),
            );
        let name = feature.to_owned();
        let events = self.events.clone();
        self.driver.define(
            feature,
            Arc::new(move |scope| {
                let resolver = resolver.clone();
                let feature = name.clone();
                let events = events.clone();
                Box::pin(async move {
                    let value = if scope == Scope::Null && !acceptsNullScope {
                        crate::events::dispatch(
                            events.as_ref(),
                            crate::events::UnexpectedNullScopeEncountered {
                                feature: feature.clone(),
                            },
                        )
                        .await?;
                        Value::Bool(false)
                    } else {
                        resolver(scope.clone()).await?
                    };
                    crate::events::dispatch(
                        events.as_ref(),
                        crate::events::FeatureResolved {
                            feature,
                            scope,
                            value: value.clone(),
                        },
                    )
                    .await?;
                    Ok(value)
                })
            }),
        )
    }
    pub fn defineClass<F: crate::FeatureDefinition>(
        &self,
        factory: impl Fn() -> crate::FeatureFuture<'static, Arc<F>> + Send + Sync + 'static,
    ) -> Result<()> {
        let factory: Arc<dyn Fn() -> crate::FeatureFuture<'static, Arc<F>> + Send + Sync> =
            Arc::new(factory);
        self.classes
            .factories
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                F::name().into(),
                Arc::new(crate::contracts::ClassFactory(factory.clone())),
            );
        let resolverFactory = factory.clone();
        self.defineResolver(
            F::name(),
            Arc::new(move |scope| {
                let factory = resolverFactory.clone();
                Box::pin(async move { factory().await?.resolve(&scope).await })
            }),
            F::acceptsNullScope(),
        )?;
        let events = self.events.clone();
        self.classes
            .before
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                F::name().into(),
                Arc::new(move |scope| {
                    let factory = factory.clone();
                    let events = events.clone();
                    Box::pin(async move {
                        if scope == Scope::Null && !F::beforeAcceptsNullScope() {
                            crate::events::dispatch(
                                events.as_ref(),
                                crate::events::UnexpectedNullScopeEncountered {
                                    feature: F::name().into(),
                                },
                            )
                            .await?;
                            return Ok(None);
                        }
                        factory().await?.before(&scope).await
                    })
                }),
            );
        self.classes
            .implementations
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                F::name().into(),
                crate::FeatureImplementation::Class(std::any::type_name::<F>()),
            );
        self.classes
            .valid
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(F::name().into(), Arc::new(F::isValidForScope));
        self.classes
            .names
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(std::any::type_name::<F>().into(), F::name().into());
        Ok(())
    }
    /// Returns the typed class instance so application-specific methods remain available.
    pub async fn instance<F: crate::FeatureDefinition>(&self) -> Result<Arc<F>> {
        let factory = self
            .classes
            .factories
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(F::name())
            .cloned()
            .ok_or(Error::InvalidValue)?;
        let factory = factory
            .downcast::<crate::contracts::ClassFactory<F>>()
            .map_err(|_| Error::InvalidValue)?;
        (factory.0)().await
    }
    pub fn name(&self, feature: &str) -> Result<String> {
        Ok(self
            .classes
            .names
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(feature)
            .cloned()
            .unwrap_or_else(|| feature.into()))
    }
    pub fn nameMap(&self) -> Result<indexmap::IndexMap<String, crate::FeatureImplementation>> {
        Ok(self
            .classes
            .implementations
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .clone())
    }
    pub fn implementation(&self, feature: &str) -> Result<crate::FeatureImplementation> {
        self.nameMap()?
            .shift_remove(&self.name(feature)?)
            .ok_or(Error::InvalidValue)
    }
    pub fn forScopeable(
        self: &Arc<Self>,
        scope: &impl crate::FeatureScopeable,
    ) -> PendingScopedFeatureInteraction {
        self.r#for(scope.toFeatureIdentifier(&self.name))
    }
    async fn before(&self, feature: &str, scope: &Scope) -> Result<Option<Value>> {
        let hook = self
            .classes
            .before
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(feature)
            .cloned();
        match hook {
            Some(hook) => hook(scope.clone()).await,
            None => Ok(None),
        }
    }
    /// Compiled registration routines replace runtime PHP source discovery.
    pub fn discover(
        &self,
        registrations: impl IntoIterator<Item = fn(&Decorator) -> Result<()>>,
    ) -> Result<()> {
        for register in registrations {
            register(self)?;
        }
        Ok(())
    }
    pub fn isResolverValidForScope(&self, feature: &str, scope: &Scope) -> Result<bool> {
        let feature = self.name(feature)?;
        let valid = self
            .classes
            .valid
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(&feature)
            .cloned();
        Ok(valid.is_none_or(|valid| valid(scope)))
    }
    pub fn definedFeaturesForScope(&self, scope: &Scope) -> Result<Vec<String>> {
        if let Some(features) = self.driver.definedFeaturesForScope(scope)? {
            return Ok(features);
        }
        self.defined()?
            .into_iter()
            .filter_map(|name| match self.isResolverValidForScope(&name, scope) {
                Ok(true) => Some(Ok(name)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }
    pub fn defineLottery(&self, feature: &str, lottery: openrust::support::Lottery) -> Result<()> {
        self.define(
            feature,
            Arc::new(move |_| Box::pin(async move { Ok(Value::Bool(lottery.choose())) })),
        )?;
        self.classes
            .implementations
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                feature.into(),
                crate::FeatureImplementation::Lottery(lottery),
            );
        Ok(())
    }
    pub fn defineValue(&self, feature: &str, value: Value) -> Result<()> {
        let literal = value.clone();
        self.define(
            feature,
            Arc::new(move |_| {
                let value = value.clone();
                Box::pin(async move { Ok(value) })
            }),
        )?;
        self.classes
            .implementations
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(feature.into(), crate::FeatureImplementation::Value(literal));
        Ok(())
    }
    pub fn defined(&self) -> Result<Vec<String>> {
        self.driver.defined()
    }
    pub async fn stored(&self) -> Result<Vec<String>> {
        self.driver.stored().await
    }
    pub fn r#for(self: &Arc<Self>, scope: impl Into<Scope>) -> PendingScopedFeatureInteraction {
        PendingScopedFeatureInteraction::new(self.clone()).r#for(scope)
    }
    pub fn globally(self: &Arc<Self>) -> PendingScopedFeatureInteraction {
        self.r#for(Scope::Global)
    }
    pub async fn get(&self, feature: &str, scope: &Scope) -> Result<Value> {
        let featureName = self.name(feature)?;
        let feature = featureName.as_str();
        let key = (
            feature.into(),
            scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
        );
        let cached = self
            .cache
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(&key)
            .cloned();
        let value = if let Some(value) = cached {
            value
        } else {
            let value = match self.before(feature, scope).await? {
                Some(value) => value,
                None => self.driver.get(feature, scope).await?,
            };
            self.cache
                .write()
                .map_err(|_| Error::StateUnavailable)?
                .insert(key, value.clone());
            value
        };
        crate::events::dispatch(
            self.events.as_ref(),
            crate::events::FeatureRetrieved {
                feature: feature.into(),
                scope: scope.clone(),
                value: value.clone(),
            },
        )
        .await?;
        Ok(value)
    }
    pub async fn getAll(&self, features: &FeatureScopes) -> Result<FeatureValues> {
        let normalized = features
            .iter()
            .map(|(feature, scopes)| Ok((self.name(feature)?, scopes.clone())))
            .collect::<Result<FeatureScopes>>()?;
        let features = &normalized;
        let mut values = FeatureValues::new();
        let mut missing = FeatureScopes::new();
        let mut indexes = BTreeMap::<String, Vec<usize>>::new();
        for (feature, scopes) in features {
            let featureName = self.name(feature)?;
            let mut resolved = Vec::with_capacity(scopes.len());
            for (index, scope) in scopes.iter().enumerate() {
                if let Some(value) = self.before(&featureName, scope).await? {
                    resolved.push(value);
                } else {
                    missing
                        .entry(featureName.clone())
                        .or_default()
                        .push(scope.clone());
                    indexes.entry(featureName.clone()).or_default().push(index);
                    resolved.push(Value::Null);
                }
            }
            values.insert(featureName, resolved);
        }
        if !missing.is_empty() {
            let loaded = self.driver.getAll(&missing).await?;
            for (feature, indexes) in indexes {
                for (index, value) in indexes
                    .into_iter()
                    .zip(loaded.get(&feature).ok_or(Error::InvalidValue)?)
                {
                    values.get_mut(&feature).ok_or(Error::InvalidValue)?[index] = value.clone();
                }
            }
        }
        let mut cache = self.cache.write().map_err(|_| Error::StateUnavailable)?;
        for (feature, scopes) in features {
            for (scope, value) in scopes
                .iter()
                .zip(values.get(feature).ok_or(Error::InvalidValue)?)
            {
                cache.insert(
                    (
                        feature.clone(),
                        scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
                    ),
                    value.clone(),
                );
            }
        }
        Ok(values)
    }
    pub async fn getAllMissing(&self, features: &FeatureScopes) -> Result<FeatureValues> {
        let normalized = features
            .iter()
            .map(|(feature, scopes)| Ok((self.name(feature)?, scopes.clone())))
            .collect::<Result<FeatureScopes>>()?;
        let features = &normalized;
        let missing: FeatureScopes = {
            let cache = self.cache.read().map_err(|_| Error::StateUnavailable)?;
            features
                .iter()
                .filter_map(|(feature, scopes)| {
                    let scopes: Vec<_> = scopes
                        .iter()
                        .filter(|scope| {
                            !cache.contains_key(&(
                                feature.clone(),
                                scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
                            ))
                        })
                        .cloned()
                        .collect();
                    (!scopes.is_empty()).then(|| (feature.clone(), scopes))
                })
                .collect()
        };
        self.getAll(&missing).await
    }

    pub async fn set(&self, feature: &str, scope: &Scope, value: Value) -> Result<()> {
        let featureName = self.name(feature)?;
        let feature = featureName.as_str();
        self.driver.set(feature, scope, value.clone()).await?;
        self.cache
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(
                (
                    feature.into(),
                    scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
                ),
                value.clone(),
            );
        crate::events::dispatch(
            self.events.as_ref(),
            crate::events::FeatureUpdated {
                feature: feature.into(),
                scope: scope.clone(),
                value,
            },
        )
        .await?;
        Ok(())
    }
    pub async fn setAll(&self, features: &[FeatureValue]) -> Result<()> {
        let normalized = features
            .iter()
            .map(|value| {
                Ok(FeatureValue {
                    feature: self.name(&value.feature)?,
                    scope: value.scope.clone(),
                    value: value.value.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let features = normalized.as_slice();
        if !self.driver.supportsSetAll() {
            for feature in features {
                self.set(&feature.feature, &feature.scope, feature.value.clone())
                    .await?;
            }
            return Ok(());
        }
        self.driver.setAll(features).await?;
        {
            let mut cache = self.cache.write().map_err(|_| Error::StateUnavailable)?;
            for feature in features {
                cache.insert(
                    (
                        feature.feature.clone(),
                        feature
                            .scope
                            .serialize(self.useMorphMap.load(Ordering::Relaxed)),
                    ),
                    feature.value.clone(),
                );
            }
        }
        for feature in features {
            crate::events::dispatch(
                self.events.as_ref(),
                crate::events::FeatureUpdated {
                    feature: feature.feature.clone(),
                    scope: feature.scope.clone(),
                    value: feature.value.clone(),
                },
            )
            .await?;
        }
        Ok(())
    }
    pub async fn setForAllScopes(&self, feature: &str, value: Value) -> Result<()> {
        let featureName = self.name(feature)?;
        let feature = featureName.as_str();
        self.driver.setForAllScopes(feature, value.clone()).await?;
        self.cache
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .retain(|(name, _), _| name != feature);
        crate::events::dispatch(
            self.events.as_ref(),
            crate::events::FeatureUpdatedForAllScopes {
                feature: feature.into(),
                value,
            },
        )
        .await?;
        Ok(())
    }
    pub async fn activateForEveryone(&self, feature: &str, value: Value) -> Result<()> {
        self.setForAllScopes(feature, value).await
    }
    pub async fn deactivateForEveryone(&self, feature: &str) -> Result<()> {
        self.setForAllScopes(feature, Value::Bool(false)).await
    }
    pub async fn delete(&self, feature: &str, scope: &Scope) -> Result<()> {
        let featureName = self.name(feature)?;
        let feature = featureName.as_str();
        self.driver.delete(feature, scope).await?;
        self.cache
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(&(
                feature.into(),
                scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
            ));
        crate::events::dispatch(
            self.events.as_ref(),
            crate::events::FeatureDeleted {
                feature: feature.into(),
                scope: scope.clone(),
            },
        )
        .await?;
        Ok(())
    }
    pub async fn purge(&self, features: Option<&[String]>) -> Result<()> {
        let normalized = features
            .map(|features| {
                features
                    .iter()
                    .map(|name| self.name(name))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let features = normalized.as_deref();
        self.driver.purge(features).await?;
        self.cache
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .retain(|(name, _), _| features.is_some_and(|features| !features.contains(name)));
        match features {
            None => {
                crate::events::dispatch(self.events.as_ref(), crate::events::AllFeaturesPurged {})
                    .await?
            }
            Some(features) => {
                crate::events::dispatch(
                    self.events.as_ref(),
                    crate::events::FeaturesPurged {
                        features: features.to_vec(),
                    },
                )
                .await?
            }
        }
        Ok(())
    }
    pub fn flushCache(&self) -> Result<()> {
        self.cache
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .clear();
        self.driver.flushCache()
    }
}

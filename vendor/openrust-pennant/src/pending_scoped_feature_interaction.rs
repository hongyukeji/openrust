use crate::{Error, FeatureValue, FeatureValues, Result, Scope, drivers::Decorator};
use indexmap::IndexMap;
use serde_json::Value;
use std::sync::Arc;

pub struct PendingScopedFeatureInteraction {
    driver: Arc<Decorator>,
    scopes: Vec<Scope>,
}
impl PendingScopedFeatureInteraction {
    pub fn new(driver: Arc<Decorator>) -> Self {
        Self {
            driver,
            scopes: vec![],
        }
    }
    pub fn r#for(mut self, scope: impl Into<Scope>) -> Self {
        self.scopes.push(scope.into());
        self
    }
    pub fn for_scopes(mut self, scopes: impl IntoIterator<Item = Scope>) -> Self {
        self.scopes.extend(scopes);
        self
    }
    fn scopes(&self) -> Vec<Scope> {
        if self.scopes.is_empty() {
            vec![Scope::Null]
        } else {
            self.scopes.clone()
        }
    }
    pub async fn load(&self, features: &[&str]) -> Result<FeatureValues> {
        self.driver
            .get_all(
                &features
                    .iter()
                    .map(|name| ((*name).into(), self.scopes()))
                    .collect(),
            )
            .await
    }
    pub async fn load_missing(&self, features: &[&str]) -> Result<FeatureValues> {
        self.driver
            .get_all_missing(
                &features
                    .iter()
                    .map(|name| ((*name).into(), self.scopes()))
                    .collect(),
            )
            .await
    }
    pub async fn load_all(&self) -> Result<FeatureValues> {
        let features = self.driver.defined_features_for_scope(&self.scopes()[0])?;
        self.load(&features.iter().map(String::as_str).collect::<Vec<_>>())
            .await
    }
    pub async fn values(&self, features: &[&str]) -> Result<IndexMap<String, Value>> {
        if self.scopes().len() > 1 {
            return Err(Error::MultipleScopes);
        }
        self.load_missing(features).await?;
        let scope = &self.scopes()[0];
        let mut values = IndexMap::new();
        for feature in features {
            values.insert((*feature).into(), self.driver.get(feature, scope).await?);
        }
        Ok(values)
    }
    pub async fn value(&self, feature: &str) -> Result<Value> {
        self.values(&[feature])
            .await?
            .shift_remove(feature)
            .ok_or(Error::InvalidValue)
    }
    pub async fn all(&self) -> Result<IndexMap<String, Value>> {
        let features = self.driver.defined_features_for_scope(&self.scopes()[0])?;
        self.values(&features.iter().map(String::as_str).collect::<Vec<_>>())
            .await
    }
    pub async fn active(&self, feature: &str) -> Result<bool> {
        self.all_are_active(&[feature]).await
    }
    pub async fn inactive(&self, feature: &str) -> Result<bool> {
        self.all_are_inactive(&[feature]).await
    }
    pub async fn all_are_active(&self, features: &[&str]) -> Result<bool> {
        self.load_missing(features).await?;
        for feature in features {
            for scope in self.scopes() {
                if self.driver.get(feature, &scope).await? == Value::Bool(false) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    pub async fn all_are_inactive(&self, features: &[&str]) -> Result<bool> {
        self.load_missing(features).await?;
        for feature in features {
            for scope in self.scopes() {
                if self.driver.get(feature, &scope).await? != Value::Bool(false) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    pub async fn some_are_active(&self, features: &[&str]) -> Result<bool> {
        self.some(features, true).await
    }
    pub async fn some_are_inactive(&self, features: &[&str]) -> Result<bool> {
        self.some(features, false).await
    }
    async fn some(&self, features: &[&str], active: bool) -> Result<bool> {
        self.load_missing(features).await?;
        for scope in self.scopes() {
            let mut found = false;
            for feature in features {
                if (self.driver.get(feature, &scope).await? != Value::Bool(false)) == active {
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub async fn when<T>(
        &self,
        feature: &str,
        when_active: impl FnOnce(Value) -> T,
        when_inactive: impl FnOnce() -> T,
    ) -> Result<T> {
        if self.active(feature).await? {
            Ok(when_active(self.value(feature).await?))
        } else {
            Ok(when_inactive())
        }
    }
    pub async fn unless<T>(
        &self,
        feature: &str,
        when_inactive: impl FnOnce() -> T,
        when_active: impl FnOnce(Value) -> T,
    ) -> Result<T> {
        self.when(feature, when_active, when_inactive).await
    }
    pub async fn activate(&self, feature: &str) -> Result<()> {
        self.activate_with_value(&[feature], Value::Bool(true))
            .await
    }
    pub async fn activate_with_value(&self, features: &[&str], value: Value) -> Result<()> {
        let values: Vec<_> = features
            .iter()
            .flat_map(|feature| {
                self.scopes().into_iter().map(|scope| FeatureValue {
                    feature: (*feature).into(),
                    scope,
                    value: value.clone(),
                })
            })
            .collect();
        self.driver.set_all(&values).await
    }
    pub async fn deactivate(&self, feature: &str) -> Result<()> {
        self.activate_with_value(&[feature], Value::Bool(false))
            .await
    }
    pub async fn forget(&self, features: &[&str]) -> Result<()> {
        for feature in features {
            for scope in self.scopes() {
                self.driver.delete(feature, &scope).await?;
            }
        }
        Ok(())
    }
}

use crate::{
    Driver, Error, FeatureFuture, FeatureScopes, FeatureValue, FeatureValues, Resolver, Result,
    Scope,
};
use openrust::database::{Connection, DatabaseError, Row, Value as Sql, query::Record};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct DatabaseDriver {
    connection: Connection,
    table: String,
    timezone: openrust::support::DateTimezone,
    pub(crate) resolvers: crate::contracts::Definitions,
    pub(crate) useMorphMap: Arc<AtomicBool>,
    pub(crate) events: Option<Arc<openrust::events::Dispatcher>>,
}
impl DatabaseDriver {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            table: "features".into(),
            timezone: openrust::support::DateTimezone::UTC,
            resolvers: Arc::default(),
            useMorphMap: Arc::new(AtomicBool::new(false)),
            events: None,
        }
    }
    pub fn withTable(mut self, table: impl Into<String>) -> Result<Self> {
        let table = table.into();
        if table.is_empty() {
            return Err(Error::InvalidConfiguration);
        }
        self.table = table;
        Ok(self)
    }
    pub fn withTimezone(mut self, timezone: &str) -> Result<Self> {
        self.timezone =
            openrust::support::Date::timezone(timezone).map_err(|_| Error::InvalidConfiguration)?;
        Ok(self)
    }
    async fn retrieve(&self, feature: &str, scope: &Scope) -> Result<Option<Value>> {
        self.connection
            .table(&self.table)
            .r#where("name", "=", feature)
            .r#where(
                "scope",
                "=",
                scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
            )
            .first()
            .await?
            .map(|row| decode(&row))
            .transpose()
    }
    fn resolver(&self, feature: &str) -> Result<Option<Resolver>> {
        Ok(self
            .resolvers
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(feature)
            .cloned())
    }
}
impl Driver for DatabaseDriver {
    fn supportsSetAll(&self) -> bool {
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
            self.connection
                .table(&self.table)
                .distinct()
                .pluck("name")
                .await?
                .into_iter()
                .map(|value| match value {
                    Sql::Text(value) => Ok(value),
                    _ => Err(Error::InvalidValue),
                })
                .collect()
        })
    }
    fn get<'a>(&'a self, feature: &'a str, scope: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async move {
            for retry in 0..=1 {
                if let Some(value) = self.retrieve(feature, scope).await? {
                    return Ok(value);
                }
                let Some(resolver) = self.resolver(feature)? else {
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
                match self
                    .connection
                    .table(&self.table)
                    .insert(&[record(
                        feature,
                        scope,
                        &value,
                        self.useMorphMap.load(Ordering::Relaxed),
                        &self.timezone,
                    )?])
                    .await
                {
                    Ok(_) => return Ok(value),
                    Err(source) if unique(&source) && retry == 0 => continue,
                    Err(source) => return Err(source.into()),
                }
            }
            Err(Error::InvalidValue)
        })
    }
    fn getAll<'a>(&'a self, features: &'a FeatureScopes) -> FeatureFuture<'a, FeatureValues> {
        Box::pin(async move {
            if features.values().all(Vec::is_empty) {
                return Ok(features.keys().map(|name| (name.clone(), vec![])).collect());
            }
            for retry in 0..=2 {
                let mut query = self.connection.table(&self.table);
                for (feature, scopes) in features {
                    for scope in scopes {
                        query = query.orWhereNested(|query| {
                            query.r#where("name", "=", feature.as_str()).r#where(
                                "scope",
                                "=",
                                scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
                            )
                        });
                    }
                }
                let rows = query.get().await?;
                let mut records = BTreeMap::new();
                for row in rows {
                    let text = |name| match row.get(name) {
                        Some(Sql::Text(value)) => Ok(value.clone()),
                        _ => Err(Error::InvalidValue),
                    };
                    records.insert((text("name")?, text("scope")?), decode(&row)?);
                }
                let mut results = FeatureValues::new();
                let mut inserts = Vec::new();
                for (feature, scopes) in features {
                    let resolver = self.resolver(feature)?;
                    let mut values = Vec::new();
                    for scope in scopes {
                        let key = (
                            feature.clone(),
                            scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
                        );
                        let value = if let Some(value) = records.get(&key) {
                            value.clone()
                        } else if let Some(resolver) = &resolver {
                            let value = resolver(scope.clone()).await?;
                            // Duplicate requested scopes share the first resolved insert.
                            records.insert(key, value.clone());
                            inserts.push(record(
                                feature,
                                scope,
                                &value,
                                self.useMorphMap.load(Ordering::Relaxed),
                                &self.timezone,
                            )?);
                            value
                        } else {
                            crate::events::dispatch(
                                self.events.as_ref(),
                                crate::events::UnknownFeatureResolved {
                                    feature: feature.into(),
                                    scope: scope.clone(),
                                },
                            )
                            .await?;
                            Value::Bool(false)
                        };
                        values.push(value);
                    }
                    results.insert(feature.clone(), values);
                }
                match self.connection.table(&self.table).insert(&inserts).await {
                    Ok(_) => return Ok(results),
                    Err(source) if unique(&source) && retry < 2 => continue,
                    Err(source) => return Err(source.into()),
                }
            }
            Err(Error::InvalidValue)
        })
    }
    fn set<'a>(
        &'a self,
        feature: &'a str,
        scope: &'a Scope,
        value: Value,
    ) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            self.connection
                .table(&self.table)
                .upsert(
                    &[record(
                        feature,
                        scope,
                        &value,
                        self.useMorphMap.load(Ordering::Relaxed),
                        &self.timezone,
                    )?],
                    &["name", "scope"],
                    Some(&["value", "updated_at"]),
                )
                .await?;
            Ok(())
        })
    }
    fn setAll<'a>(&'a self, features: &'a [FeatureValue]) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            let rows = features
                .iter()
                .map(|feature| {
                    record(
                        &feature.feature,
                        &feature.scope,
                        &feature.value,
                        self.useMorphMap.load(Ordering::Relaxed),
                        &self.timezone,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            self.connection
                .table(&self.table)
                .upsert(&rows, &["name", "scope"], Some(&["value", "updated_at"]))
                .await?;
            Ok(())
        })
    }
    fn setForAllScopes<'a>(&'a self, feature: &'a str, value: Value) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            self.connection
                .table(&self.table)
                .r#where("name", "=", feature)
                .update(BTreeMap::from([
                    ("value".into(), Sql::Text(value.to_string())),
                    ("updated_at".into(), timestamp(&self.timezone)?),
                ]))
                .await?;
            Ok(())
        })
    }
    fn delete<'a>(&'a self, feature: &'a str, scope: &'a Scope) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            self.connection
                .table(&self.table)
                .r#where("name", "=", feature)
                .r#where(
                    "scope",
                    "=",
                    scope.serialize(self.useMorphMap.load(Ordering::Relaxed)),
                )
                .delete()
                .await?;
            Ok(())
        })
    }
    fn purge<'a>(&'a self, features: Option<&'a [String]>) -> FeatureFuture<'a, ()> {
        Box::pin(async move {
            let query = self.connection.table(&self.table);
            match features {
                None => {
                    query.delete().await?;
                }
                Some(features) => {
                    query
                        .where_in("name", features.iter().cloned().map(Sql::Text).collect())
                        .delete()
                        .await?;
                }
            }
            Ok(())
        })
    }
}
fn record(
    feature: &str,
    scope: &Scope,
    value: &Value,
    useMorphMap: bool,
    timezone: &openrust::support::DateTimezone,
) -> Result<Record> {
    let now = timestamp(timezone)?;
    Ok(BTreeMap::from([
        ("name".into(), feature.into()),
        ("scope".into(), scope.serialize(useMorphMap).into()),
        ("value".into(), value.to_string().into()),
        ("created_at".into(), now.clone()),
        ("updated_at".into(), now),
    ]))
}
fn timestamp(timezone: &openrust::support::DateTimezone) -> Result<Sql> {
    Ok(openrust::support::Date::now(Some(timezone.clone()))
        .map_err(|_| Error::InvalidValue)?
        .asDateTime()
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
        .into())
}
fn decode(row: &Row) -> Result<Value> {
    match row.get("value") {
        Some(Sql::Text(value)) => serde_json::from_str(value).map_err(|_| Error::InvalidValue),
        _ => Err(Error::InvalidValue),
    }
}
fn unique(error: &DatabaseError) -> bool {
    matches!(error, DatabaseError::Driver { source, .. } if source.as_database_error().is_some_and(|error| error.is_unique_violation()))
}

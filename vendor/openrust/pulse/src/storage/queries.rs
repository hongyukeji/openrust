use super::DatabaseStorage;
use super::database_storage::number;
use crate::{Error, Result};
use indexmap::IndexMap;
use openrust::database::{Row, Value as Sql, query::Builder};
use std::collections::BTreeMap;

pub type Graph = BTreeMap<String, IndexMap<String, IndexMap<String, Option<f64>>>>;
fn validate(aggregate: &str) -> Result<()> {
    if ["count", "min", "max", "sum", "avg"].contains(&aggregate) {
        Ok(())
    } else {
        Err(Error::InvalidAggregate(aggregate.into()))
    }
}
impl DatabaseStorage {
    fn interval(&self, interval: i64) -> Result<(i64, i64, i64)> {
        if interval < 60 || interval % 60 != 0 {
            return Err(Error::InvalidConfiguration(
                "interval must contain whole positive minutes".into(),
            ));
        }
        let now = (self.clock)();
        let period = interval / 60;
        let oldest = now.div_euclid(period) * period - interval + period;
        Ok((period, now - interval + 1, oldest))
    }
    fn aggregateSql(&self, aggregate: &str, column: &str) -> String {
        format!(
            "{}({})",
            if aggregate == "count" {
                "sum"
            } else {
                aggregate
            },
            self.wrap(column)
        )
    }
    pub async fn graph(&self, types: &[String], aggregate: &str, interval: i64) -> Result<Graph> {
        validate(aggregate)?;
        let (period, _, _) = self.interval(interval)?;
        let now = (self.clock)();
        let first = now.div_euclid(period) * period - 59 * period;
        let rows = self
            .connection
            .table("pulse_aggregates")
            .select(["bucket", "type", "key", "value"])
            .whereIn("type", types.iter().cloned().map(Sql::Text).collect())
            .r#where("aggregate", "=", aggregate)
            .r#where("period", "=", period)
            .r#where("bucket", ">=", first)
            .orderBy("bucket", "asc")
            .get()
            .await?;
        let mut padding = IndexMap::new();
        for i in 0..60 {
            padding.insert(bucketLabel(first + i * period)?, None);
        }
        let structure: IndexMap<_, _> =
            types.iter().map(|t| (t.clone(), padding.clone())).collect();
        let mut graph: Graph = BTreeMap::new();
        for row in rows {
            let key = text(&row, "key")?;
            let r#type = text(&row, "type")?;
            let bucket = row
                .get("bucket")
                .and_then(number)
                .ok_or(Error::StateUnavailable)? as i64;
            graph
                .entry(key)
                .or_insert_with(|| structure.clone())
                .entry(r#type)
                .or_insert_with(|| padding.clone())
                .insert(bucketLabel(bucket)?, row.get("value").and_then(number));
        }
        Ok(graph)
    }
    fn mapKeys(&self, query: Builder<'static>, columns: &[String]) -> Result<Builder<'static>> {
        let keys = self
            .connection
            .table("pulse_entries as keys")
            .select(["key"])
            .whereColumn("keys.key_hash", "=", "aggregated.key_hash")
            .limit(1);
        Ok(self
            .connection
            .table("pulse_entries")
            .select(columns.iter().map(String::as_str))
            .selectSub(&keys, "key")?
            .fromSub(&query, "aggregated")?)
    }
    pub async fn aggregate(
        &self,
        r#type: &str,
        aggregates: &[String],
        interval: i64,
        orderBy: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> Result<Vec<Row>> {
        if aggregates.is_empty() {
            return Ok(Vec::new());
        }
        for a in aggregates {
            validate(a)?;
        }
        let order = orderBy.unwrap_or(&aggregates[0]);
        if !aggregates.iter().any(|a| a == order) {
            return Err(Error::InvalidAggregate(order.into()));
        }
        let (period, window, oldest) = self.interval(interval)?;
        let mut tail = self.connection.table("pulse_entries").select(["key_hash"]);
        for a in aggregates {
            let expression = if a == "count" {
                "count(*)".into()
            } else {
                format!("{a}({})", self.wrap("value"))
            };
            tail = tail.selectRaw(&format!("{expression} as {}", self.wrap(a)), vec![]);
        }
        tail = tail
            .r#where("type", "=", r#type)
            .r#where("timestamp", ">=", window)
            .r#where("timestamp", "<=", oldest - 1)
            .groupBy(&["key_hash"]);
        for current in aggregates {
            let mut buckets = self
                .connection
                .table("pulse_aggregates")
                .select(["key_hash"]);
            for a in aggregates {
                let expression = if a == current {
                    self.aggregateSql(a, "value")
                } else {
                    "null".into()
                };
                buckets = buckets.selectRaw(&format!("{expression} as {}", self.wrap(a)), vec![]);
            }
            buckets = buckets
                .r#where("period", "=", period)
                .r#where("type", "=", r#type)
                .r#where("aggregate", "=", current.as_str())
                .r#where("bucket", ">=", oldest)
                .groupBy(&["key_hash"]);
            tail = tail.unionAll(&buckets)?;
        }
        let mut grouped = self
            .connection
            .table("pulse_entries")
            .select(["key_hash"])
            .fromSub(&tail, "results")?;
        for a in aggregates {
            grouped = grouped.selectRaw(
                &format!("{} as {}", self.aggregateSql(a, a), self.wrap(a)),
                vec![],
            );
        }
        grouped = grouped
            .groupBy(&["key_hash"])
            .orderBy(order, direction)
            .limit(limit);
        Ok(self.mapKeys(grouped, aggregates)?.get().await?)
    }
    pub async fn aggregateTypes(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
        orderBy: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> Result<Vec<Row>> {
        validate(aggregate)?;
        if types.is_empty() {
            return Ok(vec![]);
        }
        let order = orderBy.unwrap_or(&types[0]);
        if !types.iter().any(|t| t == order) {
            return Err(Error::InvalidConfiguration(
                "orderBy must be a requested type".into(),
            ));
        }
        let (period, window, oldest) = self.interval(interval)?;
        let mut tail = self.connection.table("pulse_entries").select(["key_hash"]);
        let mut buckets = self
            .connection
            .table("pulse_aggregates")
            .select(["key_hash"]);
        for t in types {
            let conditional = format!(
                "case when ({} = ?) then {} else null end",
                self.wrap("type"),
                if aggregate == "count" {
                    "true".into()
                } else {
                    self.wrap("value")
                }
            );
            tail = tail.selectRaw(
                &format!("{aggregate}({conditional}) as {}", self.wrap(t)),
                vec![t.clone().into()],
            );
            let conditional = format!(
                "case when ({} = ?) then {} else null end",
                self.wrap("type"),
                self.wrap("value")
            );
            buckets = buckets.selectRaw(
                &format!(
                    "{}({conditional}) as {}",
                    if aggregate == "count" {
                        "sum"
                    } else {
                        aggregate
                    },
                    self.wrap(t)
                ),
                vec![t.clone().into()],
            );
        }
        tail = tail
            .whereIn("type", types.iter().cloned().map(Sql::Text).collect())
            .r#where("timestamp", ">=", window)
            .r#where("timestamp", "<=", oldest - 1)
            .groupBy(&["key_hash"]);
        buckets = buckets
            .r#where("period", "=", period)
            .whereIn("type", types.iter().cloned().map(Sql::Text).collect())
            .r#where("aggregate", "=", aggregate)
            .r#where("bucket", ">=", oldest)
            .groupBy(&["key_hash"]);
        tail = tail.unionAll(&buckets)?;
        let mut grouped = self
            .connection
            .table("pulse_entries")
            .select(["key_hash"])
            .fromSub(&tail, "results")?;
        for t in types {
            grouped = grouped.selectRaw(
                &format!("{} as {}", self.aggregateSql(aggregate, t), self.wrap(t)),
                vec![],
            );
        }
        grouped = grouped
            .groupBy(&["key_hash"])
            .orderBy(order, direction)
            .limit(limit);
        Ok(self.mapKeys(grouped, types)?.get().await?)
    }
    pub async fn aggregateTotal(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> Result<IndexMap<String, Option<f64>>> {
        validate(aggregate)?;
        let (period, window, oldest) = self.interval(interval)?;
        let tailSql = if aggregate == "count" {
            "count(*)".into()
        } else {
            format!("{aggregate}({})", self.wrap("value"))
        };
        let mut tail = self
            .connection
            .table("pulse_entries")
            .select(["type"])
            .selectRaw(&format!("{tailSql} as {}", self.wrap(aggregate)), vec![])
            .whereIn("type", types.iter().cloned().map(Sql::Text).collect())
            .r#where("timestamp", ">=", window)
            .r#where("timestamp", "<=", oldest - 1)
            .groupBy(&["type"]);
        let buckets = self
            .connection
            .table("pulse_aggregates")
            .select(["type"])
            .selectRaw(
                &format!(
                    "{} as {}",
                    self.aggregateSql(aggregate, "value"),
                    self.wrap(aggregate)
                ),
                vec![],
            )
            .r#where("period", "=", period)
            .whereIn("type", types.iter().cloned().map(Sql::Text).collect())
            .r#where("aggregate", "=", aggregate)
            .r#where("bucket", ">=", oldest)
            .groupBy(&["type"]);
        tail = tail.unionAll(&buckets)?;
        let rows = self
            .connection
            .table("pulse_entries")
            .select(["type"])
            .selectRaw(
                &format!(
                    "{} as {}",
                    self.aggregateSql(aggregate, aggregate),
                    self.wrap(aggregate)
                ),
                vec![],
            )
            .fromSub(&tail, "child")?
            .groupBy(&["type"])
            .get()
            .await?;
        rows.into_iter()
            .map(|r| Ok((text(&r, "type")?, r.get(aggregate).and_then(number))))
            .collect()
    }
}
fn text(row: &Row, column: &str) -> Result<String> {
    match row.get(column) {
        Some(Sql::Text(v)) => Ok(v.clone()),
        _ => Err(Error::StateUnavailable),
    }
}
fn bucketLabel(timestamp: i64) -> Result<String> {
    Ok(jiff::Timestamp::from_second(timestamp)
        .map_err(|_| Error::StateUnavailable)?
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string())
}

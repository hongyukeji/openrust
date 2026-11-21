use crate::{Entry, Item, Result};
use indexmap::IndexMap;
use openrust::database::{
    Connection, Value as Sql,
    query::{Expression, Record, UpsertUpdate},
};
use std::{collections::BTreeMap, sync::Arc};

type AggregateKey = (String, i64, i64, String);
type AggregateGroups = BTreeMap<String, IndexMap<AggregateKey, Record>>;

pub struct DatabaseStorage {
    pub(super) connection: Connection,
    chunk: usize,
    keep: i64,
    pub(super) clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}
impl DatabaseStorage {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            chunk: 1000,
            keep: 604800,
            clock: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
            }),
        }
    }
    pub fn withClock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }
    pub fn withChunk(mut self, chunk: usize) -> Result<Self> {
        if chunk == 0 {
            return Err(crate::Error::InvalidConfiguration(
                "storage.database.chunk must be positive".into(),
            ));
        }
        self.chunk = chunk;
        Ok(self)
    }
    pub fn withKeep(mut self, seconds: i64) -> Self {
        self.keep = seconds;
        self
    }
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub(super) fn wrap(&self, identifier: &str) -> String {
        let quote = if self.connection.getDriverName() == "mysql"
            || self.connection.getDriverName() == "mariadb"
        {
            '`'
        } else {
            '"'
        };
        format!(
            "{quote}{}{quote}",
            identifier.replace(quote, &format!("{quote}{quote}"))
        )
    }
    fn keyHash(&self, key: &str, row: &mut Record) {
        if self.connection.getDriverName() == "sqlite" {
            row.insert(
                "key_hash".into(),
                format!("{:x}", md5::compute(key.as_bytes())).into(),
            );
        }
    }
    fn entryRow(&self, entry: &Entry) -> Record {
        let mut row = Record::from([
            ("timestamp".into(), entry.timestamp.into()),
            ("type".into(), entry.r#type.clone().into()),
            ("key".into(), entry.key.clone().into()),
            (
                "value".into(),
                entry.value.map(Sql::Integer).unwrap_or(Sql::Null),
            ),
        ]);
        self.keyHash(&entry.key, &mut row);
        row
    }
    fn preaggregate(&self, items: &[Item]) -> BTreeMap<String, Vec<Record>> {
        let mut groups: AggregateGroups = BTreeMap::new();
        let now = (self.clock)();
        for item in items {
            let Item::Entry(entry) = item else { continue };
            for aggregate in entry.aggregations() {
                for period in [60i64, 360, 1440, 10080] {
                    if entry.timestamp < now - period * 60 {
                        continue;
                    }
                    let bucket = entry.timestamp.div_euclid(period) * period;
                    let key = (entry.r#type.clone(), period, bucket, entry.key.clone());
                    let row = groups
                        .entry(aggregate.clone())
                        .or_default()
                        .entry(key)
                        .or_insert_with(|| {
                            let mut row = Record::from([
                                ("bucket".into(), bucket.into()),
                                ("period".into(), period.into()),
                                ("type".into(), entry.r#type.clone().into()),
                                ("aggregate".into(), aggregate.clone().into()),
                                ("key".into(), entry.key.clone().into()),
                            ]);
                            self.keyHash(&entry.key, &mut row);
                            row
                        });
                    let previous = row.get("value").and_then(number);
                    let next = entry.value.map(|v| v as f64);
                    let count = row.get("count").and_then(number).unwrap_or(0.0);
                    let value = match aggregate.as_str() {
                        "count" => Some(previous.unwrap_or(0.0) + 1.0),
                        "sum" => Some(previous.unwrap_or(0.0) + next.unwrap_or(0.0)),
                        "min" => previous.map_or(next, |v| {
                            Some(if entry.value.is_none() {
                                0.0
                            } else {
                                v.min(next.unwrap_or(0.0))
                            })
                        }),
                        "max" => previous.map_or(next, |v| {
                            Some(if entry.value.is_none() {
                                v
                            } else {
                                v.max(next.unwrap_or(0.0))
                            })
                        }),
                        "avg" => {
                            row.insert("count".into(), Sql::Integer(count as i64 + 1));
                            previous.map_or(next, |v| {
                                Some((v * count + next.unwrap_or(0.0)) / (count + 1.0))
                            })
                        }
                        _ => unreachable!("Entry validates aggregation names"),
                    };
                    row.insert("value".into(), value.map(Sql::Real).unwrap_or(Sql::Null));
                }
            }
        }
        groups
            .into_iter()
            .map(|(key, rows)| (key, rows.into_values().collect()))
            .collect()
    }
    fn assignments(&self, aggregate: &str) -> Vec<UpsertUpdate<'static>> {
        let mysql = matches!(self.connection.getDriverName(), "mysql" | "mariadb");
        let alias = mysql
            && self
                .connection
                .getConfig("use_upsert_alias")
                .and_then(|v| v.as_bool())
                == Some(true);
        let table = self.wrap(&format!(
            "{}pulse_aggregates",
            self.connection.getTablePrefix()
        ));
        let current = |c: &str| {
            if mysql && !alias {
                self.wrap(c)
            } else {
                format!("{table}.{}", self.wrap(c))
            }
        };
        let inserted = |c: &str| {
            if alias {
                format!("{}.{}", self.wrap("laravel_upsert_alias"), self.wrap(c))
            } else if mysql {
                format!("values({})", self.wrap(c))
            } else {
                format!("{}.{}", self.wrap("excluded"), self.wrap(c))
            }
        };
        let v = current("value");
        let n = inserted("value");
        let expression = match aggregate {
            "count" | "sum" => format!("{v} + {n}"),
            "min" => format!(
                "{}({v}, {n})",
                if self.connection.getDriverName() == "sqlite" {
                    "min"
                } else {
                    "least"
                }
            ),
            "max" => format!(
                "{}({v}, {n})",
                if self.connection.getDriverName() == "sqlite" {
                    "max"
                } else {
                    "greatest"
                }
            ),
            "avg" => format!(
                "({v} * {} + ({n} * {})) / ({} + {})",
                current("count"),
                inserted("count"),
                current("count"),
                inserted("count")
            ),
            _ => unreachable!(),
        };
        let mut assignments = vec![UpsertUpdate::Expression(
            "value",
            Expression::new(expression),
        )];
        if aggregate == "avg" {
            assignments.push(UpsertUpdate::Expression(
                "count",
                Expression::new(format!("{} + {}", current("count"), inserted("count"))),
            ));
        }
        assignments
    }
    pub async fn store(&self, items: &[Item]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let entries: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Entry(e) if !e.isOnlyBuckets() => Some(self.entryRow(e)),
                _ => None,
            })
            .collect();
        let mut values = IndexMap::new();
        // PHP reverse()->unique() retains the last supplied value, irrespective of timestamp.
        for item in items.iter().rev() {
            if let Item::Value(value) = item {
                values
                    .entry((value.r#type.clone(), value.key.clone()))
                    .or_insert_with(|| {
                        let mut row = Record::from([
                            ("timestamp".into(), value.timestamp.into()),
                            ("type".into(), value.r#type.clone().into()),
                            ("key".into(), value.key.clone().into()),
                            ("value".into(), value.value.clone().into()),
                        ]);
                        self.keyHash(&value.key, &mut row);
                        row
                    });
            }
        }
        let values: Vec<_> = values.into_values().collect();
        let aggregates = self.preaggregate(items);
        let updates: Vec<_> = aggregates
            .into_iter()
            .map(|(name, rows)| {
                let assignments = self.assignments(&name);
                (rows, assignments)
            })
            .collect();
        let entries = Arc::new(entries);
        let values = Arc::new(values);
        let updates = Arc::new(updates);
        let chunk = self.chunk;
        self.connection
            .transaction(
                move |transaction| {
                    let entries = entries.clone();
                    let values = values.clone();
                    let updates = updates.clone();
                    Box::pin(async move {
                        for rows in entries.chunks(chunk) {
                            transaction.table("pulse_entries").insert(rows).await?;
                        }
                        for (rows, assignments) in updates.iter() {
                            for rows in rows.chunks(chunk) {
                                transaction
                                    .table("pulse_aggregates")
                                    .upsertWith(
                                        rows,
                                        &["bucket", "period", "type", "aggregate", "key_hash"],
                                        assignments,
                                    )
                                    .await?;
                            }
                        }
                        for rows in values.chunks(chunk) {
                            transaction
                                .table("pulse_values")
                                .upsert(rows, &["type", "key_hash"], Some(&["timestamp", "value"]))
                                .await?;
                        }
                        Ok(())
                    })
                },
                3,
            )
            .await?;
        Ok(())
    }
    pub async fn purge(&self, types: Option<&[String]>) -> Result<()> {
        for table in ["pulse_values", "pulse_entries", "pulse_aggregates"] {
            let query = self.connection.table(table);
            if let Some(types) = types {
                query
                    .whereIn("type", types.iter().cloned().map(Sql::Text).collect())
                    .delete()
                    .await?;
            } else {
                query.truncate().await?;
            }
        }
        Ok(())
    }
    pub async fn trim(&self) -> Result<()> {
        let now = (self.clock)();
        let before = (now - self.keep).max(now - 604800);
        for table in ["pulse_values", "pulse_entries"] {
            self.connection
                .table(table)
                .r#where("timestamp", "<=", before)
                .delete()
                .await?;
        }
        let periods = self
            .connection
            .table("pulse_aggregates")
            .distinct()
            .pluck("period")
            .await?;
        for period in periods {
            if let Some(p) = number(&period) {
                self.connection
                    .table("pulse_aggregates")
                    .r#where("period", "=", period)
                    .r#where("bucket", "<=", (now - p as i64 * 60).max(before))
                    .delete()
                    .await?;
            }
        }
        Ok(())
    }
    pub async fn values(
        &self,
        r#type: &str,
        keys: Option<&[String]>,
    ) -> Result<Vec<openrust::database::Row>> {
        let mut q = self
            .connection
            .table("pulse_values")
            .select(["timestamp", "key", "value"])
            .r#where("type", "=", r#type);
        if let Some(keys) = keys.filter(|k| !k.is_empty()) {
            q = q.whereIn("key", keys.iter().cloned().map(Sql::Text).collect());
        }
        Ok(q.get().await?)
    }
}
impl super::Storage for DatabaseStorage {
    fn values<'a>(
        &'a self,
        r#type: &'a str,
        keys: Option<&'a [String]>,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(self.values(r#type, keys))
    }
    fn graph<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
    ) -> crate::PulseFuture<'a, super::Graph> {
        Box::pin(self.graph(types, aggregate, interval))
    }
    fn aggregate<'a>(
        &'a self,
        r#type: &'a str,
        aggregates: &'a [String],
        interval: i64,
        orderBy: Option<&'a str>,
        direction: &'a str,
        limit: u64,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(self.aggregate(r#type, aggregates, interval, orderBy, direction, limit))
    }
    fn aggregateTypes<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
        orderBy: Option<&'a str>,
        direction: &'a str,
        limit: u64,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(self.aggregateTypes(types, aggregate, interval, orderBy, direction, limit))
    }
    fn aggregateTotal<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
    ) -> crate::PulseFuture<'a, indexmap::IndexMap<String, Option<f64>>> {
        Box::pin(self.aggregateTotal(types, aggregate, interval))
    }

    fn store<'a>(&'a self, items: &'a [Item]) -> crate::PulseFuture<'a, ()> {
        Box::pin(self.store(items))
    }
    fn purge<'a>(&'a self, types: Option<&'a [String]>) -> crate::PulseFuture<'a, ()> {
        Box::pin(self.purge(types))
    }
    fn trim(&self) -> crate::PulseFuture<'_, ()> {
        Box::pin(self.trim())
    }
}
pub(super) fn number(v: &Sql) -> Option<f64> {
    match v {
        Sql::Integer(v) => Some(*v as f64),
        Sql::Unsigned(v) => Some(*v as f64),
        Sql::Real(v) => Some(*v),
        Sql::Text(v) => v.parse().ok(),
        _ => None,
    }
}

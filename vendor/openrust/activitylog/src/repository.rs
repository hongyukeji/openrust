use crate::{Activity, ActivityRecord, Error, IntoLogName, Result, Subject};
use openrust::database::{Connection, DatabaseError, DatabaseResult, Row, Transaction, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Top-level property and changed-attribute allowlists. Nothing is recorded by default.
/// Secret-shaped keys are redacted recursively even when explicitly allowed.
#[derive(Clone, Default)]
pub struct Redaction {
    properties: BTreeSet<String>,
    attributes: BTreeSet<String>,
    excluded: BTreeSet<String>,
}
impl Redaction {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn log_properties(mut self, keys: &[&str]) -> Self {
        self.properties.extend(keys.iter().map(|s| (*s).into()));
        self
    }
    pub fn log_only(mut self, attributes: &[&str]) -> Self {
        self.attributes
            .extend(attributes.iter().map(|s| (*s).into()));
        self
    }
    pub fn log_except(mut self, attributes: &[&str]) -> Self {
        self.excluded
            .extend(attributes.iter().map(|s| normalize(s)));
        self
    }
    fn clean(
        &self,
        value: &serde_json::Value,
        allowed: &BTreeSet<String>,
    ) -> Result<serde_json::Value> {
        let object = value.as_object().ok_or(Error::InvalidActivity)?;
        let mut clean = serde_json::Map::new();
        for (key, value) in object {
            if allowed.contains(key) {
                clean.insert(key.clone(), self.redact(key, value, 0)?);
            }
        }
        Ok(clean.into())
    }
    fn redact(
        &self,
        key: &str,
        value: &serde_json::Value,
        depth: usize,
    ) -> Result<serde_json::Value> {
        if depth > 16 {
            return Err(Error::InvalidActivity);
        }
        let normalized = normalize(key);
        if self.excluded.contains(&normalized)
            || [
                "password",
                "secret",
                "token",
                "authorization",
                "cookie",
                "privatekey",
                "apikey",
                "sessionkey",
                "credential",
                "otp",
                "verificationcode",
            ]
            .iter()
            .any(|s| normalized.contains(s))
        {
            return Ok("[REDACTED]".into());
        }
        match value {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(k, v)| Ok((k.clone(), self.redact(k, v, depth + 1)?)))
                .collect::<Result<serde_json::Map<_, _>>>()
                .map(Into::into),
            serde_json::Value::Array(values) => values
                .iter()
                .map(|v| self.redact("", v, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Into::into),
            _ => Ok(value.clone()),
        }
    }
}
fn normalize(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone)]
pub struct ActivityRepository {
    connection: Connection,
    table: &'static str,
    model: std::sync::Arc<crate::models::ActivityModel>,
    policy: Option<Redaction>,
    enabled: bool,
    pub(crate) default_log_name: Option<String>,
    pub(crate) default_except_attributes: Vec<String>,
}
impl ActivityRepository {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            table: "activity_log",
            model: std::sync::Arc::new(
                crate::models::ActivityModel::of::<crate::models::Activity>(),
            ),
            policy: None,
            enabled: true,
            default_log_name: Some("default".into()),
            default_except_attributes: Vec::new(),
        }
    }
    pub fn with_activity_model<M: openrust::database::eloquent::Model>(self) -> Self {
        self.with_model(std::sync::Arc::new(crate::models::ActivityModel::of::<M>()))
    }
    pub(crate) fn with_model(
        mut self,
        model: std::sync::Arc<crate::models::ActivityModel>,
    ) -> Self {
        self.table = model.table;
        self.model = model;
        self
    }
    pub fn with_default_except_attributes(mut self, names: Vec<String>) -> Self {
        self.default_except_attributes = names;
        self
    }
    pub fn with_default_log_name(mut self, name: impl IntoLogName) -> Self {
        self.default_log_name = name.into_log_name();
        self
    }
    pub(crate) fn uses_connection(&self, connection: &Connection) -> bool {
        self.connection.get_name() == connection.get_name()
            && self.connection.get_database_name() == connection.get_database_name()
    }
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn with_redaction(mut self, policy: Redaction) -> Self {
        self.policy = Some(policy);
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn prepare(&self, activity: Activity) -> Result<Option<PreparedActivity>> {
        activity.validate()?;
        if !self.enabled {
            return Ok(None);
        }
        let (properties, changes) = if let Some(policy) = &self.policy {
            let properties = policy.clean(&activity.properties, &policy.properties)?;
            let mut changes = serde_json::Map::new();
            for (key, value) in activity.changes.as_object().ok_or(Error::InvalidActivity)? {
                changes.insert(key.clone(), policy.clean(value, &policy.attributes)?);
            }
            (properties, changes.into())
        } else {
            (activity.properties.clone(), activity.changes.clone())
        };
        let record = ActivityRecord {
            id: None,
            log_name: activity.log_name,
            description: activity.description.unwrap_or_default(),
            event: activity.event,
            subject: activity.subject,
            causer: activity.causer,
            request_id: activity.request_id,
            properties,
            attribute_changes: changes,
            created_at: activity
                .created_at
                .unwrap_or_else(|| jiff::Timestamp::now().as_millisecond()),
        };
        if self.policy.is_some()
            && serde_json::to_vec(&record)
                .map_err(|_| Error::InvalidActivity)?
                .len()
                > 16_384
        {
            return Err(Error::InvalidActivity);
        }
        sql_timestamp(record.created_at)?;
        Ok(Some(PreparedActivity(record, self.model.clone())))
    }
    pub async fn log(&self, activity: Activity) -> Result<Option<ActivityRecord>> {
        let Some(prepared) = self.prepare(activity)? else {
            return Ok(None);
        };
        self.save_prepared(prepared).await.map(Some)
    }
    pub(crate) async fn insert_buffered(&self, records: Vec<PreparedActivity>) -> Result<()> {
        let rows: Vec<_> = records.iter().map(PreparedActivity::row).collect();
        let table = self.table;
        self.connection
            .transaction(
                move |tx| {
                    let rows = rows.clone();
                    Box::pin(async move { tx.table(table).insert(&rows).await.map(|_| ()) })
                },
                1,
            )
            .await
            .map_err(Into::into)
    }
    pub(crate) async fn save_prepared(&self, prepared: PreparedActivity) -> Result<ActivityRecord> {
        (prepared.1.save)(&self.connection, &prepared)
            .await
            .map_err(Into::into)
    }
    /// The supplied transaction selects the database. Prepare before mutating business data.
    pub async fn log_in(
        &self,
        tx: &mut Transaction,
        activity: Activity,
    ) -> DatabaseResult<Option<ActivityRecord>> {
        let prepared = self
            .prepare(activity)
            .map_err(|_| DatabaseError::InvalidConfiguration("Invalid activity"))?;
        match prepared {
            None => Ok(None),
            Some(record) => record.log_in(tx).await.map(Some),
        }
    }
    pub fn query(&self) -> ActivityQuery {
        ActivityQuery {
            connection: self.connection.clone(),
            table: self.table,
            filters: Vec::new(),
            log_groups: Vec::new(),
            after: None,
            limit: 50,
        }
    }
    pub(crate) async fn delete_old_activities(
        &self,
        before: i64,
        log_name: Option<&str>,
    ) -> Result<u64> {
        let mut query =
            self.connection
                .table(self.table)
                .r#where("created_at", "<", sql_timestamp(before)?);
        if let Some(name) = log_name {
            query = query.where_exact_text("log_name", name);
        }
        query.delete().await.map_err(Into::into)
    }
    /// Deletes one bounded batch strictly before the UTC cutoff, sharing a transaction.
    pub async fn clean_before(&self, before: i64, limit: u64) -> Result<u64> {
        if before < 0
            || before > jiff::Timestamp::now().as_millisecond()
            || !(1..=1000).contains(&limit)
        {
            return Err(Error::InvalidActivity);
        }
        let connection = self.connection.clone();
        let table = self.table;
        let before = sql_timestamp(before)?;
        connection
            .transaction(
                move |tx| {
                    let before = before.clone();
                    Box::pin(async move {
                        let ids: Vec<Value> = tx
                            .table(table)
                            .r#where("created_at", "<", before.clone())
                            .order_by("created_at", "asc")
                            .limit(limit)
                            .get()
                            .await?
                            .into_iter()
                            .filter_map(|row| row.get("id").cloned())
                            .collect();
                        if ids.is_empty() {
                            return Ok(0);
                        }
                        tx.table(table)
                            .where_in("id", ids)
                            .r#where("created_at", "<", before.clone())
                            .delete()
                            .await
                    })
                },
                1,
            )
            .await
            .map_err(Into::into)
    }
}
#[derive(Clone)]
pub struct PreparedActivity(ActivityRecord, std::sync::Arc<crate::models::ActivityModel>);
impl PreparedActivity {
    pub fn record(&self) -> &ActivityRecord {
        &self.0
    }
    pub async fn log_in(&self, tx: &mut Transaction) -> DatabaseResult<ActivityRecord> {
        (self.1.save_in)(self, tx).await
    }
    pub(crate) fn row(&self) -> BTreeMap<String, Value> {
        let r = &self.0;
        let optional = |s: &Option<String>| s.clone().map_or(Value::Null, Into::into);
        let subject = |s: &Option<Subject>, kind: bool| {
            s.as_ref().map_or(Value::Null, |s| {
                if kind {
                    s.kind.clone().into()
                } else {
                    s.id.clone().into()
                }
            })
        };
        [
            ("log_name", optional(&r.log_name)),
            ("description", r.description.clone().into()),
            ("event", optional(&r.event)),
            ("subject_type", subject(&r.subject, true)),
            ("subject_id", subject(&r.subject, false)),
            ("causer_type", subject(&r.causer, true)),
            ("causer_id", subject(&r.causer, false)),
            ("request_id", optional(&r.request_id)),
            ("properties", Value::Json(r.properties.clone())),
            (
                "attribute_changes",
                Value::Json(r.attribute_changes.clone()),
            ),
            (
                "created_at",
                sql_timestamp(r.created_at)
                    .expect("validated timestamp")
                    .into(),
            ),
            (
                "updated_at",
                sql_timestamp(r.created_at)
                    .expect("validated timestamp")
                    .into(),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect()
    }
}
#[derive(Clone)]
pub struct ActivityQuery {
    connection: Connection,
    table: &'static str,
    filters: Vec<(&'static str, String)>,
    log_groups: Vec<Vec<String>>,
    after: Option<i64>,
    limit: u64,
}
impl ActivityQuery {
    pub fn in_log(self, name: impl Into<String>) -> Self {
        self.in_logs([name])
    }
    pub fn in_logs(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.log_groups
            .push(names.into_iter().map(Into::into).collect());
        self
    }
    pub fn for_event(mut self, event: impl Into<String>) -> Self {
        self.filters.push(("event", event.into()));
        self
    }
    pub fn for_subject(mut self, subject: &Subject) -> Self {
        self.filters.push(("subject_type", subject.kind.clone()));
        self.filters.push(("subject_id", subject.id.clone()));
        self
    }
    pub fn caused_by(mut self, subject: &Subject) -> Self {
        self.filters.push(("causer_type", subject.kind.clone()));
        self.filters.push(("causer_id", subject.id.clone()));
        self
    }
    pub fn for_request(mut self, id: impl Into<String>) -> Self {
        self.filters.push(("request_id", id.into()));
        self
    }
    /// Descending numeric cursor returned by the preceding page; no total-count scan.
    pub fn before(mut self, id: i64) -> Self {
        self.after = Some(id);
        self
    }
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }
    pub async fn get(self) -> Result<Vec<ActivityRecord>> {
        if !(1..=1000).contains(&self.limit) {
            return Err(Error::InvalidActivity);
        }
        let mut query = self.connection.table(self.table);
        for (column, value) in self.filters {
            query = query.where_exact_text(column, &value);
        }
        for names in self.log_groups {
            query = query.where_in("log_name", names.into_iter().map(Value::from).collect());
        }
        if let Some(id) = self.after {
            if id <= 0 {
                return Err(Error::InvalidActivity);
            }
            query = query.r#where("id", "<", id);
        }
        query
            .order_by_desc("id")
            .limit(self.limit)
            .get()
            .await?
            .iter()
            .map(decode)
            .collect()
    }
}
fn decode(row: &Row) -> Result<ActivityRecord> {
    decode_attributes(&row.columns().iter().cloned().collect())
}
pub(crate) fn decode_attributes(row: &BTreeMap<String, Value>) -> Result<ActivityRecord> {
    let text = |key: &str| match row.get(key) {
        Some(Value::Text(v)) => Ok(v.clone()),
        _ => Err(Error::InvalidActivity),
    };
    let optional = |key: &str| match row.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::Text(v)) => Ok(Some(v.clone())),
        _ => Err(Error::InvalidActivity),
    };
    let subject = |prefix: &str| -> Result<Option<Subject>> {
        match (
            optional(&format!("{prefix}_type"))?,
            match row.get(&format!("{prefix}_id")) {
                Some(Value::Null) => None,
                Some(Value::Text(value)) => Some(value.clone()),
                Some(Value::Integer(value)) => Some(value.to_string()),
                Some(Value::Unsigned(value)) => Some(value.to_string()),
                _ => return Err(Error::InvalidActivity),
            },
        ) {
            (None, None) => Ok(None),
            (Some(kind), Some(id)) => Subject::new(kind, id).map(Some),
            _ => Err(Error::InvalidActivity),
        }
    };
    Ok(ActivityRecord {
        id: row.get("id").map(record_id).transpose()?,
        log_name: optional("log_name")?,
        description: text("description")?,
        event: optional("event")?,
        subject: subject("subject")?,
        causer: subject("causer")?,
        request_id: optional("request_id")?,
        properties: json_column(row, "properties")?,
        attribute_changes: json_column(row, "attribute_changes")?,
        created_at: match row.get("created_at") {
            Some(Value::Text(value)) => format!("{}Z", value.replace(' ', "T"))
                .parse::<jiff::Timestamp>()
                .map_err(|_| Error::InvalidActivity)?
                .as_millisecond(),
            _ => return Err(Error::InvalidActivity),
        },
    })
}

fn record_id(value: &Value) -> Result<i64> {
    match value {
        Value::Integer(value) if *value > 0 => Ok(*value),
        Value::Unsigned(value) if *value > 0 => {
            i64::try_from(*value).map_err(|_| Error::InvalidActivity)
        }
        _ => Err(Error::InvalidActivity),
    }
}

fn sql_timestamp(milliseconds: i64) -> Result<String> {
    let timestamp =
        jiff::Timestamp::from_millisecond(milliseconds).map_err(|_| Error::InvalidActivity)?;
    Ok(timestamp
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string())
}
fn json_column(row: &BTreeMap<String, Value>, key: &str) -> Result<serde_json::Value> {
    match row.get(key) {
        Some(Value::Json(value)) => Ok(value.clone()),
        Some(Value::Text(value)) => serde_json::from_str(value).map_err(|_| Error::InvalidActivity),
        Some(Value::Null) => Ok(serde_json::json!({})),
        _ => Err(Error::InvalidActivity),
    }
}

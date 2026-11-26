use openrust::{
    database::{Connection, DatabaseError, DatabaseResult, Row, Value, query::Record},
    log::LogContext,
    support::Str,
};
use serde::Serialize;

use crate::managed_keys::{now, nullable};

const TABLE: &str = "ai_gateway_usage";
const COLUMNS: [&str; 19] = [
    "id",
    "request_id",
    "key_id",
    "key_name",
    "group_name",
    "model",
    "provider",
    "upstream_model",
    "endpoint",
    "upstream_endpoint",
    "outcome",
    "status",
    "input_tokens",
    "output_tokens",
    "cached_input_tokens",
    "cache_write_input_tokens",
    "reasoning_tokens",
    "started_at",
    "finished_at",
];

#[derive(Clone)]
pub(crate) struct UsageStore {
    connection: Connection,
}

pub(crate) struct UsageAttempt {
    store: UsageStore,
    pub(crate) id: String,
}

pub(crate) struct Settlement {
    outcome: &'static str,
    status: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    cache_write_input_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    finished_at: i64,
}

impl Settlement {
    pub(crate) fn from_context(outcome: &'static str, context: &LogContext) -> Self {
        let count = |key: &str| context.get(key).and_then(serde_json::Value::as_i64);
        Self {
            outcome,
            status: count("status"),
            input_tokens: count("input_tokens"),
            output_tokens: count("output_tokens"),
            cached_input_tokens: count("cached_input_tokens"),
            cache_write_input_tokens: count("cache_write_input_tokens"),
            reasoning_tokens: count("reasoning_tokens"),
            finished_at: now(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct UsageRecord {
    pub(crate) id: String,
    request_id: String,
    key_id: Option<String>,
    key: String,
    group: Option<String>,
    model: String,
    provider: String,
    upstream_model: String,
    endpoint: String,
    upstream_endpoint: String,
    outcome: String,
    status: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    cache_write_input_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    started_at: i64,
    finished_at: Option<i64>,
}

impl UsageStore {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn begin(&self, context: &LogContext) -> DatabaseResult<UsageAttempt> {
        let id = Str::ulid().to_string();
        let field = |key: &str| {
            context
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map_or(Value::Null, Into::into)
        };
        if !self
            .connection
            .table(TABLE)
            .insert(&[Record::from([
                ("id".into(), id.as_str().into()),
                ("request_id".into(), field("request_id")),
                ("key_id".into(), field("key_id")),
                ("key_name".into(), field("key")),
                ("group_name".into(), field("group")),
                ("model".into(), field("model")),
                ("provider".into(), field("provider")),
                ("upstream_model".into(), field("upstream_model")),
                ("endpoint".into(), field("endpoint")),
                ("upstream_endpoint".into(), field("upstream_endpoint")),
                ("outcome".into(), "pending".into()),
                ("started_at".into(), now().into()),
            ])])
            .await?
        {
            return Err(DatabaseError::InvalidQuery("AI 网关用量登记失败"));
        }
        Ok(UsageAttempt {
            store: self.clone(),
            id,
        })
    }

    pub(crate) async fn list(
        &self,
        request: Option<&str>,
        key: Option<&str>,
        before: Option<&str>,
        limit: u64,
    ) -> DatabaseResult<Vec<UsageRecord>> {
        let mut query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .select(COLUMNS)
            .order_by_desc("id")
            .limit(limit);
        if let Some(request) = request {
            query = query.r#where("request_id", "=", request);
        }
        if let Some(key) = key {
            query = query.r#where("key_id", "=", key);
        }
        if let Some(before) = before {
            query = query.r#where("id", "<", before);
        }
        query.get().await?.into_iter().map(decode).collect()
    }
}

impl UsageAttempt {
    pub(crate) async fn finish(&self, settlement: Settlement) -> DatabaseResult<()> {
        let changed = self
            .store
            .connection
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
            .where_null("finished_at")
            .update(Record::from([
                ("outcome".into(), settlement.outcome.into()),
                ("status".into(), nullable(settlement.status)),
                ("input_tokens".into(), nullable(settlement.input_tokens)),
                ("output_tokens".into(), nullable(settlement.output_tokens)),
                (
                    "cached_input_tokens".into(),
                    nullable(settlement.cached_input_tokens),
                ),
                (
                    "cache_write_input_tokens".into(),
                    nullable(settlement.cache_write_input_tokens),
                ),
                (
                    "reasoning_tokens".into(),
                    nullable(settlement.reasoning_tokens),
                ),
                ("finished_at".into(), settlement.finished_at.into()),
            ]))
            .await?;
        if changed == 0 {
            let query = self
                .store
                .connection
                .table(TABLE)
                .use_write_pdo()
                .select(["finished_at"])
                .r#where("id", "=", self.id.as_str());
            let confirmed = query
                .first()
                .await?
                .is_some_and(|row| matches!(row.get("finished_at"), Some(Value::Integer(_))));
            if !confirmed {
                return Err(DatabaseError::InvalidQuery("AI 网关用量结算记录不存在"));
            }
        }
        Ok(())
    }
}

fn decode(row: Row) -> DatabaseResult<UsageRecord> {
    let text = |column| super::managed_keys::text(&row, column);
    let integer = |column| super::managed_keys::integer(&row, column);
    let optional_text = |column| match row.get(column) {
        Some(Value::Text(value)) => Ok(Some(value.clone())),
        Some(Value::Null) => Ok(None),
        _ => Err(DatabaseError::InvalidScalar),
    };
    Ok(UsageRecord {
        id: text("id")?,
        request_id: text("request_id")?,
        key_id: optional_text("key_id")?,
        key: text("key_name")?,
        group: optional_text("group_name")?,
        model: text("model")?,
        provider: text("provider")?,
        upstream_model: text("upstream_model")?,
        endpoint: text("endpoint")?,
        upstream_endpoint: text("upstream_endpoint")?,
        outcome: text("outcome")?,
        status: integer("status")?,
        input_tokens: integer("input_tokens")?,
        output_tokens: integer("output_tokens")?,
        cached_input_tokens: integer("cached_input_tokens")?,
        cache_write_input_tokens: integer("cache_write_input_tokens")?,
        reasoning_tokens: integer("reasoning_tokens")?,
        started_at: integer("started_at")?.ok_or(DatabaseError::InvalidScalar)?,
        finished_at: integer("finished_at")?,
    })
}

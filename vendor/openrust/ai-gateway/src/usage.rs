use std::ops::Range;

use openrust::{
    database::{
        Connection, DatabaseError, DatabaseResult, Row, Value,
        query::{Builder, Record},
    },
    log::LogContext,
    support::Str,
};
use serde::Serialize;

use crate::{
    billing::ChargePlan,
    managed_keys::{now, nullable},
    output_quota::QuotaReservation,
};

const TABLE: &str = "ai_gateway_usage";
const COLUMNS: [&str; 23] = [
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
    "usage_state",
    "dispatch_started_at",
    "upstream_request_id",
    "reported_model",
];

pub(crate) const DAY: i64 = 86_400;
const DAY_START: &str = "started_at - (started_at % 86400)";
const ROLLUP: &str = "
    COUNT(DISTINCT request_id) AS requests, COUNT(*) AS attempts,
    COUNT(finished_at) AS settled_attempts, SUM(input_tokens) AS input_tokens,
    SUM(output_tokens) AS output_tokens, SUM(cached_input_tokens) AS cache_read_tokens,
    SUM(cache_write_input_tokens) AS cache_creation_tokens, SUM(reasoning_tokens) AS reasoning_tokens,
    SUM(CASE WHEN input_tokens IS NOT NULL THEN input_tokens + COALESCE(output_tokens, 0)
        + CASE WHEN upstream_endpoint = 'messages' THEN COALESCE(cached_input_tokens, 0)
        + COALESCE(cache_write_input_tokens, 0) ELSE 0 END END) AS total_tokens
";

pub(crate) enum UsageOwner<'a> {
    Managed(&'a str),
    Static(String),
}

impl UsageOwner<'_> {
    fn filter<'a>(&self, query: Builder<'a>) -> Builder<'a> {
        match self {
            Self::Managed(id) => query.r#where("key_id", "=", *id),
            Self::Static(scope) => {
                query
                    .where_null("key_id")
                    .r#where("static_key_scope", "=", scope.as_str())
            }
        }
    }
}

#[derive(Default, Serialize)]
pub(crate) struct UsageTotals {
    requests: u64,
    attempts: u64,
    settled_attempts: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct DailyUsage {
    date: String,
    #[serde(flatten)]
    totals: UsageTotals,
}

#[derive(Serialize)]
pub(crate) struct ModelUsage {
    pub(crate) model: String,
    #[serde(flatten)]
    totals: UsageTotals,
}

pub(crate) struct KeyUsageSummary {
    pub(crate) total: UsageTotals,
    pub(crate) daily: Vec<DailyUsage>,
    pub(crate) models: Vec<ModelUsage>,
}

#[derive(Clone)]
pub(crate) struct UsageStore {
    connection: Connection,
}

pub(crate) struct UsageAttempt {
    store: UsageStore,
    pub(crate) id: String,
    quota: Option<QuotaReservation>,
    charges: Option<ChargePlan>,
}

#[derive(Clone)]
pub(crate) struct Settlement {
    pub(crate) outcome: &'static str,
    pub(crate) state: &'static str,
    status: Option<i64>,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) cache_write_input_tokens: Option<i64>,
    pub(crate) reasoning_tokens: Option<i64>,
    upstream_request_id: Option<String>,
    pub(crate) reported_model: Option<String>,
    finished_at: i64,
    pub(crate) upstream_endpoint: String,
}

impl Settlement {
    pub(crate) fn from_context(
        outcome: &'static str,
        context: &LogContext,
        dispatch_started: bool,
    ) -> Self {
        let count = |key: &str| context.get(key).and_then(serde_json::Value::as_i64);
        let text = |key: &str| {
            context
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let input = count("input_tokens");
        let output = count("output_tokens");
        let cache_read = count("cached_input_tokens");
        let cache_written = count("cache_write_input_tokens");
        let reasoning = count("reasoning_tokens");
        let input_only = matches!(
            context
                .get("upstream_endpoint")
                .and_then(serde_json::Value::as_str),
            Some("embeddings" | "embedContent" | "batchEmbedContents")
        );
        let state = if !dispatch_started {
            "not_dispatched"
        } else if outcome == "completed" && input.is_some() && (input_only || output.is_some()) {
            "final"
        } else if [input, output, cache_read, cache_written, reasoning]
            .into_iter()
            .any(|counter| counter.is_some())
        {
            "partial"
        } else {
            "unknown"
        };
        Self {
            outcome,
            state,
            status: count("status"),
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: cache_read,
            cache_write_input_tokens: cache_written,
            reasoning_tokens: reasoning,
            upstream_request_id: text("upstream_request_id"),
            reported_model: text("reported_model"),
            upstream_endpoint: text("upstream_endpoint").unwrap_or_default(),
            finished_at: now(),
        }
    }

    fn observation(&self) -> Record {
        Record::from([
            ("usage_state".into(), self.state.into()),
            ("status".into(), nullable(self.status)),
            ("input_tokens".into(), nullable(self.input_tokens)),
            ("output_tokens".into(), nullable(self.output_tokens)),
            (
                "cached_input_tokens".into(),
                nullable(self.cached_input_tokens),
            ),
            (
                "cache_write_input_tokens".into(),
                nullable(self.cache_write_input_tokens),
            ),
            ("reasoning_tokens".into(), nullable(self.reasoning_tokens)),
            (
                "upstream_request_id".into(),
                self.upstream_request_id
                    .as_deref()
                    .map_or(Value::Null, Into::into),
            ),
            (
                "reported_model".into(),
                self.reported_model
                    .as_deref()
                    .map_or(Value::Null, Into::into),
            ),
        ])
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
    usage_state: String,
    dispatch_started_at: Option<i64>,
    upstream_request_id: Option<String>,
    reported_model: Option<String>,
}

impl UsageStore {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn charges(
        &self,
        records: &[UsageRecord],
    ) -> DatabaseResult<Vec<crate::billing::ChargeRecord>> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        self.connection
            .table(crate::billing::TABLE)
            .use_write_pdo()
            .where_in(
                "attempt_id",
                records
                    .iter()
                    .map(|record| Value::from(record.id.as_str()))
                    .collect::<Vec<_>>(),
            )
            .select(crate::billing::COLUMNS)
            .order_by_desc("attempt_id")
            .order_by("direction", "asc")
            .get()
            .await?
            .into_iter()
            .map(crate::billing::decode)
            .collect()
    }

    pub(crate) async fn begin(
        &self,
        context: &LogContext,
        quota: Option<QuotaReservation>,
        owner: UsageOwner<'_>,
        charges: Option<ChargePlan>,
    ) -> DatabaseResult<Option<UsageAttempt>> {
        let id = Str::ulid().to_string();
        let field = |key: &str| {
            context
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map_or(Value::Null, Into::into)
        };
        let mut record = Record::from([
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
            ("usage_state".into(), "not_dispatched".into()),
            ("started_at".into(), now().into()),
        ]);
        if let UsageOwner::Static(scope) = owner {
            record.insert("static_key_scope".into(), scope.into());
        }
        if let Some(reservation) = &quota {
            record.insert("output_quota_id".into(), reservation.id.as_str().into());
            record.insert("reserved_output_tokens".into(), reservation.amount.into());
        }
        if quota.is_some() || charges.is_some() {
            let reservation = quota.clone();
            let plan = charges.clone();
            let attempt = id.clone();
            let reserved = self
                .connection
                .transaction(
                    move |transaction| {
                        let reservation = reservation.clone();
                        let plan = plan.clone();
                        let attempt = attempt.clone();
                        let record = record.clone();
                        Box::pin(async move {
                            if let Some(reservation) = reservation
                                && !reservation.reserve(transaction).await?
                            {
                                return Ok(false);
                            }
                            insert_usage(transaction.table(TABLE), record).await?;
                            if let Some(plan) = plan {
                                plan.register(transaction, &attempt).await?;
                            }
                            Ok(true)
                        })
                    },
                    3,
                )
                .await?;
            if !reserved {
                return Ok(None);
            }
        } else {
            insert_usage(self.connection.table(TABLE), record).await?;
        }
        Ok(Some(UsageAttempt {
            store: self.clone(),
            id,
            quota,
            charges,
        }))
    }

    pub(crate) async fn key_summary(
        &self,
        owner: &UsageOwner<'_>,
        days: u8,
        model_window: Range<i64>,
    ) -> DatabaseResult<KeyUsageSummary> {
        let scope = || owner.filter(self.connection.table(TABLE).use_write_pdo());
        let today = now().div_euclid(DAY) * DAY;
        let start = today - i64::from(days - 1) * DAY;
        let (model_group, null_model) = match self.connection.get_driver_name() {
            "mysql" | "mariadb" => ("model, CAST(model AS BINARY)", "NULL"),
            "pgsql" => ("model, model COLLATE \"C\"", "NULL"),
            "sqlite" => ("model, CAST(model AS BLOB)", "NULL"),
            "sqlsrv" => (
                "model, CONVERT(VARBINARY(512), model)",
                "CAST(NULL AS NVARCHAR(255))",
            ),
            _ => ("model", "NULL"),
        };
        let mut recent = scope()
            .r#where("started_at", ">=", start)
            .r#where("started_at", "<", today + DAY)
            .select_raw(
                &format!("{DAY_START} AS day_start, {null_model} AS model, {ROLLUP}"),
                Vec::new(),
            )
            .group_by_raw(DAY_START, Vec::new());
        let mut models = scope()
            .r#where("started_at", ">=", model_window.start)
            .r#where("started_at", "<", model_window.end)
            .select_raw(&format!("NULL AS day_start, model, {ROLLUP}"), Vec::new())
            .group_by_raw(model_group, Vec::new());
        let rows = scope()
            .select_raw(
                &format!("NULL AS day_start, {null_model} AS model, {ROLLUP}"),
                Vec::new(),
            )
            .union_all(&mut recent)?
            .union_all(&mut models)?
            .get()
            .await?;
        let mut total = None;
        let mut buckets = std::collections::BTreeMap::new();
        let mut models = Vec::new();
        for row in rows {
            let totals = UsageTotals::from_row(&row)?;
            match row.get("model") {
                Some(Value::Text(model)) => models.push(ModelUsage {
                    model: model.clone(),
                    totals,
                }),
                Some(Value::Null) => {
                    if let Some(start) = super::managed_keys::integer(&row, "day_start")? {
                        buckets.insert(start, totals);
                    } else {
                        total = Some(totals);
                    }
                }
                _ => return Err(DatabaseError::InvalidScalar),
            }
        }
        models.sort_unstable_by(|left, right| left.model.cmp(&right.model));
        let mut daily = Vec::with_capacity(usize::from(days));
        for offset in 0..days {
            let day = start + i64::from(offset) * DAY;
            let date = openrust::support::facades::Date::create_from_timestamp_utc(day)
                .and_then(|date| date.format("Y-m-d"))
                .map_err(|_| DatabaseError::InvalidScalar)?;
            daily.push(DailyUsage {
                date,
                totals: buckets.remove(&day).unwrap_or_default(),
            });
        }
        Ok(KeyUsageSummary {
            total: total.ok_or(DatabaseError::InvalidScalar)?,
            daily,
            models,
        })
    }

    pub(crate) fn today(daily: &[DailyUsage]) -> Option<&UsageTotals> {
        daily.last().map(|day| &day.totals)
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
    pub(crate) async fn begin_dispatch(&self) -> DatabaseResult<()> {
        let changed = self
            .store
            .connection
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
            .where_null("finished_at")
            .where_null("dispatch_started_at")
            .update(Record::from([
                ("dispatch_started_at".into(), now().into()),
                ("usage_state".into(), "unknown".into()),
            ]))
            .await?;
        if changed == 1 {
            Ok(())
        } else {
            Err(DatabaseError::InvalidQuery("AI 网关出站尝试的状态无效"))
        }
    }

    pub(crate) async fn observe(&self, settlement: &Settlement) -> DatabaseResult<()> {
        self.store
            .connection
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
            .where_null("finished_at")
            .update(settlement.observation())
            .await?;
        Ok(())
    }

    pub(crate) async fn finish(&self, settlement: Settlement) -> DatabaseResult<()> {
        let changed = if self.quota.is_some() || self.charges.is_some() {
            let reservation = self.quota.clone();
            let plan = self.charges.clone();
            let id = self.id.clone();
            self.store
                .connection
                .transaction(
                    move |transaction| {
                        let reservation = reservation.clone();
                        let plan = plan.clone();
                        let id = id.clone();
                        let settlement = settlement.clone();
                        Box::pin(async move {
                            let changed =
                                finish_usage(transaction.table(TABLE), &id, &settlement).await?;
                            if changed == 1 {
                                if let Some(plan) = plan {
                                    plan.settle(transaction, &id, &settlement).await?;
                                }
                                if let Some(reservation) = reservation
                                    && settlement.outcome == "completed"
                                {
                                    reservation
                                        .settle(transaction, settlement.output_tokens)
                                        .await?;
                                }
                            }
                            Ok(changed)
                        })
                    },
                    3,
                )
                .await?
        } else {
            finish_usage(self.store.connection.table(TABLE), &self.id, &settlement).await?
        };
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

async fn insert_usage(query: Builder<'_>, record: Record) -> DatabaseResult<()> {
    if !query.insert(&[record]).await? {
        return Err(DatabaseError::InvalidQuery("AI 网关用量登记失败"));
    }
    Ok(())
}

async fn finish_usage(
    query: Builder<'_>,
    id: &str,
    settlement: &Settlement,
) -> DatabaseResult<u64> {
    let mut fields = settlement.observation();
    fields.insert("outcome".into(), settlement.outcome.into());
    fields.insert("finished_at".into(), settlement.finished_at.into());
    query
        .r#where("id", "=", id)
        .where_null("finished_at")
        .update(fields)
        .await
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
        usage_state: text("usage_state")?,
        dispatch_started_at: integer("dispatch_started_at")?,
        upstream_request_id: optional_text("upstream_request_id")?,
        reported_model: optional_text("reported_model")?,
    })
}

impl UsageTotals {
    fn from_row(row: &Row) -> DatabaseResult<Self> {
        fn number(row: &Row, column: &str) -> DatabaseResult<Option<u64>> {
            match row.get(column) {
                Some(Value::Unsigned(value)) => Ok(Some(*value)),
                Some(Value::Integer(value)) => u64::try_from(*value)
                    .map(Some)
                    .map_err(|_| DatabaseError::InvalidScalar),
                Some(Value::Text(value)) => value
                    .parse()
                    .map(Some)
                    .map_err(|_| DatabaseError::InvalidScalar),
                Some(Value::Null) => Ok(None),
                _ => Err(DatabaseError::InvalidScalar),
            }
        }

        let count = |column| number(row, column)?.ok_or(DatabaseError::InvalidScalar);
        Ok(Self {
            requests: count("requests")?,
            attempts: count("attempts")?,
            settled_attempts: count("settled_attempts")?,
            input_tokens: number(row, "input_tokens")?,
            output_tokens: number(row, "output_tokens")?,
            cache_read_tokens: number(row, "cache_read_tokens")?,
            cache_creation_tokens: number(row, "cache_creation_tokens")?,
            reasoning_tokens: number(row, "reasoning_tokens")?,
            total_tokens: number(row, "total_tokens")?,
        })
    }
}

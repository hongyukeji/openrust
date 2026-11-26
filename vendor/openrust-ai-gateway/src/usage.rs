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
use sha2::{Digest, Sha256};

use crate::{
    billing::{self, ChargePlan, ChargeRecord},
    budget::{self, Budget, BudgetBalance, BudgetReservations, BudgetScope},
    managed_keys::{integer, now, nullable, text},
    output_quota::{self, QuotaReservation},
    reconciliation,
};

const TABLE: &str = "ai_gateway_usage";
const COLUMNS: [&str; 26] = [
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
    "consumer_invocation",
    "invocation_scope",
    "model_turn",
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
    fn invocation_scope(&self, invocation: &str) -> String {
        let (kind, owner) = match self {
            Self::Managed(id) => ("managed", *id),
            Self::Static(scope) => ("static", scope.as_str()),
        };
        let digest = Sha256::new()
            .chain_update(b"openrust.ai-gateway.invocation\0")
            .chain_update(kind)
            .chain_update([0])
            .chain_update(owner)
            .chain_update([0])
            .chain_update(invocation)
            .finalize();
        format!("{digest:x}")
    }

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
    budget: BudgetReservations,
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
    pub(crate) reported_cost_usd: Option<billing::Amount>,
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
            reported_cost_usd: None,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    consumer_invocation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invocation_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_turn: Option<i64>,
}

impl UsageStore {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn budget_balance(
        &self,
        scope: BudgetScope<'_>,
        budget: &Budget,
    ) -> DatabaseResult<BudgetBalance> {
        budget.balance(&self.connection, scope).await
    }

    pub(crate) async fn reconcile_budget(
        &self,
        attempt: String,
        direction: billing::adjustments::Direction,
        reference: String,
        actor: String,
    ) -> DatabaseResult<budget::settlements::BudgetSettlement> {
        budget::settlements::reconcile(&self.connection, attempt, direction, reference, actor).await
    }

    pub(crate) async fn recover_budget(
        &self,
        scope: BudgetScope<'_>,
        age: i64,
        limit: u64,
    ) -> DatabaseResult<usize> {
        let cutoff = now().saturating_sub(age);
        let (owner_column, owner, required_budget) = match scope {
            BudgetScope::Group(group) => ("group_name", group, "budget_id"),
            BudgetScope::Account(account) => ("provider", account, "account_budget_id"),
        };
        let rows = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .select([
                "id",
                "group_name",
                "provider",
                "budget_id",
                "reserved_receivable_atoms",
                "account_budget_id",
                "reserved_cost_atoms",
                "output_quota_id",
                "reserved_output_tokens",
            ])
            .r#where(owner_column, "=", owner)
            .r#where("started_at", "<=", cutoff)
            .r#where("usage_state", "=", "not_dispatched")
            .where_null("dispatch_started_at")
            .where_null("finished_at")
            .where_not_null(required_budget)
            .order_by("id", "asc")
            .limit(limit)
            .get()
            .await?;
        let mut recovered = 0;
        for row in rows {
            // 数据库的默认排序规则可能把大小写不同的授权分组视为相等。
            if text(&row, owner_column)? != owner {
                continue;
            }
            let recovered_one = self
                .connection
                .transaction(
                    move |transaction| {
                        let row = row.clone();
                        Box::pin(async move {
                            let id = text(&row, "id")?;
                            // 与出站标记共用原子条件。已开始出站的尝试留给账务核对。
                            if transaction
                                .table(TABLE)
                                .r#where("id", "=", id.as_str())
                                .r#where("usage_state", "=", "not_dispatched")
                                .where_null("dispatch_started_at")
                                .where_null("finished_at")
                                .update(Record::from([
                                    ("outcome".into(), "expired_before_dispatch".into()),
                                    ("finished_at".into(), now().into()),
                                ]))
                                .await?
                                != 1
                            {
                                return Ok(false);
                            }
                            billing::close_undispatched(transaction, &id).await?;
                            if let Some(tokens) = integer(&row, "reserved_output_tokens")? {
                                output_quota::release_reservation(
                                    transaction,
                                    &text(&row, "output_quota_id")?,
                                    tokens,
                                )
                                .await?;
                            }
                            for (reservation_id, atoms) in [
                                ("budget_id", "reserved_receivable_atoms"),
                                ("account_budget_id", "reserved_cost_atoms"),
                            ] {
                                if let Some(reserved) = integer(&row, atoms)? {
                                    budget::release_reservation(
                                        transaction,
                                        &text(&row, reservation_id)?,
                                        reserved,
                                        0,
                                    )
                                    .await?;
                                }
                            }
                            Ok(true)
                        })
                    },
                    3,
                )
                .await?;
            recovered += usize::from(recovered_one);
        }
        Ok(recovered)
    }

    pub(crate) async fn import_cost_report(
        &self,
        report: crate::reconciliation::CostReport,
        actor: String,
    ) -> DatabaseResult<crate::reconciliation::ImportResult> {
        crate::reconciliation::import(&self.connection, report, actor).await
    }

    pub(crate) async fn apply_adjustment(
        &self,
        input: billing::adjustments::Input,
        actor: String,
    ) -> DatabaseResult<billing::adjustments::Applied> {
        billing::adjustments::post(&self.connection, input, actor).await
    }

    pub(crate) async fn cost_report(
        &self,
        id: String,
    ) -> DatabaseResult<Option<reconciliation::StoredReport>> {
        reconciliation::read(&self.connection, id).await
    }

    pub(crate) async fn begin(
        &self,
        context: &LogContext,
        quota: Option<QuotaReservation>,
        owner: UsageOwner<'_>,
        charges: Option<ChargePlan>,
        budget: BudgetReservations,
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
        if let Some(invocation) = context
            .get("consumer_invocation")
            .and_then(serde_json::Value::as_str)
        {
            record.insert("consumer_invocation".into(), invocation.into());
            record.insert(
                "invocation_scope".into(),
                owner.invocation_scope(invocation).into(),
            );
            record.insert(
                "model_turn".into(),
                nullable(
                    context
                        .get("model_turn")
                        .and_then(serde_json::Value::as_i64),
                ),
            );
        }
        if let UsageOwner::Static(scope) = owner {
            record.insert("static_key_scope".into(), scope.into());
        }
        if let Some(reservation) = &quota {
            record.insert("output_quota_id".into(), reservation.id.as_str().into());
            record.insert("reserved_output_tokens".into(), reservation.amount.into());
        }
        if let Some(reservation) = &budget.group {
            record.insert("budget_id".into(), reservation.id.as_str().into());
            record.insert("reserved_receivable_atoms".into(), reservation.atoms.into());
        }
        if let Some(reservation) = &budget.account {
            record.insert("account_budget_id".into(), reservation.id.as_str().into());
            record.insert("reserved_cost_atoms".into(), reservation.atoms.into());
        }
        if quota.is_some() || charges.is_some() || !budget.is_empty() {
            let reservation = quota.clone();
            let plan = charges.clone();
            let budget_reservation = budget.clone();
            let attempt = id.clone();
            let reserved = self
                .connection
                .transaction(
                    move |transaction| {
                        let reservation = reservation.clone();
                        let plan = plan.clone();
                        let budget = budget_reservation.clone();
                        let attempt = attempt.clone();
                        let record = record.clone();
                        Box::pin(async move {
                            if let Some(reservation) = reservation
                                && !reservation.reserve(transaction).await?
                            {
                                return Ok(false);
                            }
                            for budget in [budget.group, budget.account].into_iter().flatten() {
                                budget.reserve(transaction).await?;
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
            budget,
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
        invocation: (Option<&str>, Option<&str>),
        limit: u64,
        billing: bool,
    ) -> DatabaseResult<(Vec<UsageRecord>, Option<Vec<ChargeRecord>>)> {
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
        if let Some(id) = invocation.0 {
            query = query.r#where("consumer_invocation", "=", id);
        }
        if let Some(scope) = invocation.1 {
            query = query.r#where("invocation_scope", "=", scope);
        }
        if !billing {
            let usage = query
                .get()
                .await?
                .iter()
                .map(decode)
                .collect::<DatabaseResult<_>>()?;
            return Ok((usage, None));
        }
        self.billing_page(query).await
    }

    async fn billing_page(
        &self,
        mut page: Builder<'_>,
    ) -> DatabaseResult<(Vec<UsageRecord>, Option<Vec<ChargeRecord>>)> {
        // 先限定用量页，再在同一条查询中读取结算与证据。
        let query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .from_sub(&mut page, "usage_page")?
            .left_join(
                billing::TABLE,
                "usage_page.id",
                "=",
                "ai_gateway_charges.attempt_id",
            )
            .select(["usage_page.*"])
            .add_select(billing::COLUMNS);
        let rows = billing::adjustments::with_adjustments(reconciliation::with_evidence(
            query,
            &self.connection,
        )?)
        .order_by_desc("usage_page.id")
        .order_by("ai_gateway_charges.direction", "asc")
        .get()
        .await?;
        let mut actual = reconciliation::actual_costs(&rows)?;
        let mut usage = Vec::<UsageRecord>::new();
        let mut charges = Vec::<ChargeRecord>::new();
        for chunk in rows.chunk_by(|left, right| {
            left.get("id") == right.get("id") && left.get("direction") == right.get("direction")
        }) {
            let row = &chunk[0];
            let id = super::managed_keys::text(row, "id")?;
            if usage.last().is_none_or(|record| record.id != id) {
                usage.push(decode(row)?);
            }
            if matches!(row.get("attempt_id"), Some(Value::Null)) {
                continue;
            }
            let mut charge = billing::decode(row)?;
            if charge.direction == "cost" {
                charge.actual = actual.remove(&id);
            }
            charge.attach_adjustments(chunk)?;
            charges.push(charge);
        }
        Ok((usage, Some(charges)))
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
        let changed = if self.quota.is_some() || self.charges.is_some() || !self.budget.is_empty() {
            let reservation = self.quota.clone();
            let plan = self.charges.clone();
            let budget = self.budget.clone();
            let id = self.id.clone();
            self.store
                .connection
                .transaction(
                    move |transaction| {
                        let reservation = reservation.clone();
                        let plan = plan.clone();
                        let budget = budget.clone();
                        let id = id.clone();
                        let settlement = settlement.clone();
                        Box::pin(async move {
                            let changed =
                                finish_usage(transaction.table(TABLE), &id, &settlement).await?;
                            if changed == 1 {
                                if let Some(plan) = &plan {
                                    plan.settle(transaction, &id, &settlement).await?;
                                }
                                if let Some(reservation) = reservation
                                    && settlement.outcome == "completed"
                                {
                                    reservation
                                        .settle(transaction, settlement.output_tokens)
                                        .await?;
                                }
                                if let Some(plan) = plan {
                                    if let Some(group) = budget.group {
                                        group
                                            .settle(transaction, plan.receivable(&settlement))
                                            .await?;
                                    }
                                    if let Some(account) = budget.account
                                        && !budget::settlements::applied(
                                            transaction,
                                            &id,
                                            billing::adjustments::Direction::Cost,
                                        )
                                        .await?
                                    {
                                        account.settle(transaction, plan.cost(&settlement)).await?;
                                    }
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

fn decode(row: &Row) -> DatabaseResult<UsageRecord> {
    let text = |column| super::managed_keys::text(row, column);
    let integer = |column| super::managed_keys::integer(row, column);
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
        consumer_invocation: optional_text("consumer_invocation")?,
        invocation_scope: optional_text("invocation_scope")?,
        model_turn: integer("model_turn")?,
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

//! 投递状态、领取租约及单次尝试的数据库边界。
use aws_lc_rs::digest::{SHA256, digest};
use openrust::{
    database::{
        Connection, Row, Transaction, Value,
        query::{Builder, Expression, UpdateValue, ValueOperand},
    },
    support::Str,
};
use std::fmt::Write as _;

use crate::{
    Error, PreparedWebhook, Result,
    endpoints::{EndpointPermission, EndpointStore},
    idempotency::IdempotencyKind,
    limits::{self, DeliveryLimits},
    publication::MESSAGES,
    replay::ReplayRecord,
    snapshot::Snapshot,
};

pub(crate) const DELIVERIES: &str = "webhook_deliveries";
pub(crate) const ATTEMPTS: &str = "webhook_delivery_attempts";
pub(crate) const REPLAYS: &str = "webhook_delivery_replays";
const WAKE_COOLDOWN_SECONDS: u32 = 60;

pub(crate) enum WakeClaim {
    Ready,
    Retired,
    Unavailable,
}

#[derive(Clone)]
pub struct DeliveryStore {
    pub(crate) connection: Connection,
    scope: String,
    pub(crate) scope_key: String,
}

pub(crate) struct Claim {
    pub token: String,
    pub number: u32,
    pub timestamp: i64,
    pub deadline: i64,
    pub snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryProgress {
    Unavailable,
    Pending,
    Succeeded,
    Failed,
    Expired,
    Cancelled,
}

impl DeliveryProgress {
    pub(crate) fn is_final_failure(self) -> bool {
        matches!(self, Self::Failed | Self::Expired)
    }
}

pub(crate) struct AttemptResult {
    pub outcome: &'static str,
    pub status: Option<u16>,
    pub retry_after: Option<RetryAfter>,
}

pub(crate) enum RetryAfter {
    Seconds(u64),
    Timestamp(i64),
}

impl RetryAfter {
    fn earliest(&self, now: i64) -> i64 {
        match *self {
            Self::Seconds(seconds) => future_time(now, seconds),
            Self::Timestamp(timestamp) => timestamp,
        }
    }
}

impl AttemptResult {
    pub fn transient(outcome: &'static str) -> Self {
        Self {
            outcome,
            status: None,
            retry_after: Some(RetryAfter::Seconds(0)),
        }
    }

    pub fn failure(outcome: &'static str) -> Self {
        Self {
            outcome,
            status: None,
            retry_after: None,
        }
    }
}

impl DeliveryStore {
    pub fn new(connection: Connection, scope: &str) -> Result<Self> {
        check_text("scope", scope)?;
        Ok(Self {
            connection,
            scope: scope.into(),
            scope_key: hash(scope.as_bytes()),
        })
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn endpoints(&self) -> EndpointStore {
        EndpointStore::new(self.connection.clone(), &self.scope_key)
    }

    /// 已绑定的历史记录在登记时需与此范围的当前版本一致。
    pub(crate) async fn check_endpoint(&self, snapshot: &Snapshot) -> Result<()> {
        match self.endpoints().permission(snapshot).await? {
            EndpointPermission::Active => Ok(()),
            EndpointPermission::Paused => Err(Error::EndpointPaused),
            EndpointPermission::Revoked => Err(Error::EndpointRevoked),
        }
    }

    pub(super) fn worker_key(&self) -> String {
        let identity = (
            self.connection.get_driver_name(),
            self.connection.get_name(),
            self.connection.get_database_name(),
            &self.scope_key,
        );
        hash(&serde_json::to_vec(&identity).expect("编码数据库与投递范围标识"))
    }

    pub async fn submit(
        &self,
        key: &str,
        webhook: &PreparedWebhook,
        lifetime: u32,
    ) -> Result<String> {
        let mut tx = self.connection.begin_transaction().await?;
        let result = self
            .submit_in_transaction(&mut tx, key, webhook, lifetime)
            .await;
        settle(tx, result).await
    }

    /// 同库业务事务可以登记发送意图，发送入口只会领取已提交的记录。
    pub async fn submit_in_transaction(
        &self,
        tx: &mut Transaction,
        key: &str,
        webhook: &PreparedWebhook,
        lifetime: u32,
    ) -> Result<String> {
        if !tx.belongs_to(&self.connection) || tx.transaction_level() == 0 {
            return Err(Error::InvalidConfig("delivery.transaction"));
        }
        check_text("idempotency_key", key)?;
        if !(1..=2_592_000).contains(&lifetime) {
            return Err(Error::InvalidConfig("delivery.lifetime"));
        }
        let snapshot = webhook.freeze(lifetime)?;
        if self.endpoints().permission_in(&snapshot, tx).await? == EndpointPermission::Revoked {
            return Err(Error::EndpointRevoked);
        }
        let legacy_id = hash(format!("{}:{key}", self.scope_key).as_bytes());
        let parent = tx.transaction_level();
        tx.begin_transaction().await?;
        let result = self.persist_submission(tx, &legacy_id, &snapshot).await;
        match result {
            Ok(id) => {
                tx.commit().await?;
                Ok(id)
            }
            Err(error) => {
                tx.roll_back(Some(parent)).await?;
                Err(error)
            }
        }
    }

    async fn persist_submission(
        &self,
        tx: &mut Transaction,
        legacy_id: &str,
        snapshot: &Snapshot,
    ) -> Result<String> {
        let binding = self
            .bind_idempotency_key(tx, IdempotencyKind::Delivery, legacy_id)
            .await?;
        let id = binding.id;
        if binding.new_record {
            if self.insert_delivery(tx, &id, snapshot).await?.0 != 1 {
                return Err(Error::IdempotencyConflict);
            }
        } else {
            let row = self
                .owned_row(tx.table(DELIVERIES), &id)
                .select(["digest"])
                .shared_lock()
                .first()
                .await?
                .ok_or(Error::InvalidRecord)?;
            if text(&row, "digest")? != hash(snapshot.encode()?.as_bytes()) {
                return Err(Error::IdempotencyConflict);
            }
        }
        Ok(id)
    }

    pub(crate) async fn insert_delivery(
        &self,
        tx: &mut Transaction,
        id: &str,
        snapshot: &Snapshot,
    ) -> Result<(u64, String)> {
        let bytes = snapshot.encode()?;
        let fingerprint = hash(bytes.as_bytes());
        let now = self.time_in(tx).await?;
        let inserted = tx
            .table(DELIVERIES)
            .insert_or_ignore(&[[
                ("id".into(), id.into()),
                ("scope_key".into(), self.scope_key.as_str().into()),
                ("snapshot".into(), bytes.into()),
                ("digest".into(), fingerprint.as_str().into()),
                ("state".into(), "pending".into()),
                ("maximum_tries".into(), i64::from(snapshot.tries).into()),
                ("timeout_seconds".into(), i64::from(snapshot.timeout).into()),
                ("available_at".into(), now.into()),
                (
                    "deadline".into(),
                    (now + i64::from(snapshot.lifetime)).into(),
                ),
            ]
            .into_iter()
            .collect()])
            .await?;
        Ok((inserted, fingerprint))
    }

    pub(crate) async fn replay_source(&self, id: &str) -> Result<(Snapshot, String)> {
        check_id(id)?;
        let row = self
            .owned_row(self.connection.table(DELIVERIES), id)
            .use_write_pdo()
            .where_null("payload_pruned_at")
            .select(["state", "snapshot", "digest"])
            .first()
            .await?
            .ok_or(Error::ReplayUnavailable)?;
        let snapshot = self
            .hydrate_message(
                decode(&row)?,
                self.connection.table(MESSAGES).use_write_pdo(),
            )
            .await?;
        if snapshot.replay {
            return Err(Error::ReplayUnavailable);
        }
        Ok((snapshot, text(&row, "state")?.to_owned()))
    }

    pub(crate) async fn replay_generation(&self, original_id: &str) -> Result<u32> {
        let previous = self
            .connection
            .table(REPLAYS)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("original_id", "=", original_id)
            .select(["delivery_id", "generation"])
            .use_write_pdo()
            .order_by("generation", "desc")
            .first()
            .await?;
        let Some(previous) = previous else {
            return Ok(1);
        };
        let delivery = text(&previous, "delivery_id")?;
        let state = self
            .owned_row(self.connection.table(DELIVERIES), delivery)
            .use_write_pdo()
            .select(["state"])
            .first()
            .await?
            .ok_or(Error::InvalidRecord)?;
        if !matches!(
            text(&state, "state")?,
            "failed" | "expired" | "succeeded" | "cancelled"
        ) {
            return Err(Error::ReplayUnavailable);
        }
        let generation = number(&previous, "generation")?;
        if !(1..10).contains(&generation) {
            return Err(Error::ReplayUnavailable);
        }
        u32::try_from(generation + 1).map_err(|_| Error::InvalidRecord)
    }

    pub(crate) async fn persist_replay(
        &self,
        id: &str,
        record: &ReplayRecord,
        snapshot: &Snapshot,
    ) -> Result<()> {
        let limit = limits::current()?.replays_per_scope;
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            let (inserted, _) = self.insert_delivery(&mut tx, id, snapshot).await?;
            if inserted != 1 {
                return Err(Error::ReplayConflict);
            }
            // SQLite 已先写入新投递，其他后端在这里锁定原始投递再检查留存。
            if self
                .owned_row(tx.table(DELIVERIES), &record.original_id)
                .where_null("payload_pruned_at")
                .r#where("state", "=", record.source_state.as_str())
                .select(["id"])
                .lock_for_update()
                .first()
                .await?
                .is_none()
            {
                return Err(Error::ReplayUnavailable);
            }
            let now = self.time_in(&mut tx).await?;
            if let Some(limit) = limit {
                limits::consume(&mut tx, &self.scope_key, "replay", "", limit, now).await?;
            }
            tx.table(REPLAYS)
                .insert(&[[
                    ("delivery_id".into(), id.into()),
                    ("original_id".into(), record.original_id.as_str().into()),
                    ("scope_key".into(), self.scope_key.as_str().into()),
                    ("generation".into(), i64::from(record.generation).into()),
                    ("actor".into(), record.actor.as_str().into()),
                    ("reason".into(), record.reason.as_str().into()),
                    ("source_state".into(), record.source_state.as_str().into()),
                    (
                        "original_credential".into(),
                        record
                            .original_credential
                            .as_deref()
                            .map_or(Value::Null, Into::into),
                    ),
                    (
                        "replay_credential".into(),
                        snapshot
                            .credential
                            .as_deref()
                            .map_or(Value::Null, Into::into),
                    ),
                    ("created_at".into(), now.into()),
                ]
                .into_iter()
                .collect()])
                .await?;
            Ok(())
        }
        .await;
        settle(tx, result).await
    }

    pub(crate) async fn replay_record(&self, id: &str) -> Result<ReplayRecord> {
        let row = self
            .connection
            .table(REPLAYS)
            .r#where("delivery_id", "=", id)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .use_write_pdo()
            .first()
            .await?
            .ok_or(Error::ReplayDenied)?;
        let original_credential = match row.get("original_credential") {
            Some(Value::Text(value)) => Some(value.clone()),
            Some(Value::Null) => None,
            _ => return Err(Error::InvalidRecord),
        };
        Ok(ReplayRecord {
            original_id: text(&row, "original_id")?.into(),
            generation: u32::try_from(number(&row, "generation")?)
                .map_err(|_| Error::InvalidRecord)?,
            actor: text(&row, "actor")?.into(),
            reason: text(&row, "reason")?.into(),
            source_state: text(&row, "source_state")?.into(),
            original_credential,
        })
    }

    pub(crate) async fn snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        check_id(id)?;
        let row = self
            .owned_row(self.connection.table(DELIVERIES), id)
            .use_write_pdo()
            .select(["snapshot", "digest"])
            .first()
            .await?;
        let Some(row) = row else { return Ok(None) };
        let snapshot = self
            .hydrate_message(
                decode(&row)?,
                self.connection.table(MESSAGES).use_write_pdo(),
            )
            .await?;
        Ok(Some(snapshot))
    }

    pub(crate) async fn due_ids(&self, limit: u32) -> Result<Vec<String>> {
        self.bounded_ids(self.reclaimable(self.connection.table(DELIVERIES)), limit)
            .await
    }

    pub(crate) async fn wake_ids(&self, limit: u32) -> Result<Vec<String>> {
        let clock = self.clock();
        let candidates = self
            .reclaimable(self.connection.table(DELIVERIES))
            .where_nested(|query| {
                query
                    .where_null("next_wake_at")
                    .or_where_raw(&format!("next_wake_at <= {clock}"), vec![])
                    .or_where_raw(&format!("deadline <= {clock}"), vec![])
                    .or_where_raw("attempts >= maximum_tries", vec![])
            });
        self.bounded_ids(candidates, limit).await
    }

    async fn bounded_ids(&self, candidates: Builder<'_>, limit: u32) -> Result<Vec<String>> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("delivery.limit"));
        }
        let rows = candidates
            .r#where("scope_key", "=", self.scope_key.as_str())
            .use_write_pdo()
            .select(["id"])
            .order_by("available_at", "asc")
            .order_by("id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?;
        rows.iter()
            .map(|row| Ok(text(row, "id")?.to_owned()))
            .collect()
    }

    pub(crate) async fn claim_wake(&self, id: &str) -> Result<WakeClaim> {
        check_id(id)?;
        let clock = self.clock();
        let candidates = self
            .reclaimable(self.owned_row(self.connection.table(DELIVERIES), id))
            .where_nested(|query| {
                query
                    .where_null("next_wake_at")
                    .or_where_raw(&format!("next_wake_at <= {clock}"), vec![])
            })
            .where_raw(&format!("deadline > {clock}"), vec![])
            .where_column("attempts", "<", "maximum_tries");
        if self.mark_wake(candidates).await? {
            return Ok(WakeClaim::Ready);
        }
        let mut tx = self.connection.begin_transaction().await?;
        let result = self.retire_unavailable(&mut tx, id).await;
        Ok(if settle(tx, result).await? {
            WakeClaim::Retired
        } else {
            WakeClaim::Unavailable
        })
    }

    pub(crate) async fn defer_recovery(&self, id: &str) -> Result<()> {
        let candidates = self.reclaimable(self.owned_row(self.connection.table(DELIVERIES), id));
        self.mark_wake(candidates).await?;
        Ok(())
    }

    async fn mark_wake(&self, candidates: Builder<'_>) -> Result<bool> {
        let clock = self.clock();
        let value = Expression::new(format!("{clock} + {WAKE_COOLDOWN_SECONDS}"));
        let changed = candidates
            .update_with([assignment("next_wake_at", value)].into_iter().collect())
            .await?;
        Ok(changed == 1)
    }

    pub(crate) async fn claim<T>(
        &self,
        id: &str,
        resolve: impl FnOnce(&Snapshot) -> Result<T> + Send,
    ) -> Result<Option<(Claim, T)>> {
        check_id(id)?;
        let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let clock = self.clock();
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            let changed = self
                .reclaimable(self.owned_row(tx.table(DELIVERIES), id))
                .where_raw(&format!("deadline > {clock}"), vec![])
                .where_column("attempts", "<", "maximum_tries")
                .update_with(
                    [
                        assignment("state", Value::from("running")),
                        assignment("owner", Value::from(token.clone())),
                        assignment("attempts", Expression::new("attempts + 1")),
                        assignment("next_wake_at", Value::Null),
                        assignment(
                            "lease_until",
                            Expression::new(format!("{clock} + timeout_seconds + 5")),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if changed != 1 {
                self.retire_unavailable(&mut tx, id).await?;
                return Ok(None);
            }
            let row = self
                .owned_row(tx.table(DELIVERIES), id)
                .select(["snapshot", "digest", "attempts", "deadline"])
                .first()
                .await?
                .ok_or(Error::InvalidRecord)?;
            let snapshot = self
                .hydrate_message(decode(&row)?, tx.table(MESSAGES))
                .await?;
            if self.endpoints().permission_in(&snapshot, &mut tx).await?
                == EndpointPermission::Paused
            {
                return Err(Error::EndpointPaused);
            }
            let handler = resolve(&snapshot)?;
            let attempt_number = number(&row, "attempts")?;
            if attempt_number > 1 {
                self.mark_unknown(&mut tx, id, attempt_number - 1).await?;
            }
            let timestamp = self.time_in(&mut tx).await?;
            tx.table(ATTEMPTS)
                .insert(&[[
                    ("id".into(), token.as_str().into()),
                    ("delivery_id".into(), id.into()),
                    ("number".into(), attempt_number.into()),
                    ("outcome".into(), "running".into()),
                    ("started_at".into(), timestamp.into()),
                ]
                .into_iter()
                .collect()])
                .await?;
            Ok(Some((
                Claim {
                    token,
                    number: u32::try_from(attempt_number).map_err(|_| Error::InvalidRecord)?,
                    timestamp,
                    deadline: number(&row, "deadline")?,
                    snapshot,
                },
                handler,
            )))
        }
        .await;
        settle(tx, result).await
    }

    pub(crate) async fn reserve_send(
        &self,
        id: &str,
        claim: &Claim,
        limits: DeliveryLimits,
    ) -> Result<()> {
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            let now = self.time_in(&mut tx).await?;
            if let Some(limit) = limits.sends_per_scope {
                limits::consume(&mut tx, &self.scope_key, "scope", "", limit, now).await?;
            }
            if let Some((limit, endpoint)) = limits
                .sends_per_endpoint
                .zip(claim.snapshot.endpoint.as_ref())
            {
                limits::consume(
                    &mut tx,
                    &self.scope_key,
                    "endpoint",
                    &endpoint.id,
                    limit,
                    now,
                )
                .await?;
            }
            let clock = self.clock();
            let owned = self
                .owned_row(tx.table(DELIVERIES), id)
                .r#where("state", "=", "running")
                .r#where("owner", "=", claim.token.as_str())
                .where_raw(
                    &format!("lease_until > {clock} and deadline > {clock}"),
                    vec![],
                )
                .select(["id"])
                .lock_for_update()
                .first()
                .await?;
            if owned.is_none() {
                return Err(Error::LeaseLost);
            }
            Ok(())
        }
        .await;
        settle(tx, result).await
    }

    /// 尚未开始网络传输时归还尝试额度，并按需要持久化下次可领取时间。
    pub(crate) async fn defer_claim(
        &self,
        id: &str,
        claim: &Claim,
        retry_at: Option<i64>,
    ) -> Result<()> {
        let mut updates = vec![
            assignment("state", Value::from("pending")),
            assignment("owner", Value::Null),
            assignment("lease_until", Value::Null),
            assignment("next_wake_at", Value::Null),
            assignment("attempts", Value::from(i64::from(claim.number - 1))),
        ];
        if let Some(retry_at) = retry_at {
            updates.push(assignment(
                "available_at",
                Value::from(retry_at.min(claim.deadline)),
            ));
        }
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            let changed = self
                .claimed_row(tx.table(DELIVERIES), id, claim)
                .update_with(updates.into_iter().collect())
                .await?;
            if changed != 1 {
                return Err(Error::LeaseLost);
            }
            let deleted = tx
                .table(ATTEMPTS)
                .r#where("id", "=", claim.token.as_str())
                .where_null("finished_at")
                .delete()
                .await?;
            if deleted != 1 {
                return Err(Error::LeaseLost);
            }
            Ok(())
        }
        .await;
        settle(tx, result).await
    }

    pub(crate) async fn finish(
        &self,
        id: &str,
        claim: &Claim,
        attempt: AttemptResult,
        backoff: Option<u64>,
    ) -> Result<DeliveryProgress> {
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            // SQLite 的延迟事务先取得写事务，再读取端点许可，避免并发写入后升级旧快照。
            if claim.snapshot.endpoint.is_some() && self.connection.get_driver_name() == "sqlite" {
                let owned = self
                    .claimed_row(tx.table(DELIVERIES), id, claim)
                    .update(
                        [("owner".into(), claim.token.as_str().into())]
                            .into_iter()
                            .collect(),
                    )
                    .await?;
                if owned != 1 {
                    return Err(Error::LeaseLost);
                }
            }
            let now = self.time_in(&mut tx).await?;
            let next = attempt
                .retry_after
                .zip(backoff)
                .filter(|_| claim.number < claim.snapshot.tries)
                .map(|(retry_after, delay)| retry_after.earliest(now).max(future_time(now, delay)))
                .filter(|next| *next < claim.deadline);
            let revoked = self
                .endpoints()
                .permission_for_finish(&claim.snapshot, &mut tx)
                .await?
                == EndpointPermission::Revoked;
            let (state, progress) = if attempt.outcome == "cancelled" || revoked {
                ("cancelled", DeliveryProgress::Cancelled)
            } else if now >= claim.deadline {
                ("expired", DeliveryProgress::Expired)
            } else if attempt.outcome == "succeeded" {
                ("succeeded", DeliveryProgress::Succeeded)
            } else if next.is_some() {
                ("pending", DeliveryProgress::Pending)
            } else {
                ("failed", DeliveryProgress::Failed)
            };
            let changed = self
                .claimed_row(tx.table(DELIVERIES), id, claim)
                .update_with(
                    [
                        assignment("state", Value::from(state)),
                        assignment("owner", Value::Null),
                        assignment("lease_until", Value::Null),
                        assignment("available_at", Value::from(next.unwrap_or(now))),
                        assignment("next_wake_at", Value::Null),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if changed != 1 {
                return Err(Error::LeaseLost);
            }
            let written = tx
                .table(ATTEMPTS)
                .r#where("id", "=", claim.token.as_str())
                .where_null("finished_at")
                .update_with(
                    [
                        assignment("outcome", Value::from(attempt.outcome)),
                        assignment(
                            "http_status",
                            attempt
                                .status
                                .map_or(Value::Null, |code| i64::from(code).into()),
                        ),
                        assignment("finished_at", Value::from(now)),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if written != 1 {
                return Err(Error::LeaseLost);
            }
            Ok(progress)
        }
        .await;
        settle(tx, result).await
    }

    async fn retire_unavailable(&self, tx: &mut Transaction, id: &str) -> Result<bool> {
        let clock = self.clock();
        let updated = self
            .reclaimable(self.owned_row(tx.table(DELIVERIES), id))
            .where_nested(|query| {
                query
                    .where_raw(&format!("deadline <= {clock}"), vec![])
                    .or_where_raw("attempts >= maximum_tries", vec![])
            })
            .update_with(
                [
                    assignment(
                        "state",
                        Expression::new(format!(
                            "case when deadline <= {clock} then 'expired' else 'failed' end"
                        )),
                    ),
                    assignment("owner", Value::Null),
                    assignment("lease_until", Value::Null),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        if updated == 1 {
            let row = self
                .owned_row(tx.table(DELIVERIES), id)
                .select(["attempts"])
                .first()
                .await?
                .ok_or(Error::InvalidRecord)?;
            let attempt = number(&row, "attempts")?;
            if attempt > 0 {
                self.mark_unknown(tx, id, attempt).await?;
            }
        }
        Ok(updated == 1)
    }

    async fn mark_unknown(&self, tx: &mut Transaction, id: &str, attempt: i64) -> Result<()> {
        tx.table(ATTEMPTS)
            .r#where("delivery_id", "=", id)
            .r#where("number", "=", attempt)
            .where_null("finished_at")
            .update_with(
                [
                    assignment("outcome", Value::from("unknown")),
                    assignment("finished_at", Expression::new(self.clock())),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        Ok(())
    }

    fn claimed_row<'a>(&self, builder: Builder<'a>, id: &str, claim: &Claim) -> Builder<'a> {
        let clock = self.clock();
        self.owned_row(builder, id)
            .r#where("state", "=", "running")
            .r#where("owner", "=", claim.token.as_str())
            .where_raw(&format!("lease_until > {clock}"), vec![])
    }

    fn owned_row<'a>(&self, builder: Builder<'a>, id: &str) -> Builder<'a> {
        builder
            .r#where("id", "=", id)
            .r#where("scope_key", "=", self.scope_key.as_str())
    }

    fn reclaimable<'a>(&self, builder: Builder<'a>) -> Builder<'a> {
        let clock = self.clock();
        builder.where_nested(|query| {
            query
                .where_nested(|pending| {
                    pending.r#where("state", "=", "pending").where_raw(
                        &format!("(available_at <= {clock} or deadline <= {clock})"),
                        vec![],
                    )
                })
                .or_where_nested(|running| {
                    running
                        .r#where("state", "=", "running")
                        .where_raw(&format!("lease_until <= {clock}"), vec![])
                })
        })
    }

    pub(crate) fn clock(&self) -> &'static str {
        match self.connection.get_driver_name() {
            "sqlite" => "unixepoch()",
            "pgsql" => "floor(extract(epoch from statement_timestamp()))::bigint",
            "sqlsrv" => "datediff_big(second, '19700101', sysutcdatetime())",
            _ => "unix_timestamp()",
        }
    }

    pub(crate) async fn time_in(&self, tx: &mut Transaction) -> Result<i64> {
        let row = tx
            .select_one(&format!("select {} as now", self.clock()), &[])
            .await?
            .ok_or(Error::InvalidRecord)?;
        number(&row, "now")
    }
}

pub(crate) async fn settle<T>(mut tx: Transaction, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            tx.roll_back(None).await?;
            Err(error)
        }
    }
}

fn assignment(
    name: &str,
    value: impl Into<ValueOperand>,
) -> (String, UpdateValue<'static, 'static>) {
    (name.into(), UpdateValue::Operand(value.into()))
}

fn decode(row: &Row) -> Result<Snapshot> {
    let snapshot = text(row, "snapshot")?;
    let expected = text(row, "digest")?;
    if hash(snapshot.as_bytes()) != expected {
        return Err(Error::InvalidRecord);
    }
    Snapshot::decode(snapshot)
}

fn future_time(now: i64, seconds: u64) -> i64 {
    now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX))
}

pub(crate) fn number(row: &Row, key: &str) -> Result<i64> {
    match row.get(key) {
        Some(Value::Integer(value)) => Ok(*value),
        Some(Value::Unsigned(value)) => i64::try_from(*value).map_err(|_| Error::InvalidRecord),
        _ => Err(Error::InvalidRecord),
    }
}

pub(crate) fn text<'a>(row: &'a Row, key: &str) -> Result<&'a str> {
    match row.get(key) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(Error::InvalidRecord),
    }
}

pub(crate) fn replay_id(original: &str, generation: u32) -> String {
    hash(format!("outbound-replay:{original}:{generation}").as_bytes())
}

pub(crate) fn hash(value: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest(&SHA256, value).as_ref() {
        write!(output, "{byte:02x}").expect("写入字符串");
    }
    output
}

pub(crate) fn check_text(field: &'static str, text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > 128 || text.chars().any(char::is_control) {
        return Err(Error::InvalidConfig(field));
    }
    Ok(())
}

pub(crate) fn check_id(id: &str) -> Result<()> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(Error::InvalidRecord);
    }
    Ok(())
}

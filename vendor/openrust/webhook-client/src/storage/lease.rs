use super::{DatabaseWebhookStore, attempts, scope_key};
use crate::{Error, Result, jobs::DeferredProcessingPolicy, models::WebhookCall};
use openrust::{
    database::{
        Transaction, Value,
        query::{Builder, Expression, UpdateValue, ValueOperand},
    },
    http::HttpFuture,
    support::Str,
};
use std::collections::BTreeMap;

pub(crate) enum ProcessingClaim {
    Owned(String),
    Busy,
    Settled,
    Scheduled,
}

pub(crate) enum ProcessingOutcome {
    Complete,
    Release,
    Failed,
}

impl DatabaseWebhookStore {
    fn clock(&self) -> &'static str {
        // 与原生数据库队列使用相同的数据库时钟。
        match self.connection.get_driver_name() {
            "sqlite" => "unixepoch()",
            "pgsql" => "floor(extract(epoch from statement_timestamp()))::bigint",
            "sqlsrv" => "datediff_big(second, '19700101', sysutcdatetime())",
            _ => "unix_timestamp()",
        }
    }

    fn processing_row<'a>(&self, builder: Builder<'a>, call: &WebhookCall) -> Result<Builder<'a>> {
        if call.scope() != self.scope {
            return Err(Error::RecordNotFound);
        }
        Ok(builder
            .r#where("scope_key", "=", scope_key(&self.scope))
            .r#where("id", "=", call.id())
            .where_null("processed_at")
            .where_null("processing_stopped_at"))
    }

    fn owned_processing_row<'a>(
        &self,
        builder: Builder<'a>,
        call: &WebhookCall,
        token: &str,
    ) -> Result<Builder<'a>> {
        Ok(self
            .processing_row(builder, call)?
            .r#where("processing_token", "=", token)
            .where_raw(&format!("processing_until > {}", self.clock()), vec![]))
    }

    pub(crate) async fn claim_processing(
        &self,
        call: &WebhookCall,
        seconds: u32,
    ) -> Result<ProcessingClaim> {
        let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let clock = self.clock();
        let mut transaction = self.connection.begin_transaction().await?;
        let result = async {
            let changed = self
                .processing_row(transaction.table("webhook_calls"), call)?
                .where_nested(|query| {
                    query
                        .where_null("processing_until")
                        .or_where_raw(&format!("processing_until <= {clock}"), vec![])
                })
                .where_nested(|query| {
                    query
                        .where_null("processing_next_at")
                        .or_where_raw(&format!("processing_next_at <= {clock}"), vec![])
                })
                .where_nested(|query| {
                    query
                        .where_null("processing_deadline")
                        .or_where_raw(&format!("processing_deadline > {clock}"), vec![])
                })
                .update_with(
                    [
                        assignment("processing_next_at", Value::Null),
                        assignment("processing_token", Value::from(token.clone())),
                        assignment(
                            "processing_until",
                            Expression::new(format!("{clock} + ?"))
                                .with_bindings(vec![i64::from(seconds).into()]),
                        ),
                        assignment(
                            "processing_generation",
                            Expression::new("processing_generation + 1"),
                        ),
                        assignment("exception", Value::Null),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if changed == 1 {
                attempts::begin(&mut transaction, call.id(), &token, clock).await?;
                return Ok(Some(ProcessingClaim::Owned(token)));
            }
            let expired = self
                .processing_row(transaction.table("webhook_calls"), call)?
                .where_raw(&format!("processing_deadline <= {clock}"), vec![])
                .where_nested(|query| {
                    query
                        .where_null("processing_until")
                        .or_where_raw(&format!("processing_until <= {clock}"), vec![])
                })
                .update_with(stopped_assignments(clock))
                .await?;
            if expired == 1 {
                attempts::expire(&mut transaction, call.id(), clock).await?;
                return Ok(Some(ProcessingClaim::Settled));
            }
            Ok(None)
        }
        .await;
        if let Some(claim) = settle_transaction(transaction, result).await? {
            return Ok(claim);
        }
        let row = self
            .connection
            .table("webhook_calls")
            .use_write_pdo()
            .select(["scope", "processed_at", "processing_stopped_at"])
            .select_raw(
                &format!("case when processing_next_at > {clock} then 1 else 0 end as deferred"),
                vec![],
            )
            .r#where("scope_key", "=", scope_key(&self.scope))
            .r#where("id", "=", call.id())
            .first()
            .await?
            .ok_or(Error::RecordNotFound)?;
        if !matches!(row.get("scope"), Some(Value::Text(scope)) if scope == &self.scope) {
            return Err(Error::RecordNotFound);
        }
        for field in ["processed_at", "processing_stopped_at"] {
            match row.get(field) {
                Some(Value::Null) => {}
                Some(Value::Integer(_) | Value::Unsigned(_)) => {
                    return Ok(ProcessingClaim::Settled);
                }
                _ => return Err(Error::InvalidRecord),
            }
        }
        match row.get("deferred") {
            Some(Value::Integer(1) | Value::Unsigned(1)) => Ok(ProcessingClaim::Scheduled),
            Some(Value::Integer(0) | Value::Unsigned(0)) => Ok(ProcessingClaim::Busy),
            _ => Err(Error::InvalidRecord),
        }
    }

    pub(crate) async fn finish_processing(
        &self,
        call: &WebhookCall,
        token: &str,
        outcome: ProcessingOutcome,
    ) -> Result<()> {
        let clock = self.clock();
        let processed_at = match outcome {
            ProcessingOutcome::Complete => ValueOperand::Expression(Expression::new(clock)),
            _ => ValueOperand::Value(Value::Null),
        };
        let exception = match outcome {
            ProcessingOutcome::Failed => {
                Value::Text(crate::jobs::processing_exception().to_string())
            }
            _ => Value::Null,
        };
        let attempt_outcome = match outcome {
            ProcessingOutcome::Complete => "completed",
            ProcessingOutcome::Release => "released",
            ProcessingOutcome::Failed => "failed",
        };
        self.finish_owned(
            call,
            token,
            [
                assignment("processing_token", Value::Null),
                assignment("processing_until", Value::Null),
                assignment("processed_at", processed_at),
                assignment("exception", exception),
            ]
            .into_iter()
            .collect(),
            attempt_outcome,
        )
        .await
    }

    pub(crate) async fn defer_processing(
        &self,
        call: &WebhookCall,
        token: &str,
        delay: u32,
        policy: DeferredProcessingPolicy,
        enqueue: impl for<'a> FnOnce(&'a mut Transaction) -> HttpFuture<'a, Result<()>> + Send,
    ) -> Result<bool> {
        let clock = self.clock();
        let mut transaction = self.connection.begin_transaction().await?;
        let result = async {
            let mut changed = 0;
            if delay != 0 && delay < policy.max_age_seconds {
                changed = self
                    .owned_processing_row(transaction.table("webhook_calls"), call, token)?
                    .r#where("processing_deferrals", "<", i64::from(policy.max_deferrals))
                    .where_nested(|query| {
                        query.where_null("processing_deadline").or_where_raw(
                            &format!("processing_deadline > {clock} + ?"),
                            vec![i64::from(delay).into()],
                        )
                    })
                    .update_with(
                        [
                            assignment(
                                "processing_next_at",
                                Expression::new(format!("{clock} + ?"))
                                    .with_bindings(vec![i64::from(delay).into()]),
                            ),
                            assignment(
                                "processing_deadline",
                                Expression::new(format!(
                                    "coalesce(processing_deadline, {clock} + ?)"
                                ))
                                .with_bindings(vec![i64::from(policy.max_age_seconds).into()]),
                            ),
                            assignment(
                                "processing_deferrals",
                                Expression::new("processing_deferrals + 1"),
                            ),
                            assignment("processing_token", Value::Null),
                            assignment("processing_until", Value::Null),
                        ]
                        .into_iter()
                        .collect(),
                    )
                    .await?;
            }
            if changed == 1 {
                attempts::finish(&mut transaction, call.id(), token, "deferred", clock).await?;
                enqueue(&mut transaction).await?;
                return Ok(true);
            }
            let stopped = self
                .owned_processing_row(transaction.table("webhook_calls"), call, token)?
                .update_with(stopped_assignments(clock))
                .await?;
            if stopped != 1 {
                return Err(Error::ProcessingLeaseLost);
            }
            attempts::finish(&mut transaction, call.id(), token, "stopped", clock).await?;
            Ok(false)
        }
        .await;
        settle_transaction(transaction, result).await
    }

    pub(crate) async fn stop_processing(&self, call: &WebhookCall, token: &str) -> Result<()> {
        self.finish_owned(call, token, stopped_assignments(self.clock()), "stopped")
            .await
    }

    async fn finish_owned(
        &self,
        call: &WebhookCall,
        token: &str,
        changes: BTreeMap<String, UpdateValue<'static, 'static>>,
        outcome: &'static str,
    ) -> Result<()> {
        let mut transaction = self.connection.begin_transaction().await?;
        let result = async {
            let changed = self
                .owned_processing_row(transaction.table("webhook_calls"), call, token)?
                .update_with(changes)
                .await?;
            if changed != 1 {
                return Err(Error::ProcessingLeaseLost);
            }
            attempts::finish(&mut transaction, call.id(), token, outcome, self.clock()).await
        }
        .await;
        settle_transaction(transaction, result).await
    }
}

async fn settle_transaction<T>(mut transaction: Transaction, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => {
            transaction.commit().await?;
            Ok(value)
        }
        Err(error) => {
            transaction.roll_back(None).await?;
            Err(error)
        }
    }
}

fn stopped_assignments(clock: &str) -> BTreeMap<String, UpdateValue<'static, 'static>> {
    [
        assignment("processing_token", Value::Null),
        assignment("processing_until", Value::Null),
        assignment("processing_next_at", Value::Null),
        assignment("processing_stopped_at", Expression::new(clock)),
        assignment(
            "exception",
            Value::Text(
                serde_json::json!({
                    "code": 0, "message": "Webhook 处理已终止"
                })
                .to_string(),
            ),
        ),
    ]
    .into_iter()
    .collect()
}

fn assignment(
    name: &str,
    value: impl Into<ValueOperand>,
) -> (String, UpdateValue<'static, 'static>) {
    (name.into(), UpdateValue::Operand(value.into()))
}

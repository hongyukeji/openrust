use super::{DatabaseWebhookStore, scope_key};
use crate::{Error, Result, models::WebhookCall};
use openrust::{
    database::{
        Value,
        query::{Builder, Expression, UpdateValue, ValueOperand},
    },
    support::Str,
};

pub(crate) enum ProcessingClaim {
    Owned(String),
    Busy,
    Complete,
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

    fn processing_row(&self, call: &WebhookCall) -> Result<Builder<'static>> {
        if call.scope() != self.scope {
            return Err(Error::RecordNotFound);
        }
        Ok(self
            .connection
            .table("webhook_calls")
            .r#where("scope_key", "=", scope_key(&self.scope))
            .r#where("id", "=", call.id())
            .where_null("processed_at"))
    }

    pub(crate) async fn claim_processing(
        &self,
        call: &WebhookCall,
        seconds: u32,
    ) -> Result<ProcessingClaim> {
        let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let clock = self.clock();
        let changed = self
            .processing_row(call)?
            .where_nested(|query| {
                query
                    .where_null("processing_until")
                    .or_where_raw(&format!("processing_until <= {clock}"), vec![])
            })
            .update_with(
                [
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
            return Ok(ProcessingClaim::Owned(token));
        }
        let row = self
            .row(call.id(), &["scope", "processed_at"])
            .await?
            .ok_or(Error::RecordNotFound)?;
        match row.get("processed_at") {
            Some(Value::Null) => Ok(ProcessingClaim::Busy),
            Some(Value::Integer(_) | Value::Unsigned(_)) => Ok(ProcessingClaim::Complete),
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
        let changed = self
            .processing_row(call)?
            .r#where("processing_token", "=", token)
            .where_raw(&format!("processing_until > {clock}"), vec![])
            .update_with(
                [
                    assignment("processing_token", Value::Null),
                    assignment("processing_until", Value::Null),
                    assignment("processed_at", processed_at),
                    assignment("exception", exception),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        if changed != 1 {
            return Err(Error::ProcessingLeaseLost);
        }
        Ok(())
    }
}

fn assignment(
    name: &str,
    value: impl Into<ValueOperand>,
) -> (String, UpdateValue<'static, 'static>) {
    (name.into(), UpdateValue::Operand(value.into()))
}

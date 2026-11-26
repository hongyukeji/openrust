//! 接收处理代次的固定诊断随租约状态在同一事务中提交。
use crate::{Error, Result};
use openrust::database::{
    Transaction, Value,
    query::{Expression, Record, UpdateValue, ValueOperand},
};

pub(super) async fn begin(
    transaction: &mut Transaction,
    call_id: &str,
    token: &str,
    clock: &str,
) -> Result<()> {
    expire(transaction, call_id, clock).await?;
    let receipt = transaction
        .table("webhook_calls")
        .select(["processing_generation"])
        .select_raw(&format!("{clock} as attempt_started_at"), vec![])
        .r#where("id", "=", call_id)
        .r#where("processing_token", "=", token)
        .first()
        .await?
        .ok_or(Error::ProcessingLeaseLost)?;
    let generation = receipt
        .get("processing_generation")
        .cloned()
        .ok_or(Error::InvalidRecord)?;
    let started_at = receipt
        .get("attempt_started_at")
        .cloned()
        .ok_or(Error::InvalidRecord)?;
    let row: Record = [
        ("id".into(), token.into()),
        ("call_id".into(), call_id.into()),
        ("generation".into(), generation),
        ("started_at".into(), started_at),
        ("outcome".into(), "running".into()),
    ]
    .into_iter()
    .collect();
    transaction
        .table("webhook_processing_attempts")
        .insert(&[row])
        .await?;
    Ok(())
}

pub(super) async fn expire(
    transaction: &mut Transaction,
    call_id: &str,
    clock: &str,
) -> Result<()> {
    transaction
        .table("webhook_processing_attempts")
        .r#where("call_id", "=", call_id)
        .where_null("finished_at")
        .update_with(settlement("expired", clock))
        .await?;
    Ok(())
}

pub(super) async fn finish(
    transaction: &mut Transaction,
    call_id: &str,
    token: &str,
    outcome: &'static str,
    clock: &str,
) -> Result<()> {
    let changed = transaction
        .table("webhook_processing_attempts")
        .r#where("call_id", "=", call_id)
        .r#where("id", "=", token)
        .where_null("finished_at")
        .update_with(settlement(outcome, clock))
        .await?;
    if changed != 1 {
        return Err(Error::ProcessingLeaseLost);
    }
    Ok(())
}

fn settlement(
    outcome: &'static str,
    clock: &str,
) -> std::collections::BTreeMap<String, UpdateValue<'static, 'static>> {
    [
        (
            "outcome".into(),
            UpdateValue::Operand(ValueOperand::Value(Value::from(outcome))),
        ),
        (
            "finished_at".into(),
            UpdateValue::Operand(Expression::new(clock).into()),
        ),
    ]
    .into_iter()
    .collect()
}

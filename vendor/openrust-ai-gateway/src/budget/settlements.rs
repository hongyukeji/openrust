use openrust::database::{
    Connection, DatabaseError, DatabaseResult, Row, Transaction, Value, query::Record,
};
use serde::Serialize;

use crate::{
    billing::{self, Amount, adjustments::Direction},
    managed_keys::{integer, now, text},
    reconciliation::{actual_costs, optional_text, valid, with_evidence},
};

use super::TABLE as BUDGETS;

const TABLE: &str = "ai_gateway_budget_settlements";

#[derive(Serialize)]
pub(crate) struct BudgetSettlement {
    id: String,
    attempt_id: String,
    direction: String,
    currency: String,
    effective_amount: String,
    booked: Option<Amount>,
    released: Amount,
    reference: String,
    actor: String,
    settled_at: i64,
    duplicate: bool,
}

pub(crate) async fn reconcile(
    connection: &Connection,
    attempt: String,
    direction: Direction,
    reference: String,
    actor: String,
) -> DatabaseResult<BudgetSettlement> {
    if !valid(&reference, 128) || !valid(&actor, 128) {
        return Err(DatabaseError::InvalidQuery(
            "AI 网关预算核对的凭据或操作者无效",
        ));
    }
    let source = connection.clone();
    connection
        .transaction(
            move |transaction| {
                let source = source.clone();
                let attempt = attempt.clone();
                let reference = reference.clone();
                let actor = actor.clone();
                Box::pin(async move {
                    reconcile_attempt(
                        transaction,
                        &source,
                        &attempt,
                        direction,
                        &reference,
                        &actor,
                    )
                    .await
                })
            },
            3,
        )
        .await
}

pub(crate) async fn applied(
    transaction: &mut Transaction,
    attempt: &str,
    direction: Direction,
) -> DatabaseResult<bool> {
    transaction
        .table(TABLE)
        .r#where("attempt_id", "=", attempt)
        .r#where("direction", "=", direction.as_str())
        .where_not_null("booked_atoms")
        .exists()
        .await
}

async fn reconcile_attempt(
    transaction: &mut Transaction,
    connection: &Connection,
    attempt: &str,
    direction: Direction,
    reference: &str,
    actor: &str,
) -> DatabaseResult<BudgetSettlement> {
    let (budget_column, reserved_column) = match direction {
        Direction::Cost => ("account_budget_id", "reserved_cost_atoms"),
        Direction::Receivable => ("budget_id", "reserved_receivable_atoms"),
    };
    // 与最终用量结算共用尝试行锁，迟到证据与实时结算只释放一次。
    let usage = transaction
        .table("ai_gateway_usage")
        .select([
            budget_column,
            reserved_column,
            "finished_at",
            "dispatch_started_at",
        ])
        .r#where("id", "=", attempt)
        .where_not_null(budget_column)
        .lock_for_update()
        .first()
        .await?
        .ok_or(DatabaseError::InvalidQuery("未找到对应的 AI 网关预算预占"))?;
    let direction_name = direction.as_str();
    let id =
        crate::session_affinity::key(&["budget_settlement", attempt, direction_name, reference]);
    if let Some(previous) = transaction
        .table(TABLE)
        .r#where("id", "=", id.as_str())
        .first()
        .await?
    {
        return decode(&previous, true);
    }
    let wallet = text(&usage, budget_column)?;
    let reserved = required(&usage, reserved_column)?;
    let finished = integer(&usage, "finished_at")?.is_some();
    let query = transaction
        .table(billing::TABLE)
        .select([
            "ai_gateway_charges.attempt_id",
            "ai_gateway_charges.currency",
            "ai_gateway_charges.amount",
        ])
        .r#where("ai_gateway_charges.attempt_id", "=", attempt)
        .r#where("ai_gateway_charges.direction", "=", direction_name);
    let rows = billing::adjustments::with_adjustments(with_evidence(query, connection)?)
        .get()
        .await?;
    let charge = rows
        .first()
        .ok_or(DatabaseError::InvalidQuery("未找到对应的 AI 网关费用记录"))?;
    let actual = actual_costs(&rows)?.remove(attempt);
    if !finished
        && (integer(&usage, "dispatch_started_at")?.is_none()
            || actual
                .as_ref()
                .and_then(|proof| proof.amount.as_ref())
                .is_none())
    {
        return Err(DatabaseError::InvalidQuery(
            "未结算的尝试需要已确认的供应商费用证据",
        ));
    }
    let original = optional_text(charge, "amount")?;
    let base = actual
        .as_ref()
        .map_or(original.as_deref(), |proof| proof.amount.as_deref());
    let amount = match billing::adjustments::summarize(&rows, base)? {
        Some(adjustments) => adjustments.net(),
        None => base.map(Amount::ledger).transpose()?,
    }
    .ok_or(DatabaseError::InvalidQuery("AI 网关费用仍待核对"))?;
    let amount_atoms = amount
        .atoms()
        .filter(|value| *value >= 0)
        .ok_or(DatabaseError::InvalidQuery("费用净额超出可结算的预算范围"))?;
    let currency = optional_text(charge, "currency")?
        .or_else(|| actual.and_then(|proof| proof.currency))
        .ok_or(DatabaseError::InvalidQuery("待核对的费用没有确认币种"))?;

    let previous = transaction
        .table(TABLE)
        .r#where("attempt_id", "=", attempt)
        .r#where("direction", "=", direction_name)
        .order_by_desc("sequence")
        .first()
        .await?;
    let sequence = previous.as_ref().map_or(Ok(1), |row| {
        required(row, "sequence")?
            .checked_add(1)
            .ok_or(DatabaseError::InvalidScalar)
    })?;
    let from_ledger = transaction
        .table(TABLE)
        .r#where("attempt_id", "=", attempt)
        .r#where("direction", "=", direction_name)
        .where_not_null("booked_atoms")
        .order_by_desc("sequence")
        .first()
        .await?;
    let previously_booked = match from_ledger {
        Some(row) => Some(required(&row, "booked_atoms")?),
        None => original
            .as_deref()
            .map(Amount::ledger)
            .transpose()?
            .and_then(Amount::atoms)
            .filter(|atoms| finished && *atoms <= reserved),
    };
    let counter = transaction
        .table(BUDGETS)
        .r#where("id", "=", wallet.as_str())
        .lock_for_update()
        .first()
        .await?
        .ok_or(DatabaseError::InvalidScalar)?;
    if text(&counter, "currency")? != currency {
        return Err(DatabaseError::InvalidQuery("AI 网关预算币种与账目不一致"));
    }
    let (booked, released) = if amount_atoms <= reserved {
        let released = if previously_booked.is_none() {
            reserved
        } else {
            0
        };
        let held = required(&counter, "held_atoms")?
            .checked_sub(released)
            .filter(|n| *n >= 0)
            .ok_or(DatabaseError::InvalidScalar)?;
        let occupied = required(&counter, "occupied_atoms")?
            .checked_sub(previously_booked.unwrap_or(reserved))
            .and_then(|value| value.checked_add(amount_atoms))
            .filter(|value| *value >= held)
            .ok_or(DatabaseError::InvalidScalar)?;
        transaction
            .table(BUDGETS)
            .r#where("id", "=", wallet.as_str())
            .update(Record::from([
                ("occupied_atoms".into(), occupied.into()),
                ("held_atoms".into(), held.into()),
            ]))
            .await?;
        (Some(amount_atoms), released)
    } else {
        transaction
            .table(BUDGETS)
            .r#where("id", "=", wallet.as_str())
            .update(Record::from([("overrun".into(), true.into())]))
            .await?;
        (None, 0)
    };
    if !transaction
        .table(TABLE)
        .insert(&[Record::from([
            ("id".into(), id.as_str().into()),
            ("attempt_id".into(), attempt.into()),
            ("direction".into(), direction_name.into()),
            ("budget_id".into(), wallet.as_str().into()),
            ("sequence".into(), sequence.into()),
            ("currency".into(), currency.as_str().into()),
            ("effective_amount".into(), String::from(amount).into()),
            (
                "booked_atoms".into(),
                booked.map_or(Value::Null, Into::into),
            ),
            ("released_atoms".into(), released.into()),
            ("reference".into(), reference.into()),
            ("actor".into(), actor.into()),
            ("settled_at".into(), now().into()),
        ])])
        .await?
    {
        return Err(DatabaseError::InvalidQuery("AI 网关预算核对记录未保存"));
    }
    let row = transaction
        .table(TABLE)
        .r#where("id", "=", id.as_str())
        .first()
        .await?
        .ok_or(DatabaseError::InvalidScalar)?;
    decode(&row, false)
}

fn required(row: &Row, column: &str) -> DatabaseResult<i64> {
    integer(row, column)?.ok_or(DatabaseError::InvalidScalar)
}

fn decode(row: &Row, duplicate: bool) -> DatabaseResult<BudgetSettlement> {
    Ok(BudgetSettlement {
        id: text(row, "id")?,
        attempt_id: text(row, "attempt_id")?,
        direction: text(row, "direction")?,
        currency: text(row, "currency")?,
        effective_amount: text(row, "effective_amount")?,
        booked: integer(row, "booked_atoms")?.map(Amount::from_atoms),
        released: Amount::from_atoms(required(row, "released_atoms")?),
        reference: text(row, "reference")?,
        actor: text(row, "actor")?,
        settled_at: required(row, "settled_at")?,
        duplicate,
    })
}

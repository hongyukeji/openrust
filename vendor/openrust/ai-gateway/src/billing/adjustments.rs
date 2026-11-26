use std::collections::BTreeMap;

use openrust::database::{
    Connection, DatabaseError, DatabaseResult, Row, Value,
    query::{Builder, Record},
};
use serde::{Deserialize, Serialize};

use crate::{
    managed_keys::{integer, now, text},
    reconciliation::{actual_costs, fingerprint, optional_text, valid, with_evidence},
};

use super::{Amount, TABLE as CHARGES};

const TABLE: &str = "ai_gateway_charge_adjustments";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    attempt_id: String,
    direction: Direction,
    kind: Kind,
    currency: String,
    amount: Amount,
    reference: String,
    reason: String,
    report_id: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Cost,
    Receivable,
}

impl Direction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cost => "cost",
            Self::Receivable => "receivable",
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Debit,
    Credit,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Debit => "debit",
            Self::Credit => "credit",
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Entry {
    id: String,
    kind: String,
    amount: String,
    reference: String,
    reason: String,
    report_id: Option<String>,
    actor: String,
    created_at: i64,
}

#[derive(Serialize)]
pub(crate) struct Applied {
    attempt_id: String,
    direction: String,
    currency: String,
    duplicate: bool,
    adjustment: Entry,
}

#[derive(Serialize)]
pub(crate) struct Summary {
    debits: Amount,
    credits: Amount,
    net: Option<Amount>,
    entries: Vec<Entry>,
}

impl Input {
    fn validate(&mut self, actor: &str) -> DatabaseResult<()> {
        self.attempt_id.make_ascii_uppercase();
        if let Some(report) = &mut self.report_id {
            report.make_ascii_lowercase();
        }
        if !valid(actor, 128)
            || !valid(&self.reference, 128)
            || self.reason.trim().is_empty()
            || self.reason.chars().any(char::is_control)
            || self.attempt_id.len() != 26
            || !self
                .attempt_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            || self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.amount.is_zero()
            || self.report_id.as_ref().is_some_and(|id| {
                id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(DatabaseError::InvalidQuery("AI 网关费用调整参数无效"));
        }
        Ok(())
    }

    fn id(&self) -> String {
        crate::session_affinity::key(&[
            "billing_adjustment",
            &self.attempt_id,
            self.direction.as_str(),
            &self.reference,
        ])
    }
}

pub(crate) async fn post(
    connection: &Connection,
    mut input: Input,
    actor: String,
) -> DatabaseResult<Applied> {
    input.validate(&actor)?;
    let source = connection.clone();
    connection
        .transaction(
            move |transaction| {
                let source = source.clone();
                let input = input.clone();
                let actor = actor.clone();
                Box::pin(async move {
                    let id = input.id();
                    let direction = input.direction.as_str();
                    let fingerprint = fingerprint(&input);
                    let inserted = transaction
                        .table(TABLE)
                        .insert_or_ignore(&[Record::from([
                            ("id".into(), id.as_str().into()),
                            ("attempt_id".into(), input.attempt_id.as_str().into()),
                            ("direction".into(), direction.into()),
                            ("currency".into(), input.currency.as_str().into()),
                            ("kind".into(), input.kind.as_str().into()),
                            ("amount".into(), String::from(input.amount).into()),
                            ("reference".into(), input.reference.as_str().into()),
                            ("reason".into(), input.reason.as_str().into()),
                            (
                                "report_id".into(),
                                input.report_id.as_deref().map_or(Value::Null, Into::into),
                            ),
                            ("actor".into(), actor.as_str().into()),
                            ("created_at".into(), now().into()),
                            ("fingerprint".into(), fingerprint.as_str().into()),
                        ])])
                        .await?;
                    let row = transaction
                        .table(TABLE)
                        .r#where("id", "=", id.as_str())
                        .select([
                            "id as adjustment_id",
                            "kind as adjustment_kind",
                            "amount as adjustment_amount",
                            "reference as adjustment_reference",
                            "reason as adjustment_reason",
                            "report_id as adjustment_report_id",
                            "actor as adjustment_actor",
                            "created_at as adjustment_created_at",
                            "fingerprint",
                        ])
                        .lock_for_update()
                        .first()
                        .await?
                        .ok_or(DatabaseError::InvalidScalar)?;
                    if text(&row, "fingerprint")? != fingerprint {
                        return Err(DatabaseError::InvalidQuery(
                            "AI 网关同一费用调整凭据的内容不同",
                        ));
                    }
                    if inserted != 0 {
                        let query = transaction
                            .table(CHARGES)
                            .join(
                                "ai_gateway_usage",
                                "ai_gateway_charges.attempt_id",
                                "=",
                                "ai_gateway_usage.id",
                            )
                            .select([
                                "ai_gateway_charges.attempt_id",
                                "ai_gateway_charges.currency",
                            ])
                            .r#where(
                                "ai_gateway_charges.attempt_id",
                                "=",
                                input.attempt_id.as_str(),
                            )
                            .r#where("ai_gateway_charges.direction", "=", direction)
                            .where_not_null("ai_gateway_usage.finished_at")
                            .r#where("ai_gateway_charges.state", "<>", "not_billable");
                        let rows = with_evidence(query, &source)?.get().await?;
                        let origin = rows
                            .first()
                            .ok_or(DatabaseError::InvalidQuery("未找到可调整的已结算费用"))?;
                        let actual = actual_costs(&rows)?.remove(&input.attempt_id);
                        let currency = optional_text(origin, "currency")?
                            .or_else(|| actual.and_then(|actual| actual.currency));
                        if currency.as_deref() != Some(input.currency.as_str()) {
                            return Err(DatabaseError::InvalidQuery("费用调整币种与原账目不一致"));
                        }
                        if let Some(report_id) = &input.report_id
                            && !transaction
                                .table("ai_gateway_cost_reports")
                                .r#where("id", "=", report_id.as_str())
                                .r#where("complete", "=", true)
                                .exists()
                                .await?
                        {
                            return Err(DatabaseError::InvalidQuery("未找到已完整导入的关联账单"));
                        }
                    }
                    Ok(Applied {
                        attempt_id: input.attempt_id,
                        direction: direction.into(),
                        currency: input.currency,
                        duplicate: inserted == 0,
                        adjustment: entry(&row)?,
                    })
                })
            },
            3,
        )
        .await
}

pub(crate) fn with_adjustments(query: Builder<'_>) -> Builder<'_> {
    query
        .left_join_with(TABLE, |join| {
            join.on(
                "ai_gateway_charges.attempt_id",
                "=",
                "ai_gateway_charge_adjustments.attempt_id",
            )
            .on(
                "ai_gateway_charges.direction",
                "=",
                "ai_gateway_charge_adjustments.direction",
            )
        })
        .add_select([
            "ai_gateway_charge_adjustments.id as adjustment_id",
            "ai_gateway_charge_adjustments.kind as adjustment_kind",
            "ai_gateway_charge_adjustments.amount as adjustment_amount",
            "ai_gateway_charge_adjustments.reference as adjustment_reference",
            "ai_gateway_charge_adjustments.reason as adjustment_reason",
            "ai_gateway_charge_adjustments.report_id as adjustment_report_id",
            "ai_gateway_charge_adjustments.actor as adjustment_actor",
            "ai_gateway_charge_adjustments.created_at as adjustment_created_at",
        ])
}

fn entry(row: &Row) -> DatabaseResult<Entry> {
    Ok(Entry {
        id: text(row, "adjustment_id")?,
        kind: text(row, "adjustment_kind")?,
        amount: text(row, "adjustment_amount")?,
        reference: text(row, "adjustment_reference")?,
        reason: text(row, "adjustment_reason")?,
        report_id: optional_text(row, "adjustment_report_id")?,
        actor: text(row, "adjustment_actor")?,
        created_at: integer(row, "adjustment_created_at")?.ok_or(DatabaseError::InvalidScalar)?,
    })
}

pub(crate) fn summarize(rows: &[Row], base: Option<&str>) -> DatabaseResult<Option<Summary>> {
    let mut entries = BTreeMap::new();
    for row in rows {
        if let Some(id) = optional_text(row, "adjustment_id")?
            && let std::collections::btree_map::Entry::Vacant(slot) = entries.entry(id)
        {
            slot.insert(entry(row)?);
        }
    }
    if entries.is_empty() {
        return Ok(None);
    }
    let mut debits = Amount::ZERO;
    let mut credits = Amount::ZERO;
    for row in entries.values() {
        let amount = Amount::ledger(&row.amount)?;
        match row.kind.as_str() {
            "debit" => debits = debits.add(amount)?,
            "credit" => credits = credits.add(amount)?,
            _ => return Err(DatabaseError::InvalidScalar),
        }
    }
    let net = base
        .map(|base| Amount::ledger(base)?.add(debits)?.difference(credits))
        .transpose()?;
    Ok(Some(Summary {
        debits,
        credits,
        net,
        entries: entries.into_values().collect(),
    }))
}

impl Summary {
    pub(crate) fn net(&self) -> Option<Amount> {
        self.net
    }
}

use std::collections::{BTreeMap, BTreeSet};

use futures_util::TryStreamExt;
use openrust::database::{
    Connection, DatabaseError, DatabaseResult, Row, Transaction, Value, query::Record,
};
use serde::{Deserialize, Serialize};

use crate::{
    billing::{self, Amount},
    managed_keys::text,
    reconciliation::{actual_costs, optional_text, valid, with_evidence},
};

mod gemini_storage;

const TABLE: &str = "ai_gateway_cost_totals";
const TOKENS: &str = "tokens";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CostTotal {
    id: String,
    provider: String,
    currency: String,
    amount: Amount,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct ComparedTotal {
    id: String,
    provider: String,
    currency: String,
    category: String,
    state: String,
    reported: String,
    recorded: Option<String>,
    difference: Option<String>,
    priced_attempts: u64,
    pending_attempts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage: Option<gemini_storage::StorageComparison>,
}

pub(crate) fn validate(totals: &mut [CostTotal]) -> DatabaseResult<()> {
    let mut ids = BTreeSet::new();
    let mut scopes = BTreeSet::new();
    for total in totals.iter_mut() {
        if total.category.as_deref() == Some(TOKENS) {
            total.category = None;
        }
        if !valid(&total.id, 128)
            || !valid(&total.provider, 255)
            || total.currency.len() != 3
            || !total.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || total
                .category
                .as_ref()
                .is_some_and(|category| !valid(category, 128))
            || !ids.insert(&total.id)
            || !scopes.insert((&total.provider, &total.currency, &total.category))
        {
            return Err(DatabaseError::InvalidQuery("AI 网关账单汇总项无效或重复"));
        }
    }
    totals.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(())
}

#[derive(Clone, Copy)]
struct Recorded {
    amount: Amount,
    priced: u64,
    pending: u64,
}

impl Default for Recorded {
    fn default() -> Self {
        Self {
            amount: Amount::ZERO,
            priced: 0,
            pending: 0,
        }
    }
}

type RecordedTotals = BTreeMap<(String, Option<String>), Recorded>;

async fn recorded(
    transaction: &mut Transaction,
    connection: &Connection,
    totals: &[CostTotal],
    window: (i64, i64),
) -> DatabaseResult<RecordedTotals> {
    let providers: BTreeSet<_> = totals
        .iter()
        .filter(|total| total.category.is_none())
        .map(|total| total.provider.as_str())
        .collect();
    let mut result = RecordedTotals::new();
    if providers.is_empty() {
        return Ok(result);
    }
    // 一条查询采集快照，按各连接币种累加精确小数。
    let query = transaction
        .table("ai_gateway_usage")
        .left_join_with(billing::TABLE, |join| {
            join.on("ai_gateway_usage.id", "=", "ai_gateway_charges.attempt_id")
                .r#where("ai_gateway_charges.direction", "=", "cost")
        })
        .select([
            "ai_gateway_usage.id",
            "ai_gateway_usage.provider",
            "ai_gateway_charges.attempt_id",
            "ai_gateway_charges.currency",
            "ai_gateway_charges.amount",
        ])
        .where_in(
            "ai_gateway_usage.provider",
            providers
                .iter()
                .map(|provider| Value::from(*provider))
                .collect(),
        )
        .where_not_in(
            "ai_gateway_usage.endpoint",
            crate::usage::gemini_caches::OPERATIONS
                .into_iter()
                .map(Value::from)
                .collect(),
        )
        .r#where("ai_gateway_usage.dispatch_started_at", ">=", window.0)
        .r#where("ai_gateway_usage.dispatch_started_at", "<", window.1);
    let mut rows = billing::adjustments::with_adjustments(with_evidence(query, connection)?)
        .order_by("ai_gateway_usage.id", "asc")
        .cursor();
    let mut attempt_rows = Vec::<Row>::new();
    while let Some(row) = rows.try_next().await? {
        if attempt_rows
            .first()
            .is_some_and(|previous| previous.get("id") != row.get("id"))
        {
            count_attempt(&mut result, &providers, &attempt_rows)?;
            attempt_rows.clear();
        }
        attempt_rows.push(row);
    }
    if !attempt_rows.is_empty() {
        count_attempt(&mut result, &providers, &attempt_rows)?;
    }
    Ok(result)
}

fn count_attempt(
    result: &mut RecordedTotals,
    providers: &BTreeSet<&str>,
    rows: &[Row],
) -> DatabaseResult<()> {
    let row = &rows[0];
    let provider = text(row, "provider")?;
    if !providers.contains(provider.as_str()) {
        return Ok(());
    }
    let mut currency = optional_text(row, "currency")?;
    let mut amount = optional_text(row, "amount")?;
    if let Some(actual) = actual_costs(rows)?.into_values().next() {
        currency = actual.currency.or(currency);
        amount = actual.amount;
    }
    let entry = result.entry((provider, currency)).or_default();
    let value = match billing::adjustments::summarize(rows, amount.as_deref())? {
        Some(adjustments) => adjustments.net(),
        None => amount.as_deref().map(Amount::ledger).transpose()?,
    };
    if let Some(value) = value {
        entry.amount = entry.amount.add(value)?;
        entry.priced += 1;
    } else {
        entry.pending += 1;
    }
    Ok(())
}

pub(crate) async fn save(
    transaction: &mut Transaction,
    connection: &Connection,
    report_id: &str,
    totals: &[CostTotal],
    window: (i64, i64),
    complete: bool,
) -> DatabaseResult<Vec<ComparedTotal>> {
    let baseline = recorded(transaction, connection, totals, window).await?;
    let mut scopes = BTreeMap::<String, BTreeSet<String>>::new();
    let period_elapsed = window.1 <= crate::managed_keys::now();
    if complete && period_elapsed {
        for total in totals
            .iter()
            .filter(|total| total.category.as_deref() == Some(gemini_storage::CATEGORY))
        {
            scopes
                .entry(total.provider.clone())
                .or_default()
                .insert(total.currency.clone());
        }
    }
    let storage = gemini_storage::totals(transaction, &scopes, window).await?;
    let mut compared = Vec::with_capacity(totals.len());
    for total in totals {
        let (known, pending) = if total.category.is_none() {
            let known = baseline
                .get(&(total.provider.clone(), Some(total.currency.clone())))
                .copied()
                .unwrap_or_default();
            let unknown = baseline
                .get(&(total.provider.clone(), None))
                .copied()
                .unwrap_or_default();
            (known, known.pending + unknown.pending)
        } else {
            (Recorded::default(), 0)
        };
        let storage_estimate = (complete
            && total.category.as_deref() == Some(gemini_storage::CATEGORY))
        .then(|| storage.get(&(total.provider.clone(), total.currency.clone())))
        .flatten();
        let storage_comparison = storage_estimate
            .map(|estimate| estimate.comparison(total.amount))
            .transpose()?;
        let mut difference = None;
        let state = if !complete {
            "partial_report"
        } else if total.category.as_deref() == Some(gemini_storage::CATEGORY) {
            match storage_estimate {
                _ if !period_elapsed => "pending_storage_evidence",
                Some(estimate) if estimate.pending > 0 => "pending_storage_evidence",
                Some(estimate) if estimate.priced > 0 => "estimated_storage",
                _ => "no_local_storage",
            }
        } else if total.category.is_some() {
            "unmapped_category"
        } else if pending != 0 {
            "pending_local_cost"
        } else if known.priced == 0 {
            "no_local_cost"
        } else {
            let delta = total.amount.difference(known.amount)?;
            difference = Some(String::from(delta));
            if delta.is_zero() {
                "matched"
            } else {
                "difference"
            }
        };
        let row = ComparedTotal {
            id: total.id.clone(),
            provider: total.provider.clone(),
            currency: total.currency.clone(),
            category: total.category.as_deref().unwrap_or(TOKENS).into(),
            state: state.into(),
            reported: total.amount.into(),
            recorded: (known.priced != 0).then(|| known.amount.into()),
            difference,
            priced_attempts: known.priced,
            pending_attempts: pending,
            storage: storage_comparison,
        };
        let snapshot = serde_json::to_string(&row).map_err(|_| DatabaseError::InvalidScalar)?;
        if !transaction
            .table(TABLE)
            .insert(&[Record::from([
                ("report_id".into(), report_id.into()),
                ("entry_id".into(), row.id.as_str().into()),
                ("result".into(), snapshot.into()),
            ])])
            .await?
        {
            return Err(DatabaseError::InvalidQuery("AI 网关账单汇总项未保存"));
        }
        compared.push(row);
    }
    Ok(compared)
}

pub(crate) async fn load(
    transaction: &mut Transaction,
    report_id: &str,
) -> DatabaseResult<Vec<ComparedTotal>> {
    transaction
        .table(TABLE)
        .r#where("report_id", "=", report_id)
        .order_by("entry_id", "asc")
        .select(["result"])
        .get()
        .await?
        .iter()
        .map(|row| {
            serde_json::from_str(&text(row, "result")?).map_err(|_| DatabaseError::InvalidScalar)
        })
        .collect()
}

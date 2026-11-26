use std::collections::{BTreeMap, BTreeSet};

use openrust::database::{
    Connection, DatabaseError, DatabaseResult, Row, Transaction, Value,
    query::{Builder, Record},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    billing::{self, Amount},
    managed_keys::{integer, now, text},
};

const REPORTS: &str = "ai_gateway_cost_reports";
const EVIDENCE: &str = "ai_gateway_cost_evidence";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CostReport {
    source: String,
    statement_id: String,
    revision: i64,
    complete: bool,
    starts_at: i64,
    ends_at: i64,
    entries: Vec<CostEntry>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CostEntry {
    id: String,
    provider: String,
    upstream_request_id: String,
    currency: String,
    amount: Amount,
}

#[derive(Serialize)]
pub(crate) struct ImportResult {
    report_id: String,
    complete: bool,
    duplicate: bool,
    entries: Vec<ImportedEntry>,
}

#[derive(Serialize)]
struct ImportedEntry {
    id: String,
    state: String,
    attempt_id: Option<String>,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ActualCost {
    pub(crate) state: &'static str,
    pub(crate) currency: Option<String>,
    pub(crate) amount: Option<String>,
}

impl CostReport {
    fn validate(&mut self, actor: &str) -> DatabaseResult<()> {
        if !valid(&self.source, 128)
            || !valid(&self.statement_id, 128)
            || !valid(actor, 128)
            || self.revision < 1
            || self.starts_at < 0
            || self.ends_at <= self.starts_at
        {
            return Err(DatabaseError::InvalidQuery(
                "AI 网关账单标识、版本或时间窗口无效",
            ));
        }
        let mut receipts = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            if !valid(&entry.id, 128)
                || !valid(&entry.provider, 255)
                || !valid(&entry.upstream_request_id, 255)
                || entry.currency.len() != 3
                || !entry.currency.bytes().all(|byte| byte.is_ascii_uppercase())
                || !ids.insert(&entry.id)
                || !receipts.insert((&entry.provider, &entry.upstream_request_id, &entry.currency))
            {
                return Err(DatabaseError::InvalidQuery("AI 网关账单明细无效或重复"));
            }
        }
        self.entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(())
    }

    fn statement_scope(&self) -> String {
        fingerprint(&(
            &self.source,
            &self.statement_id,
            self.starts_at,
            self.ends_at,
        ))
    }

    fn report_id(&self) -> String {
        fingerprint(&(self.statement_scope(), self.revision))
    }
}

pub(crate) async fn import(
    connection: &Connection,
    mut report: CostReport,
    actor: String,
) -> DatabaseResult<ImportResult> {
    report.validate(&actor)?;
    connection
        .transaction(
            move |transaction| {
                let report = report.clone();
                let actor = actor.clone();
                Box::pin(async move { insert_report(transaction, &report, &actor).await })
            },
            3,
        )
        .await
}

async fn insert_report(
    transaction: &mut Transaction,
    report: &CostReport,
    actor: &str,
) -> DatabaseResult<ImportResult> {
    let id = report.report_id();
    let scope = report.statement_scope();
    let hash = fingerprint(report);
    let inserted = transaction
        .table(REPORTS)
        .insert_or_ignore(&[Record::from([
            ("id".into(), id.as_str().into()),
            ("statement_scope".into(), scope.as_str().into()),
            ("source".into(), report.source.as_str().into()),
            ("statement_id".into(), report.statement_id.as_str().into()),
            ("revision".into(), report.revision.into()),
            ("complete".into(), report.complete.into()),
            ("starts_at".into(), report.starts_at.into()),
            ("ends_at".into(), report.ends_at.into()),
            ("content_hash".into(), hash.as_str().into()),
            ("actor".into(), actor.into()),
            ("imported_at".into(), now().into()),
        ])])
        .await?;
    let entries = if inserted == 0 {
        let existing = transaction
            .table(REPORTS)
            .select(["content_hash"])
            .r#where("id", "=", id.as_str())
            .lock_for_update()
            .first()
            .await?
            .ok_or(DatabaseError::InvalidQuery("AI 网关账单未保存"))?;
        if text(&existing, "content_hash")? != hash {
            return Err(DatabaseError::InvalidQuery("AI 网关同一账单版本的内容不同"));
        }
        report_entries(transaction, &id).await?
    } else {
        let mut entries = Vec::with_capacity(report.entries.len());
        for entry in &report.entries {
            let (state, attempt_id) = match_attempt(transaction, report, entry).await?;
            let entry_scope = fingerprint(&(
                &scope,
                &entry.provider,
                &entry.upstream_request_id,
                &entry.currency,
            ));
            let record = Record::from([
                ("report_id".into(), id.as_str().into()),
                ("entry_id".into(), entry.id.as_str().into()),
                ("entry_scope".into(), entry_scope.into()),
                (
                    "attempt_id".into(),
                    attempt_id.as_deref().map_or(Value::Null, Into::into),
                ),
                ("provider".into(), entry.provider.as_str().into()),
                (
                    "upstream_request_id".into(),
                    entry.upstream_request_id.as_str().into(),
                ),
                ("currency".into(), entry.currency.as_str().into()),
                ("amount".into(), String::from(entry.amount).into()),
                ("state".into(), state.into()),
            ]);
            if !transaction.table(EVIDENCE).insert(&[record]).await? {
                return Err(DatabaseError::InvalidQuery("AI 网关账单明细未保存"));
            }
            entries.push(ImportedEntry {
                id: entry.id.clone(),
                state: state.into(),
                attempt_id,
            });
        }
        entries
    };
    Ok(ImportResult {
        report_id: id,
        complete: report.complete,
        duplicate: inserted == 0,
        entries,
    })
}

async fn report_entries(
    transaction: &mut Transaction,
    report_id: &str,
) -> DatabaseResult<Vec<ImportedEntry>> {
    transaction
        .table(EVIDENCE)
        .select(["entry_id", "state", "attempt_id"])
        .r#where("report_id", "=", report_id)
        .order_by("entry_id", "asc")
        .get()
        .await?
        .iter()
        .map(|row| {
            Ok(ImportedEntry {
                id: text(row, "entry_id")?,
                state: text(row, "state")?,
                attempt_id: optional_text(row, "attempt_id")?,
            })
        })
        .collect()
}

async fn match_attempt(
    transaction: &mut Transaction,
    report: &CostReport,
    entry: &CostEntry,
) -> DatabaseResult<(&'static str, Option<String>)> {
    let attempts = transaction
        .table("ai_gateway_usage")
        .select(["id", "provider", "upstream_request_id"])
        .r#where("provider", "=", entry.provider.as_str())
        .r#where(
            "upstream_request_id",
            "=",
            entry.upstream_request_id.as_str(),
        )
        .r#where("dispatch_started_at", ">=", report.starts_at)
        .r#where("dispatch_started_at", "<", report.ends_at)
        .get()
        .await?;
    // MySQL 的默认排序规则可能不区分大小写。账单关联必须精确比较上游标识。
    let mut matching = attempts.iter().filter(|row| {
        row.get("provider") == Some(&Value::from(entry.provider.as_str()))
            && row.get("upstream_request_id")
                == Some(&Value::from(entry.upstream_request_id.as_str()))
    });
    let Some(attempt) = matching.next() else {
        return Ok(("unmatched", None));
    };
    if matching.next().is_some() {
        return Ok(("ambiguous", None));
    }
    let id = text(attempt, "id")?;
    let charge = transaction
        .table(billing::TABLE)
        .select(["currency"])
        .r#where("attempt_id", "=", id.as_str())
        .r#where("direction", "=", "cost")
        .first()
        .await?;
    let Some(charge) = charge else {
        return Ok(("missing_charge", Some(id)));
    };
    if optional_text(&charge, "currency")?.is_some_and(|currency| currency != entry.currency) {
        return Ok(("currency_mismatch", Some(id)));
    }
    Ok(("matched", Some(id)))
}

pub(crate) fn evidence_query(connection: &Connection) -> Builder<'_> {
    connection
        .table(EVIDENCE)
        .join(
            REPORTS,
            "ai_gateway_cost_evidence.report_id",
            "=",
            "ai_gateway_cost_reports.id",
        )
        .select([
            "ai_gateway_cost_evidence.attempt_id",
            "ai_gateway_cost_evidence.entry_scope",
            "ai_gateway_cost_evidence.currency",
            "ai_gateway_cost_evidence.amount",
            "ai_gateway_cost_reports.revision",
        ])
        .r#where("ai_gateway_cost_evidence.state", "=", "matched")
        .r#where("ai_gateway_cost_reports.complete", "=", true)
}

pub(crate) fn actual_costs(rows: &[Row]) -> DatabaseResult<BTreeMap<String, ActualCost>> {
    let mut latest = BTreeMap::<(String, String), (i64, ActualCost)>::new();
    for row in rows {
        let Some(revision) = integer(row, "evidence_revision")? else {
            continue;
        };
        let scope = (text(row, "attempt_id")?, text(row, "evidence_scope")?);
        let candidate = ActualCost {
            state: "confirmed",
            currency: Some(text(row, "evidence_currency")?),
            amount: Some(text(row, "evidence_amount")?),
        };
        let selected = latest.entry(scope).or_insert((revision, candidate.clone()));
        if revision > selected.0 {
            *selected = (revision, candidate);
        }
    }
    let mut resolved = BTreeMap::<String, ActualCost>::new();
    for ((attempt, _), (_, candidate)) in latest {
        let current = resolved.entry(attempt).or_insert_with(|| candidate.clone());
        if *current != candidate {
            *current = ActualCost {
                state: "conflicting_evidence",
                currency: None,
                amount: None,
            };
        }
    }
    Ok(resolved)
}

fn optional_text(row: &Row, key: &str) -> DatabaseResult<Option<String>> {
    match row.get(key) {
        Some(Value::Text(text)) => Ok(Some(text.clone())),
        Some(Value::Null) => Ok(None),
        _ => Err(DatabaseError::InvalidScalar),
    }
}

fn fingerprint(value: &impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("供应商用量证据可序列化"))
    )
}

fn valid(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|byte| byte.is_ascii_graphic())
}

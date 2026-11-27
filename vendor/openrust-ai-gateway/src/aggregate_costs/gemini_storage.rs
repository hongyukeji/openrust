use std::collections::{BTreeMap, BTreeSet};

use futures_util::TryStreamExt;
use openrust::database::{DatabaseError, DatabaseResult, Row, Transaction, Value};
use serde::{Deserialize, Serialize};

use crate::{
    billing::{self, Amount},
    managed_keys::{integer, text},
    reconciliation::optional_text,
    usage::gemini_caches::{CREATE, DELETE, OPERATIONS, UPDATE},
};

pub(super) const CATEGORY: &str = "gemini_context_cache_storage";

#[derive(Default)]
pub(super) struct Estimate {
    numerator: i128,
    pub(super) priced: u64,
    pub(super) pending: u64,
}

#[derive(Serialize, Deserialize)]
pub(super) struct StorageComparison {
    pub(super) estimated: Option<String>,
    pub(super) estimated_difference: Option<String>,
    priced_resources: u64,
    pending_resources: u64,
}

type Totals = BTreeMap<(String, String), Estimate>;

impl Estimate {
    pub(super) fn comparison(&self, reported: Amount) -> DatabaseResult<StorageComparison> {
        let estimated = (self.priced > 0)
            .then(|| {
                Amount::from_cache_storage_numerator(self.numerator)
                    .ok_or(DatabaseError::InvalidScalar)
            })
            .transpose()?;
        let delta = estimated
            .filter(|_| self.pending == 0)
            .map(|estimate| reported.difference(estimate))
            .transpose()?;
        Ok(StorageComparison {
            estimated: estimated.map(Into::into),
            estimated_difference: delta.map(Into::into),
            priced_resources: self.priced,
            pending_resources: self.pending,
        })
    }

    fn add(&mut self, value: Option<i128>) -> DatabaseResult<()> {
        if let Some(value) = value {
            self.numerator = self
                .numerator
                .checked_add(value)
                .ok_or(DatabaseError::InvalidScalar)?;
            self.priced += 1;
        } else {
            self.pending += 1;
        }
        Ok(())
    }
}

struct Event {
    action: String,
    dispatched: i64,
    finished: Option<i64>,
    state: String,
    status: Option<i64>,
    evidence: Option<Evidence>,
    currency: Option<String>,
    tariff: Option<StorageTariff>,
}

impl Event {
    fn decode(row: &Row) -> DatabaseResult<Self> {
        let parse_evidence = |column| {
            optional_text(row, column)?
                .as_deref()
                .map(serde_json::from_str::<Evidence>)
                .transpose()
                .map_err(|_| DatabaseError::InvalidScalar)
        };
        Ok(Self {
            action: text(row, "endpoint")?,
            dispatched: integer(row, "dispatch_started_at")?.ok_or(DatabaseError::InvalidScalar)?,
            finished: integer(row, "finished_at")?,
            state: text(row, "outcome")?,
            status: integer(row, "status")?,
            evidence: parse_evidence("resource_usage")?,
            currency: optional_text(row, "currency")?,
            tariff: optional_text(row, "tariff")?
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| DatabaseError::InvalidScalar)?,
        })
    }

    fn rejected(&self) -> bool {
        matches!(self.status, Some(400 | 401 | 403 | 405 | 422 | 429))
    }
}

#[derive(Deserialize)]
struct Evidence {
    created_at: Option<i64>,
    updated_at: Option<i64>,
    expires_at: Option<i64>,
    tokens: Option<i64>,
}

#[derive(Deserialize)]
struct StorageTariff {
    cache_storage_per_million_token_hours: Option<Amount>,
}

struct Segment {
    start: i64,
    end: i64,
    tokens: i64,
    rate: Option<Amount>,
}

impl Segment {
    fn from_event(
        event: &Event,
        start: i64,
        evidence: &Evidence,
        token_count: i64,
    ) -> Option<Self> {
        let end = evidence.expires_at?;
        let tokens = evidence.tokens.unwrap_or(token_count);
        (start >= 0 && end >= start && tokens >= 0).then_some(Self {
            start,
            end,
            tokens,
            rate: event
                .tariff
                .as_ref()
                .and_then(|rate| rate.cache_storage_per_million_token_hours),
        })
    }

    fn quote(&self, window: (i64, i64), end: i64) -> Option<i128> {
        let seconds = self
            .end
            .min(end)
            .min(window.1)
            .saturating_sub(self.start.max(window.0));
        if seconds <= 0 {
            return Some(0);
        }
        self.rate?.cache_storage_numerator(self.tokens, seconds)
    }

    fn overlaps(&self, window: (i64, i64), end: i64) -> bool {
        self.end.min(end).min(window.1) > self.start.max(window.0)
    }
}

enum ResourceEstimate {
    OutsideWindow,
    Pending,
    Priced(i128),
}

fn estimate(events: &[Event], window: (i64, i64), currency: &str) -> ResourceEstimate {
    match quote(events, window, currency) {
        None => ResourceEstimate::Pending,
        Some((false, _)) => ResourceEstimate::OutsideWindow,
        Some((true, total)) => ResourceEstimate::Priced(total),
    }
}

fn quote(events: &[Event], window: (i64, i64), currency: &str) -> Option<(bool, i128)> {
    let mut events = events.iter().filter(|event| !event.rejected());
    let Some(created) = events.next() else {
        return Some((false, 0));
    };
    let evidence = created.evidence.as_ref()?;
    if created.action != CREATE || created.state != "completed" {
        return None;
    }
    let mut current = Segment::from_event(created, evidence.created_at?, evidence, -1)?;
    let mut total = 0_i128;
    let mut overlaps = false;
    let mut currency_known = created.currency.as_deref() == Some(currency);

    // 续期采用上游生效时间。不能确定先后的并发或失败操作不推算金额。
    while let Some(event) = events.next() {
        if event.action == UPDATE {
            let updated = event.evidence.as_ref()?;
            let time = updated.updated_at?;
            if event.state != "completed" || time <= current.start || time > current.end {
                return None;
            }
            if current.overlaps(window, time) {
                if !currency_known {
                    return None;
                }
                overlaps = true;
                total = total.checked_add(current.quote(window, time)?)?;
            }
            current = Segment::from_event(event, time, updated, current.tokens)?;
            currency_known = event.currency.as_deref() == Some(currency);
        } else if event.action == DELETE && event.state == "completed" {
            let finished = event.finished?;
            if finished < event.dispatched
                || (event.dispatched < finished
                    && current.overlaps(
                        (window.0.max(event.dispatched), window.1.min(finished)),
                        current.end,
                    ))
            {
                return None;
            }
            if current.overlaps(window, event.dispatched) {
                if !currency_known {
                    return None;
                }
                overlaps = true;
                total = total.checked_add(current.quote(window, event.dispatched)?)?;
            }
            return (events.next().is_none()).then_some((overlaps, total));
        } else if !(event.action == DELETE
            && event.status == Some(404)
            && current.end <= event.dispatched)
        {
            return None;
        }
    }
    if current.overlaps(window, current.end) {
        if !currency_known {
            return None;
        }
        overlaps = true;
        total = total.checked_add(current.quote(window, current.end)?)?;
    }
    Some((overlaps, total))
}

pub(super) async fn totals(
    transaction: &mut Transaction,
    scopes: &BTreeMap<String, BTreeSet<String>>,
    window: (i64, i64),
) -> DatabaseResult<Totals> {
    let mut totals = Totals::new();
    if scopes.is_empty() {
        return Ok(totals);
    }
    let query = transaction
        .table("ai_gateway_usage")
        .left_join_with(billing::TABLE, |join| {
            join.on("ai_gateway_usage.id", "=", "ai_gateway_charges.attempt_id")
                .r#where("ai_gateway_charges.direction", "=", "cost")
        })
        .select([
            "ai_gateway_usage.provider",
            "ai_gateway_usage.resource_id",
            "ai_gateway_usage.endpoint",
            "ai_gateway_usage.dispatch_started_at",
            "ai_gateway_usage.finished_at",
            "ai_gateway_usage.status",
            "ai_gateway_usage.outcome",
            "ai_gateway_usage.resource_usage",
            "ai_gateway_charges.currency",
            "ai_gateway_charges.tariff",
        ])
        .where_in(
            "ai_gateway_usage.endpoint",
            OPERATIONS.into_iter().map(Value::from).collect(),
        )
        .where_in(
            "ai_gateway_usage.provider",
            scopes
                .keys()
                .map(|provider| Value::from(provider.as_str()))
                .collect(),
        )
        .r#where("ai_gateway_usage.dispatch_started_at", "<", window.1)
        .where_not_null("ai_gateway_usage.resource_id")
        .order_by("ai_gateway_usage.provider", "asc")
        .order_by("ai_gateway_usage.resource_id", "asc")
        .order_by("ai_gateway_usage.dispatch_started_at", "asc")
        .order_by("ai_gateway_usage.id", "asc");
    let mut rows = query.cursor();
    let mut current = None::<(String, String)>;
    let mut events = Vec::new();
    let mut observed = BTreeSet::new();
    while let Some(row) = rows.try_next().await? {
        let provider = text(&row, "provider")?;
        let id = text(&row, "resource_id")?;
        if !scopes.contains_key(&provider) {
            continue;
        }
        if current
            .as_ref()
            .is_some_and(|(old_provider, old_id)| old_provider != &provider || old_id != &id)
        {
            let (previous, id) = current.take().expect("已读取上一条资源");
            observed.insert(id);
            record(&mut totals, scopes, previous, &events, window)?;
            events.clear();
        }
        current = Some((provider, id));
        events.push(Event::decode(&row)?);
    }
    if let Some((provider, id)) = current {
        observed.insert(id);
        record(&mut totals, scopes, provider, &events, window)?;
    }
    drop(rows);
    // 生命周期记账迁移前已有的有效映射也可能跨入账期。
    let query = transaction
        .table("ai_gateway_gemini_caches")
        .select(["id", "provider"])
        .where_in(
            "provider",
            scopes
                .keys()
                .map(|provider| Value::from(provider.as_str()))
                .collect(),
        )
        .where_not_null("upstream_name")
        .r#where("created_at", "<", window.1)
        .where_nested(|query| {
            query
                .where_null("deleted_at")
                .or_where("deleted_at", ">", window.0)
        });
    let mut rows = query.cursor();
    while let Some(row) = rows.try_next().await? {
        let provider = text(&row, "provider")?;
        let Some(currencies) = scopes.get(&provider) else {
            continue;
        };
        let name = format!("cachedContents/{}", text(&row, "id")?);
        if !observed.contains(&name) {
            for currency in currencies {
                totals
                    .entry((provider.clone(), currency.clone()))
                    .or_default()
                    .add(None)?;
            }
        }
    }
    Ok(totals)
}

fn record(
    totals: &mut Totals,
    scopes: &BTreeMap<String, BTreeSet<String>>,
    provider: String,
    events: &[Event],
    window: (i64, i64),
) -> DatabaseResult<()> {
    for currency in &scopes[&provider] {
        let amount = match estimate(events, window, currency) {
            ResourceEstimate::OutsideWindow => continue,
            ResourceEstimate::Pending => None,
            ResourceEstimate::Priced(value) => Some(value),
        };
        totals
            .entry((provider.clone(), currency.clone()))
            .or_default()
            .add(amount)?;
    }
    Ok(())
}

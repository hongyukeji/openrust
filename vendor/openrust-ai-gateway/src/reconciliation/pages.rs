use openrust::database::{Connection, DatabaseError, DatabaseResult, Transaction, query::Record};
use serde::{Deserialize, Serialize};

use crate::managed_keys::{integer, now, text};

use super::{CostReport, ImportResult, REPORTS, StoredReport, flag, insert_report};

const UPLOADS: &str = "ai_gateway_cost_uploads";
const PAGES: &str = "ai_gateway_cost_pages";

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    number: i64,
    total: i64,
}

impl Page {
    pub(super) fn valid(self) -> bool {
        self.number >= 1 && self.number <= self.total
    }
}

#[derive(Serialize)]
pub(super) struct Progress {
    received: u64,
    total: u64,
    received_pages: Vec<i64>,
}

pub(super) async fn import(
    transaction: &mut Transaction,
    connection: &Connection,
    report: CostReport,
    actor: &str,
) -> DatabaseResult<ImportResult> {
    let id = report.report_id();
    let expected = report.page.map_or(0, |page| page.total);
    let saved = transaction
        .table(UPLOADS)
        .insert_or_ignore(&[Record::from([
            ("id".into(), id.as_str().into()),
            ("expected_pages".into(), expected.into()),
            ("complete".into(), report.complete.into()),
        ])])
        .await?;
    let upload = transaction
        .table(UPLOADS)
        .select(["expected_pages", "complete"])
        .r#where("id", "=", id.as_str())
        .lock_for_update()
        .first()
        .await?
        .ok_or(DatabaseError::InvalidQuery("AI 网关账单分批记录未保存"))?;
    let known_total = integer(&upload, "expected_pages")?.ok_or(DatabaseError::InvalidScalar)?;
    let declared_complete = flag(&upload, "complete")?;
    if report.page.is_none() && known_total == 0 {
        return insert_report(transaction, connection, &report, actor).await;
    }
    let finalized = transaction
        .table(REPORTS)
        .r#where("id", "=", id.as_str())
        .exists()
        .await?;
    let Some(page) = report.page else {
        if finalized {
            let mut result = insert_report(transaction, connection, &report, actor).await?;
            result.pagination = Some(progress(transaction, &id, known_total).await?);
            return Ok(result);
        }
        return Err(DatabaseError::InvalidQuery(
            "请继续提交现有账单版本的缺失分页",
        ));
    };
    if expected != known_total || report.complete != declared_complete || (saved != 0 && finalized)
    {
        return Err(DatabaseError::InvalidQuery(
            "AI 网关账单分批元数据与现有版本不一致",
        ));
    }

    let payload = serde_json::to_string(&report).map_err(|_| DatabaseError::InvalidScalar)?;
    let inserted = transaction
        .table(PAGES)
        .insert_or_ignore(&[Record::from([
            ("report_id".into(), id.as_str().into()),
            ("page_number".into(), page.number.into()),
            ("payload".into(), payload.as_str().into()),
            ("actor".into(), actor.into()),
            ("imported_at".into(), now().into()),
        ])])
        .await?;
    if inserted == 0 {
        let prior = transaction
            .table(PAGES)
            .select(["payload"])
            .r#where("report_id", "=", id.as_str())
            .r#where("page_number", "=", page.number)
            .first()
            .await?
            .ok_or(DatabaseError::InvalidScalar)?;
        if text(&prior, "payload")? != payload {
            return Err(DatabaseError::InvalidQuery("AI 网关同一账单分页的内容不同"));
        }
    }
    let progress = progress(transaction, &id, known_total).await?;
    if progress.received != progress.total {
        return Ok(ImportResult {
            report_id: id,
            complete: false,
            duplicate: inserted == 0,
            entries: Vec::new(),
            totals: Vec::new(),
            pagination: Some(progress),
        });
    }

    let mut assembled = report;
    assembled.page = None;
    assembled.entries.clear();
    assembled.totals.clear();
    let parts = transaction
        .table(PAGES)
        .select(["payload"])
        .r#where("report_id", "=", id.as_str())
        .order_by("page_number", "asc")
        .get()
        .await?;
    for row in &parts {
        let part: CostReport = serde_json::from_str(&text(row, "payload")?)
            .map_err(|_| DatabaseError::InvalidScalar)?;
        assembled.entries.extend(part.entries);
        assembled.totals.extend(part.totals);
    }
    assembled.validate(actor)?;
    let mut result = insert_report(transaction, connection, &assembled, actor).await?;
    result.pagination = Some(progress);
    Ok(result)
}

async fn progress(transaction: &mut Transaction, id: &str, total: i64) -> DatabaseResult<Progress> {
    let received_pages: Vec<_> = transaction
        .table(PAGES)
        .r#where("report_id", "=", id)
        .select(["page_number"])
        .order_by("page_number", "asc")
        .get()
        .await?
        .iter()
        .map(|row| integer(row, "page_number")?.ok_or(DatabaseError::InvalidScalar))
        .collect::<DatabaseResult<_>>()?;
    if received_pages
        .iter()
        .any(|number| !(1..=total).contains(number))
    {
        return Err(DatabaseError::InvalidScalar);
    }
    Ok(Progress {
        received: received_pages.len() as u64,
        total: u64::try_from(total).map_err(|_| DatabaseError::InvalidScalar)?,
        received_pages,
    })
}

pub(super) async fn read_progress(
    transaction: &mut Transaction,
    id: &str,
) -> DatabaseResult<Option<Progress>> {
    let Some(upload) = transaction
        .table(UPLOADS)
        .select(["expected_pages"])
        .r#where("id", "=", id)
        .lock_for_update()
        .first()
        .await?
    else {
        return Ok(None);
    };
    let total = integer(&upload, "expected_pages")?.ok_or(DatabaseError::InvalidScalar)?;
    if total == 0 {
        return Ok(None);
    }
    progress(transaction, id, total).await.map(Some)
}

pub(super) async fn pending(
    transaction: &mut Transaction,
    id: String,
    progress: Progress,
) -> DatabaseResult<StoredReport> {
    let first = transaction
        .table(PAGES)
        .select(["payload", "actor", "imported_at"])
        .r#where("report_id", "=", id.as_str())
        .order_by("page_number", "asc")
        .first()
        .await?
        .ok_or(DatabaseError::InvalidScalar)?;
    let report: CostReport = serde_json::from_str(&text(&first, "payload")?)
        .map_err(|_| DatabaseError::InvalidScalar)?;
    Ok(StoredReport {
        report_id: id,
        source: report.source,
        statement_id: report.statement_id,
        revision: report.revision,
        complete: false,
        starts_at: report.starts_at,
        ends_at: report.ends_at,
        actor: text(&first, "actor")?,
        imported_at: integer(&first, "imported_at")?.ok_or(DatabaseError::InvalidScalar)?,
        entries: Vec::new(),
        totals: Vec::new(),
        pagination: Some(progress),
    })
}

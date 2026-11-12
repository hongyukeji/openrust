use chrono::{DateTime, Timelike, Utc};
use serde_json::{Map, Value as Json};

use crate::{
    database::{
        Connection, DatabaseError, DatabaseResult, Row, Transaction, Value,
        query::{Builder, Record},
    },
    queue::{Payload, QueueError, QueueResult, backend, new_id},
};

use super::{Batch, BatchPhase, BatchTransition};

/// Persists queue batches in a configured SQL table.
#[derive(Clone)]
pub struct DatabaseBatchRepository {
    connection: Connection,
    table: String,
    jobs_table: String,
    outbox: String,
}

#[allow(non_snake_case, reason = "Matches Laravel batch repository methods")]
impl DatabaseBatchRepository {
    pub fn new(connection: Connection, table: impl Into<String>) -> QueueResult<Self> {
        let table = table.into();
        connection.queue_table(&table).map_err(backend)?;
        let jobs_table = format!("{table}_jobs");
        connection.queue_table(&jobs_table).map_err(backend)?;
        let outbox = format!("{table}_outbox");
        connection.queue_table(&outbox).map_err(backend)?;
        Ok(Self {
            connection,
            table,
            jobs_table,
            outbox,
        })
    }

    pub async fn store(
        &self,
        name: impl Into<String>,
        options: Map<String, Json>,
    ) -> QueueResult<Batch> {
        let batch = new_batch(name.into(), options, 0)?;
        self.connection
            .table(&self.table)
            .insert(&[insert_values(&batch).map_err(backend)?])
            .await
            .map_err(backend)?;
        Ok(batch)
    }

    pub(crate) fn uses_connection(&self, connection: &Connection) -> bool {
        self.connection.same_query_source(connection)
    }

    pub(crate) async fn begin(&self) -> QueueResult<Transaction> {
        self.connection.beginTransaction().await.map_err(backend)
    }

    pub(crate) async fn store_in_transaction(
        &self,
        transaction: &mut Transaction,
        batch: &Batch,
    ) -> QueueResult<()> {
        transaction
            .table(&self.table)
            .insert(&[insert_values(batch).map_err(backend)?])
            .await
            .map_err(backend)?;
        Ok(())
    }

    pub(crate) async fn stage_publication(
        &self,
        transaction: &mut Transaction,
        batch: &str,
        connection: &str,
        queue: &str,
        payloads: &[Payload],
    ) -> QueueResult<()> {
        let payloads = serde_json::to_string(payloads).map_err(|_| QueueError::InvalidPayload)?;
        transaction
            .table(&self.outbox)
            .insert(&[[
                ("batch_id".into(), batch.into()),
                ("connection".into(), connection.into()),
                ("queue".into(), queue.into()),
                ("payloads".into(), payloads.into()),
            ]
            .into_iter()
            .collect()])
            .await
            .map_err(backend)?;
        Ok(())
    }

    pub(crate) async fn pending_publications(&self, limit: u64) -> QueueResult<Vec<String>> {
        self.connection
            .table(&self.outbox)
            .useWritePdo()
            .orderBy("batch_id", "asc")
            .limit(limit)
            .get()
            .await
            .map_err(backend)?
            .iter()
            .map(|row| outbox_text(row, "batch_id").map(str::to_owned))
            .collect()
    }

    pub(crate) async fn claim_publication(
        &self,
        transaction: &mut Transaction,
        id: &str,
    ) -> QueueResult<Option<BatchPublication>> {
        // SQLite has no row-level FOR UPDATE. Acquire the write reservation before reading.
        if self.connection.is_sqlite() {
            transaction
                .table(&self.outbox)
                .r#where("batch_id", "=", id)
                .update([("batch_id".into(), id.into())].into_iter().collect())
                .await
                .map_err(backend)?;
        }
        transaction
            .table(&self.outbox)
            .r#where("batch_id", "=", id)
            .lockForUpdate()
            .first()
            .await
            .map_err(backend)?
            .map(|row| {
                Ok(BatchPublication {
                    connection: outbox_text(&row, "connection")?.into(),
                    queue: outbox_text(&row, "queue")?.into(),
                    payloads: serde_json::from_str(outbox_text(&row, "payloads")?)
                        .map_err(|_| QueueError::InvalidPayload)?,
                })
            })
            .transpose()
    }

    pub(crate) async fn find_in_transaction(
        &self,
        transaction: &mut Transaction,
        id: &str,
    ) -> QueueResult<Option<Batch>> {
        transaction
            .table(&self.table)
            .r#where("id", "=", id)
            .first()
            .await
            .map_err(backend)?
            .map(decode)
            .transpose()
            .map_err(backend)
    }

    pub(crate) async fn acknowledge_publication(
        &self,
        transaction: &mut Transaction,
        id: &str,
    ) -> QueueResult<()> {
        transaction
            .table(&self.outbox)
            .r#where("batch_id", "=", id)
            .delete()
            .await
            .map_err(backend)?;
        Ok(())
    }

    pub(crate) async fn recorded(
        &self,
        id: &str,
        job: &str,
        delivery: &str,
    ) -> QueueResult<Option<bool>> {
        let row = self
            .connection
            .table(&self.jobs_table)
            .useWritePdo()
            .r#where("batch_id", "=", id)
            .r#where("job_id", "=", job)
            .whereNested(|query| {
                query
                    .r#where("delivery_id", "=", delivery)
                    .orWhere("succeeded", "=", true)
            })
            .orderByDesc("succeeded")
            .first()
            .await
            .map_err(backend)?;
        row.as_ref().map(succeeded).transpose().map_err(backend)
    }

    pub(crate) async fn record(
        &self,
        id: &str,
        job: &str,
        delivery: &str,
        succeeded: bool,
    ) -> QueueResult<Option<BatchTransition>> {
        let repository = self.clone();
        let (id, job, delivery) = (id.to_owned(), job.to_owned(), delivery.to_owned());
        self.connection
            .transaction(
                move |transaction| {
                    let repository = repository.clone();
                    let (id, job, delivery) = (id.clone(), job.clone(), delivery.clone());
                    Box::pin(async move {
                        let Some(row) = transaction
                            .table(&repository.table)
                            .r#where("id", "=", id.as_str())
                            .lockForUpdate()
                            .first()
                            .await?
                        else {
                            return Ok(None);
                        };
                        let previous = transaction
                            .table(&repository.jobs_table)
                            .r#where("batch_id", "=", id.as_str())
                            .r#where("job_id", "=", job.as_str())
                            .whereNested(|query| {
                                query
                                    .r#where("delivery_id", "=", delivery.as_str())
                                    .orWhere("succeeded", "=", true)
                            })
                            .lockForUpdate()
                            .first()
                            .await?;
                        if previous.is_some() {
                            return Ok(None);
                        }
                        let mut batch = decode(row)?;
                        let first =
                            batch.pending_jobs == batch.total_jobs && batch.failed_jobs == 0;
                        let was_terminal = all_jobs_terminal(&batch);
                        let mut phases = Vec::new();
                        let started;
                        let mut canceled = false;
                        if succeeded {
                            phases.push(BatchPhase::Progress);
                            batch.pending_jobs =
                                batch.pending_jobs.checked_sub(1).ok_or(invalid())?;
                            batch.failed_job_ids.retain(|failed| failed != &job);
                            started = first.then(|| batch.clone());
                            if batch.pending_jobs == 0 {
                                batch.finished_at = Some(now());
                                phases.push(BatchPhase::Then);
                            }
                        } else {
                            batch.failed_jobs = increment(batch.failed_jobs, 1)?;
                            if !batch.failed_job_ids.contains(&job) {
                                batch.failed_job_ids.push(job.clone());
                            }
                            if batch.allowsFailures() {
                                phases.extend([BatchPhase::Progress, BatchPhase::Failure]);
                            }
                            if batch.failed_jobs == 1 {
                                phases.push(BatchPhase::Catch);
                            }
                            started = first.then(|| batch.clone());
                            if !batch.allowsFailures() && !batch.cancelled() {
                                canceled = true;
                                let now = now();
                                batch.cancelled_at = Some(now);
                                batch.finished_at = Some(now);
                            }
                        }
                        if !was_terminal && all_jobs_terminal(&batch) {
                            phases.push(BatchPhase::Finally);
                        }
                        let values = [
                            ("batch_id".into(), id.as_str().into()),
                            ("job_id".into(), job.as_str().into()),
                            ("delivery_id".into(), delivery.as_str().into()),
                            ("succeeded".into(), succeeded.into()),
                        ]
                        .into_iter()
                        .collect();
                        transaction
                            .table(&repository.jobs_table)
                            .insert(&[values])
                            .await?;
                        transaction
                            .table(&repository.table)
                            .r#where("id", "=", id.as_str())
                            .update(state(&batch)?)
                            .await?;
                        Ok(Some(BatchTransition {
                            batch,
                            phases,
                            started,
                            canceled,
                        }))
                    })
                },
                10,
            )
            .await
            .map_err(backend)
    }

    pub async fn find(&self, id: &str) -> QueueResult<Option<Batch>> {
        self.connection
            .table(&self.table)
            .useWritePdo()
            .r#where("id", "=", id)
            .first()
            .await
            .map_err(backend)?
            .map(decode)
            .transpose()
            .map_err(backend)
    }

    pub async fn incrementTotalJobs(&self, id: &str, amount: u32) -> QueueResult<Option<Batch>> {
        self.update(id, move |batch| {
            batch.total_jobs = increment(batch.total_jobs, amount)?;
            batch.pending_jobs = increment(batch.pending_jobs, amount)?;
            batch.finished_at = None;
            Ok(())
        })
        .await
    }

    pub async fn decrementPendingJobs(&self, id: &str, job_id: &str) -> QueueResult<Option<Batch>> {
        let job_id = job_id.to_owned();
        self.update(id, move |batch| {
            batch.pending_jobs = batch.pending_jobs.checked_sub(1).ok_or(invalid())?;
            batch.failed_job_ids.retain(|failed| failed != &job_id);
            Ok(())
        })
        .await
    }

    pub async fn incrementFailedJobs(&self, id: &str, job_id: &str) -> QueueResult<Option<Batch>> {
        let job_id = job_id.to_owned();
        self.update(id, move |batch| {
            batch.failed_jobs = increment(batch.failed_jobs, 1)?;
            if !batch.failed_job_ids.contains(&job_id) {
                batch.failed_job_ids.push(job_id.clone());
            }
            Ok(())
        })
        .await
    }

    pub async fn markAsFinished(&self, id: &str) -> QueueResult<Option<Batch>> {
        self.update(id, |batch| {
            batch.finished_at = Some(now());
            Ok(())
        })
        .await
    }

    pub async fn cancel(&self, id: &str) -> QueueResult<Option<Batch>> {
        self.cancel_once(id).await.map(|(batch, _)| batch)
    }

    pub(crate) async fn cancel_once(&self, id: &str) -> QueueResult<(Option<Batch>, bool)> {
        let (table, id) = (self.table.clone(), id.to_owned());
        let now = now().timestamp();
        self.connection
            .transaction(
                move |transaction| {
                    let (table, id) = (table.clone(), id.clone());
                    Box::pin(async move {
                        let changed = transaction
                            .table(&table)
                            .r#where("id", "=", id.as_str())
                            .whereNull("cancelled_at")
                            .update(
                                [
                                    ("cancelled_at".into(), now.into()),
                                    ("finished_at".into(), now.into()),
                                ]
                                .into_iter()
                                .collect(),
                            )
                            .await?
                            != 0;
                        let batch = transaction
                            .table(&table)
                            .r#where("id", "=", id.as_str())
                            .first()
                            .await?
                            .map(decode)
                            .transpose()?;
                        Ok((batch, changed))
                    })
                },
                10,
            )
            .await
            .map_err(backend)
    }

    pub async fn delete(&self, id: &str) -> QueueResult<bool> {
        self.connection
            .table(&self.table)
            .r#where("id", "=", id)
            .delete()
            .await
            .map(|rows| rows != 0)
            .map_err(backend)
    }

    pub async fn prune(&self, before: DateTime<Utc>) -> QueueResult<u64> {
        self.prune_matching(|query| {
            query
                .whereNotNull("finished_at")
                .r#where("finished_at", "<", before.timestamp())
        })
        .await
    }

    pub async fn pruneUnfinished(&self, before: DateTime<Utc>) -> QueueResult<u64> {
        self.prune_matching(|query| {
            query
                .whereNull("finished_at")
                .r#where("created_at", "<", before.timestamp())
        })
        .await
    }

    pub async fn pruneCancelled(&self, before: DateTime<Utc>) -> QueueResult<u64> {
        self.prune_matching(|query| {
            query
                .whereNotNull("cancelled_at")
                .r#where("created_at", "<", before.timestamp())
        })
        .await
    }

    async fn prune_matching(
        &self,
        predicate: impl Fn(Builder<'static>) -> Builder<'static>,
    ) -> QueueResult<u64> {
        let mut total = 0;
        loop {
            let count = predicate(self.connection.table(&self.table))
                .orderBy("id", "asc")
                .limit(1000)
                .delete()
                .await
                .map_err(backend)?;
            total += count;
            if count == 0 {
                return Ok(total);
            }
        }
    }

    async fn update(
        &self,
        id: &str,
        apply: impl Fn(&mut Batch) -> DatabaseResult<()> + Clone + Send + 'static,
    ) -> QueueResult<Option<Batch>> {
        let table = self.table.clone();
        let id = id.to_owned();
        self.connection
            .transaction(
                move |transaction| {
                    let table = table.clone();
                    let id = id.clone();
                    let apply = apply.clone();
                    Box::pin(async move {
                        let Some(row) = transaction
                            .table(&table)
                            .r#where("id", "=", id.as_str())
                            .lockForUpdate()
                            .first()
                            .await?
                        else {
                            return Ok(None);
                        };
                        let mut batch = decode(row)?;
                        apply(&mut batch)?;
                        transaction
                            .table(&table)
                            .r#where("id", "=", id.as_str())
                            .update(state(&batch)?)
                            .await?;
                        Ok(Some(batch))
                    })
                },
                10,
            )
            .await
            .map_err(backend)
    }
}

pub(crate) struct BatchPublication {
    pub connection: String,
    pub queue: String,
    pub payloads: Vec<Payload>,
}

fn outbox_text<'a>(row: &'a Row, column: &str) -> QueueResult<&'a str> {
    match row.get(column) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(QueueError::InvalidPayload),
    }
}

fn all_jobs_terminal(batch: &Batch) -> bool {
    usize::try_from(batch.pending_jobs).is_ok_and(|pending| pending == batch.failed_job_ids.len())
}

pub(crate) fn new_batch(
    name: String,
    options: Map<String, Json>,
    count: u32,
) -> QueueResult<Batch> {
    Ok(Batch {
        id: new_id()?,
        name,
        total_jobs: count,
        pending_jobs: count,
        failed_jobs: 0,
        failed_job_ids: Vec::new(),
        options,
        cancelled_at: None,
        created_at: now(),
        finished_at: None,
    })
}

fn insert_values(batch: &Batch) -> DatabaseResult<Record> {
    let mut values = state(batch)?;
    values.extend([
        ("id".into(), batch.id.as_str().into()),
        ("name".into(), batch.name.as_str().into()),
        (
            "options".into(),
            Json::Object(batch.options.clone()).to_string().into(),
        ),
        ("created_at".into(), batch.created_at.timestamp().into()),
    ]);
    Ok(values)
}

fn succeeded(row: &Row) -> DatabaseResult<bool> {
    match row.get("succeeded") {
        Some(Value::Bool(true) | Value::Integer(1)) => Ok(true),
        Some(Value::Bool(false) | Value::Integer(0)) => Ok(false),
        _ => Err(invalid()),
    }
}

fn increment(value: u32, amount: u32) -> DatabaseResult<u32> {
    value
        .checked_add(amount)
        .filter(|value| i32::try_from(*value).is_ok())
        .ok_or(invalid())
}

fn state(batch: &Batch) -> DatabaseResult<Record> {
    let failed_ids = serde_json::to_string(&batch.failed_job_ids).map_err(|_| invalid())?;
    let time =
        |value: Option<DateTime<Utc>>| value.map_or(Value::Null, |date| date.timestamp().into());
    Ok([
        ("total_jobs".into(), i64::from(batch.total_jobs).into()),
        ("pending_jobs".into(), i64::from(batch.pending_jobs).into()),
        ("failed_jobs".into(), i64::from(batch.failed_jobs).into()),
        ("failed_job_ids".into(), failed_ids.into()),
        ("cancelled_at".into(), time(batch.cancelled_at)),
        ("finished_at".into(), time(batch.finished_at)),
    ]
    .into_iter()
    .collect())
}

fn decode(row: Row) -> DatabaseResult<Batch> {
    let text = |key| match row.get(key) {
        Some(Value::Text(value)) => Ok(value.as_str()),
        _ => Err(invalid()),
    };
    let count = |key| match row.get(key) {
        Some(Value::Integer(value)) => u32::try_from(*value).map_err(|_| invalid()),
        _ => Err(invalid()),
    };
    let time = |key| match row.get(key) {
        Some(Value::Integer(value)) => DateTime::from_timestamp(*value, 0)
            .map(Some)
            .ok_or(invalid()),
        Some(Value::Null) => Ok(None),
        _ => Err(invalid()),
    };
    let options = match row.get("options") {
        Some(Value::Null) => Map::new(),
        Some(Value::Text(value)) => serde_json::from_str(value).map_err(|_| invalid())?,
        _ => return Err(invalid()),
    };
    Ok(Batch {
        id: text("id")?.into(),
        name: text("name")?.into(),
        total_jobs: count("total_jobs")?,
        pending_jobs: count("pending_jobs")?,
        failed_jobs: count("failed_jobs")?,
        failed_job_ids: serde_json::from_str(text("failed_job_ids")?).map_err(|_| invalid())?,
        options,
        cancelled_at: time("cancelled_at")?,
        created_at: time("created_at")?.ok_or(invalid())?,
        finished_at: time("finished_at")?,
    })
}

fn now() -> DateTime<Utc> {
    Utc::now().with_nanosecond(0).expect("valid whole second")
}

const fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("job batch state")
}

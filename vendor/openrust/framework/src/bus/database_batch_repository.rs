use chrono::{DateTime, Timelike, Utc};
use serde_json::{Map, Value as Json};

use crate::{
    database::{
        Connection, DatabaseError, DatabaseResult, Row, Transaction, Value,
        query::{Builder, Record},
    },
    queue::{ChainUpdate, Payload, QueueError, QueueResult, backend, new_id},
};

use super::{Batch, BatchContinuation, BatchPhase, BatchTransition};

/// Persists queue batches in a configured SQL table.
#[derive(Clone)]
pub struct DatabaseBatchRepository {
    connection: Connection,
    table: String,
    jobs_table: String,
    outbox: String,
    additions: String,
    continuations: String,
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
        let additions = format!("{table}_additions");
        connection.queue_table(&additions).map_err(backend)?;
        let continuations = format!("{table}_continuations");
        connection.queue_table(&continuations).map_err(backend)?;
        Ok(Self {
            connection,
            table,
            jobs_table,
            outbox,
            additions,
            continuations,
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

    pub(crate) async fn store_continuation(
        &self,
        transaction: &mut Transaction,
        continuation: &BatchContinuation,
    ) -> QueueResult<()> {
        let next = serde_json::to_string(continuation.next.as_slice())
            .map_err(|_| QueueError::InvalidPayload)?;
        let catches =
            serde_json::to_string(&continuation.catches).map_err(|_| QueueError::InvalidPayload)?;
        transaction
            .table(&self.continuations)
            .insert(&[[
                ("batch_id".into(), continuation.id.as_str().into()),
                ("connection".into(), continuation.connection.as_str().into()),
                ("queue".into(), continuation.queue.as_str().into()),
                ("payloads".into(), next.into()),
                ("catches".into(), catches.into()),
            ]
            .into_iter()
            .collect()])
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn settle_continuation(
        &self,
        transaction: &mut Transaction,
        batch: &Batch,
        canceled_by_failure: bool,
    ) -> DatabaseResult<(bool, Vec<String>)> {
        let Some(row) = transaction
            .table(&self.continuations)
            .r#where("batch_id", "=", batch.id.as_str())
            .first()
            .await?
        else {
            return Ok((false, Vec::new()));
        };
        let catches = if canceled_by_failure {
            serde_json::from_str(outbox_text(&row, "catches").map_err(|_| invalid())?)
                .map_err(|_| invalid())?
        } else {
            Vec::new()
        };
        let ready =
            !batch.cancelled() && outbox_text(&row, "payloads").map_err(|_| invalid())? != "[]";
        let query =
            transaction
                .table(&self.continuations)
                .r#where("batch_id", "=", batch.id.as_str());
        if ready {
            query
                .update([("ready".into(), true.into())].into_iter().collect())
                .await?;
        } else {
            query.delete().await?;
        }
        Ok((ready, catches))
    }

    pub(crate) async fn stage_publication(
        &self,
        transaction: &mut Transaction,
        publication: &PublicationKey,
        batch: &str,
        connection: &str,
        queue: &str,
        payloads: &[Payload],
    ) -> QueueResult<()> {
        let payloads = serde_json::to_string(payloads).map_err(|_| QueueError::InvalidPayload)?;
        let (table, column) = self.publication_table(publication);
        let mut row: Record = [
            ("batch_id".into(), batch.into()),
            ("connection".into(), connection.into()),
            ("queue".into(), queue.into()),
            ("payloads".into(), payloads.into()),
        ]
        .into_iter()
        .collect();
        if matches!(publication, PublicationKey::Addition(_)) {
            row.insert(column.into(), publication.id().into());
        }
        transaction
            .table(table)
            .insert(&[row])
            .await
            .map_err(backend)?;
        Ok(())
    }

    fn publication_table(&self, publication: &PublicationKey) -> (&str, &'static str) {
        match publication {
            PublicationKey::Initial(_) => (&self.outbox, "batch_id"),
            PublicationKey::Addition(_) => (&self.additions, "id"),
            PublicationKey::Continuation(_) => (&self.continuations, "batch_id"),
        }
    }

    pub(crate) async fn pending_publications(
        &self,
        limit: u64,
    ) -> QueueResult<Vec<PublicationKey>> {
        let mut keys = Vec::new();
        let initial = self
            .connection
            .table(&self.outbox)
            .useWritePdo()
            .orderBy("batch_id", "asc")
            .limit(limit)
            .get()
            .await
            .map_err(backend)?;
        for row in initial {
            keys.push(PublicationKey::Initial(
                outbox_text(&row, "batch_id")?.into(),
            ));
        }
        let remaining = limit.saturating_sub(keys.len() as u64);
        if remaining != 0 {
            let additions = self
                .connection
                .table(&self.additions)
                .useWritePdo()
                .orderBy("id", "asc")
                .limit(remaining)
                .get()
                .await
                .map_err(backend)?;
            for row in additions {
                keys.push(PublicationKey::Addition(outbox_text(&row, "id")?.into()));
            }
        }
        let remaining = limit.saturating_sub(keys.len() as u64);
        if remaining != 0 {
            let continuations = self
                .connection
                .table(&self.continuations)
                .useWritePdo()
                .r#where("ready", "=", true)
                .orderBy("batch_id", "asc")
                .limit(remaining)
                .get()
                .await
                .map_err(backend)?;
            for row in continuations {
                keys.push(PublicationKey::Continuation(
                    outbox_text(&row, "batch_id")?.into(),
                ));
            }
        }
        Ok(keys)
    }

    pub(crate) async fn claim_publication(
        &self,
        transaction: &mut Transaction,
        publication: &PublicationKey,
    ) -> QueueResult<Option<BatchPublication>> {
        let (table, column) = self.publication_table(publication);
        let id = publication.id();
        if matches!(publication, PublicationKey::Continuation(_)) && !self.connection.is_sqlite() {
            // Batch state transitions lock the batch before touching its continuation.
            transaction
                .table(&self.table)
                .r#where("id", "=", id)
                .lockForUpdate()
                .first()
                .await
                .map_err(backend)?;
        }
        // SQLite acquires a write reservation before its read.
        if self.connection.is_sqlite() {
            publication
                .restrict(transaction.table(table).r#where(column, "=", id))
                .update([(column.into(), id.into())].into_iter().collect())
                .await
                .map_err(backend)?;
        }
        publication
            .restrict(transaction.table(table).r#where(column, "=", id))
            .lockForUpdate()
            .first()
            .await
            .map_err(backend)?
            .map(|row| {
                Ok(BatchPublication {
                    batch_id: outbox_text(&row, "batch_id")?.into(),
                    connection: outbox_text(&row, "connection")?.into(),
                    queue: outbox_text(&row, "queue")?.into(),
                    payloads: serde_json::from_str(outbox_text(&row, "payloads")?)
                        .map_err(|_| QueueError::InvalidPayload)?,
                })
            })
            .transpose()
    }

    pub(crate) async fn initial_publication_pending(
        &self,
        transaction: &mut Transaction,
        batch_id: &str,
    ) -> QueueResult<bool> {
        transaction
            .table(&self.outbox)
            .r#where("batch_id", "=", batch_id)
            .exists()
            .await
            .map_err(backend)
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
        publication: &PublicationKey,
    ) -> QueueResult<()> {
        let (table, column) = self.publication_table(publication);
        transaction
            .table(table)
            .r#where(column, "=", publication.id())
            .delete()
            .await
            .map_err(backend)?;
        Ok(())
    }

    pub(crate) async fn append_in_transaction(
        &self,
        transaction: &mut Transaction,
        id: &str,
        count: u32,
    ) -> QueueResult<Option<Batch>> {
        if count == 0 {
            return self.find_in_transaction(transaction, id).await;
        }
        let changed = transaction
            .table(&self.table)
            .r#where("id", "=", id)
            .r#where("total_jobs", "<=", i64::from(i32::MAX) - i64::from(count))
            .increment(
                "total_jobs",
                i64::from(count),
                [("finished_at".into(), Value::Null)].into_iter().collect(),
            )
            .await
            .map_err(backend)?;
        if changed == 0 {
            return if self.find_in_transaction(transaction, id).await?.is_some() {
                Err(QueueError::InvalidPayload)
            } else {
                Ok(None)
            };
        }
        transaction
            .table(&self.table)
            .r#where("id", "=", id)
            .increment("pending_jobs", i64::from(count), Record::new())
            .await
            .map_err(backend)?;
        self.find_in_transaction(transaction, id).await
    }

    pub(crate) async fn recorded(
        &self,
        id: &str,
        job: &str,
        delivery: &str,
    ) -> QueueResult<Option<RecordedBatchJob>> {
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
        row.as_ref()
            .map(|row| {
                let successor = match row.get("chain_successor") {
                    Some(Value::Text(raw)) => Some(Payload::decode(raw)?),
                    Some(Value::Null) | None => None,
                    _ => return Err(QueueError::InvalidPayload),
                };
                Ok(RecordedBatchJob {
                    succeeded: succeeded(row).map_err(backend)?,
                    successor,
                })
            })
            .transpose()
    }

    pub(crate) async fn record(
        &self,
        id: &str,
        job: &str,
        delivery: &str,
        succeeded: bool,
        edit: Option<&ChainUpdate>,
    ) -> QueueResult<Option<BatchTransition>> {
        let added = edit.map_or(0, |edit| edit.added);
        let successor = edit.map(|edit| edit.next.encode()).transpose()?;
        let repository = self.clone();
        let (id, job, delivery) = (id.to_owned(), job.to_owned(), delivery.to_owned());
        self.connection
            .transaction(
                move |transaction| {
                    let repository = repository.clone();
                    let (id, job, delivery) = (id.clone(), job.clone(), delivery.clone());
                    let successor = successor.clone();
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
                            batch.total_jobs = increment(batch.total_jobs, added)?;
                            batch.pending_jobs = increment(
                                batch.pending_jobs.checked_sub(1).ok_or(invalid())?,
                                added,
                            )?;
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
                        let terminal = !was_terminal && all_jobs_terminal(&batch);
                        if terminal {
                            phases.push(BatchPhase::Finally);
                        }
                        let (continuation_ready, chain_catches) = if canceled || terminal {
                            repository
                                .settle_continuation(transaction, &batch, canceled)
                                .await?
                        } else {
                            (false, Vec::new())
                        };
                        let mut values: Record = [
                            ("batch_id".into(), id.as_str().into()),
                            ("job_id".into(), job.as_str().into()),
                            ("delivery_id".into(), delivery.as_str().into()),
                            ("succeeded".into(), succeeded.into()),
                        ]
                        .into_iter()
                        .collect();
                        if let Some(successor) = successor {
                            values.insert("chain_successor".into(), successor.into());
                        }
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
                            continuation_ready,
                            chain_catches,
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
        let continuations = self.continuations.clone();
        let now = now().timestamp();
        self.connection
            .transaction(
                move |transaction| {
                    let (table, id) = (table.clone(), id.clone());
                    let continuations = continuations.clone();
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
                        if changed {
                            transaction
                                .table(&continuations)
                                .r#where("batch_id", "=", id.as_str())
                                .delete()
                                .await?;
                        }
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
            let ready = self
                .connection
                .table(&self.continuations)
                .whereColumn(
                    &format!("{}.batch_id", self.continuations),
                    "=",
                    &format!("{}.id", self.table),
                )
                .r#where("ready", "=", true);
            let count = predicate(self.connection.table(&self.table))
                .whereNotExists(&ready)
                .map_err(backend)?
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

pub(crate) struct RecordedBatchJob {
    pub(crate) succeeded: bool,
    pub(crate) successor: Option<Payload>,
}

pub(crate) enum PublicationKey {
    Initial(String),
    Addition(String),
    Continuation(String),
}

impl PublicationKey {
    fn restrict<'a>(&self, query: Builder<'a>) -> Builder<'a> {
        if matches!(self, Self::Continuation(_)) {
            query.r#where("ready", "=", true)
        } else {
            query
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Initial(id) | Self::Addition(id) | Self::Continuation(id) => id,
        }
    }
}

pub(crate) struct BatchPublication {
    pub batch_id: String,
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

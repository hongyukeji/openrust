use chrono::{DateTime, Timelike, Utc};
use serde_json::{Map, Value as Json};

use crate::{
    database::{Connection, DatabaseError, DatabaseResult, Row, Value, query::Record},
    queue::{QueueResult, backend, new_id},
};

use super::Batch;

/// Persists queue batches in a configured SQL table.
#[derive(Clone)]
pub struct DatabaseBatchRepository {
    connection: Connection,
    table: String,
}

#[allow(non_snake_case, reason = "Matches Laravel batch repository methods")]
impl DatabaseBatchRepository {
    pub fn new(connection: Connection, table: impl Into<String>) -> QueueResult<Self> {
        let table = table.into();
        connection.queue_table(&table).map_err(backend)?;
        Ok(Self { connection, table })
    }

    pub async fn store(
        &self,
        name: impl Into<String>,
        options: Map<String, Json>,
    ) -> QueueResult<Batch> {
        let batch = Batch {
            id: new_id()?,
            name: name.into(),
            total_jobs: 0,
            pending_jobs: 0,
            failed_jobs: 0,
            failed_job_ids: Vec::new(),
            options,
            cancelled_at: None,
            created_at: now(),
            finished_at: None,
        };
        let mut values = state(&batch).map_err(backend)?;
        values.extend([
            ("id".into(), batch.id.as_str().into()),
            ("name".into(), batch.name.as_str().into()),
            (
                "options".into(),
                Json::Object(batch.options.clone()).to_string().into(),
            ),
            ("created_at".into(), batch.created_at.timestamp().into()),
        ]);
        self.connection
            .table(&self.table)
            .insert(&[values])
            .await
            .map_err(backend)?;
        Ok(batch)
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
        self.update(id, |batch| {
            let now = now();
            batch.cancelled_at = Some(now);
            batch.finished_at = Some(now);
            Ok(())
        })
        .await
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

use std::time::Duration;

use crate::database::{Connection, Row, Transaction, Value};

use super::{FailedJob, FailedJobProviderInterface};
use crate::queue::{Payload, QueueError, QueueFuture, QueueResult, backend};

/// The database-uuids failed-job provider.
pub struct DatabaseUuidFailedJobProvider {
    connection: Connection,
    table: String,
}

impl DatabaseUuidFailedJobProvider {
    pub(crate) fn new(connection: Connection, table: String) -> QueueResult<Self> {
        connection.queue_table(&table).map_err(backend)?;
        Ok(Self { connection, table })
    }
}

impl FailedJobProviderInterface for DatabaseUuidFailedJobProvider {
    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(async move {
            let values = [
                ("uuid".into(), job.id.as_str().into()),
                ("connection".into(), job.connection.as_str().into()),
                ("queue".into(), job.queue.as_str().into()),
                ("payload".into(), job.payload.encode()?.into()),
                ("exception".into(), job.exception.as_str().into()),
                ("failed_at".into(), job.failed_at.as_str().into()),
            ]
            .into_iter()
            .collect();
            self.connection
                .table(&self.table)
                .upsert(
                    &[values],
                    &["uuid"],
                    Some(&["payload", "exception", "failed_at"]),
                )
                .await
                .map_err(backend)?;
            Ok(Some(job.id.clone()))
        })
    }

    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>> {
        Box::pin(async move {
            self.connection
                .table(&self.table)
                .useWritePdo()
                .orderByDesc("id")
                .get()
                .await
                .map_err(backend)?
                .into_iter()
                .map(decode)
                .collect()
        })
    }

    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>> {
        Box::pin(async move {
            self.connection
                .table(&self.table)
                .useWritePdo()
                .r#where("uuid", "=", id)
                .first()
                .await
                .map_err(backend)?
                .map(decode)
                .transpose()
        })
    }

    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>> {
        Box::pin(async move {
            Ok(self
                .connection
                .table(&self.table)
                .r#where("uuid", "=", id)
                .delete()
                .await
                .map_err(backend)?
                > 0)
        })
    }

    fn forget_retry<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<bool>> {
        Box::pin(async move {
            Ok(self
                .connection
                .table(&self.table)
                .r#where("uuid", "=", job.id.as_str())
                .r#where("payload", "=", job.payload.encode()?)
                .delete()
                .await
                .map_err(backend)?
                > 0)
        })
    }
    fn forget_retry_in_transaction<'a>(
        &'a self,
        job: &'a FailedJob,
        transaction: &'a mut Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<bool>>> {
        Box::pin(async move {
            if !transaction.belongs_to(&self.connection) {
                return Ok(None);
            }
            let deleted = transaction
                .table(&self.table)
                .r#where("uuid", "=", job.id.as_str())
                .r#where("payload", "=", job.payload.encode()?)
                .delete()
                .await
                .map_err(backend)?;
            Ok(Some(deleted != 0))
        })
    }

    fn flush(&self, older_than: Option<Duration>) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            let mut query = self.connection.table(&self.table);
            if let Some(age) = older_than {
                let cutoff = chrono::Utc::now()
                    .checked_sub_signed(
                        chrono::Duration::from_std(age)
                            .map_err(|_| QueueError::InvalidConfiguration)?,
                    )
                    .ok_or(QueueError::InvalidConfiguration)?;
                query = query.r#where("failed_at", "<=", super::timestamp(cutoff));
            }
            query.delete().await.map_err(backend)?;
            Ok(())
        })
    }
}

fn decode(row: Row) -> QueueResult<FailedJob> {
    let text = |key| match row.get(key) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(QueueError::InvalidPayload),
    };
    Ok(FailedJob {
        id: text("uuid")?,
        connection: text("connection")?,
        queue: text("queue")?,
        payload: Payload::decode(&text("payload")?)?,
        exception: text("exception")?,
        failed_at: text("failed_at")?,
    })
}

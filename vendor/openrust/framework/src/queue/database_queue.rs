use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use crate::{
    contracts::queue::{Job, Queue as Contract},
    database::{Connection, Row, Transaction, Value},
};

use super::{
    Delay, JobContext, Payload, Queue, QueueError, QueueFuture, QueueResult, backend,
    jobs::{Reservation, ReservedJob},
    queue_name,
};

/// Stores and reserves jobs using a database connection.
#[derive(Clone)]
pub struct DatabaseQueue {
    pub(super) name: String,
    pub(super) queue: String,
    pub(super) table: String,
    pub(super) retry_after: i64,
    pub(super) connection: Connection,
    pub(super) registry: Arc<Queue>,
}

impl DatabaseQueue {
    fn table_sql(&self) -> QueueResult<String> {
        self.connection.queue_table(&self.table).map_err(backend)
    }

    fn clock(&self) -> &'static str {
        if self.connection.is_sqlite() {
            "unixepoch()"
        } else {
            "unix_timestamp()"
        }
    }

    fn insert(
        &self,
        queue: &str,
        payload: String,
        attempts: u32,
        delay: Delay,
    ) -> QueueResult<(String, Vec<Value>)> {
        let table = self.table_sql()?;
        let clock = self.clock();
        let (available_at, seconds) = match delay {
            Delay::For(duration) => (format!("{clock} + ?"), seconds(duration)?),
            Delay::Until(time) => (
                "?".to_owned(),
                seconds(time.duration_since(UNIX_EPOCH).unwrap_or_default())?,
            ),
        };
        let sql = format!(
            "insert into {table} (queue, payload, attempts, reserved_at, available_at, created_at) values (?, ?, ?, null, {available_at}, {clock})"
        );
        Ok((
            sql,
            vec![
                queue.into(),
                payload.into(),
                i64::from(attempts).into(),
                seconds.into(),
            ],
        ))
    }

    async fn enqueue(
        &self,
        delay: Delay,
        payload: &Payload,
        queue: Option<&str>,
        transaction: Option<&mut Transaction>,
    ) -> QueueResult<Option<String>> {
        let queue = queue_name(queue, &self.queue)?;
        let (sql, bindings) = self.insert(queue, payload.encode()?, 0, delay)?;
        let id =
            if let Some(transaction) = transaction.filter(|tx| tx.belongs_to(&self.connection)) {
                transaction.insert_get_id(&sql, &bindings).await
            } else {
                self.connection.insert_get_id(&sql, &bindings).await
            }
            .map_err(backend)?;
        Ok(Some(number(&id)?.to_string()))
    }

    async fn reserve(&self, queue: &str) -> QueueResult<Option<Box<dyn Job>>> {
        let table = self.table_sql()?;
        let clock = self.clock();
        let ready = format!(
            "queue = ? and ((reserved_at is null and available_at <= {clock}) or reserved_at <= {clock} - ?)"
        );
        let bindings = [Value::from(queue), self.retry_after.into()];
        let row = if self.connection.is_sqlite() {
            // One write statement prevents a deferred read transaction from racing an upgrade.
            let sql = format!(
                "update {table} set reserved_at = {clock}, attempts = attempts + 1 where id = (select id from {table} where {ready} order by id limit 1) returning id, payload, attempts"
            );
            self.connection
                .selectFromWriteConnection(&sql, &bindings)
                .await
                .map_err(backend)?
                .into_iter()
                .next()
        } else {
            let mut transaction = self.connection.beginTransaction().await.map_err(backend)?;
            let sql = format!(
                "select id, payload, attempts from {table} where {ready} order by id limit 1 for update skip locked"
            );
            let row = transaction
                .selectOne(&sql, &bindings)
                .await
                .map_err(backend)?;
            if let Some(row) = &row {
                let id = field(row, "id")?.clone();
                let sql = format!(
                    "update {table} set reserved_at = {clock}, attempts = attempts + 1 where id = ?"
                );
                transaction
                    .affectingStatement(&sql, &[id])
                    .await
                    .map_err(backend)?;
            }
            transaction.commit().await.map_err(backend)?;
            row
        };
        let Some(row) = row else { return Ok(None) };
        let raw = match field(&row, "payload")? {
            Value::Text(raw) => raw.clone(),
            _ => return Err(QueueError::InvalidPayload),
        };
        let payload = Payload::decode(&raw)?;
        let id = field(&row, "id")?.clone();
        let previous = number(field(&row, "attempts")?)?;
        let attempts = if self.connection.is_sqlite() {
            previous
        } else {
            previous.checked_add(1).ok_or(QueueError::InvalidPayload)?
        };
        let attempts = u32::try_from(attempts).map_err(|_| QueueError::InvalidPayload)?;
        let storage = DatabaseReservation {
            driver: self.clone(),
            queue: queue.into(),
            id: id.clone(),
            attempts,
            raw,
        };
        Ok(Some(Box::new(ReservedJob {
            registry: self.registry.clone(),
            storage: Box::new(storage),
            id: number(&id)?.to_string(),
            context: JobContext {
                connection: self.name.clone(),
                queue: queue.into(),
                attempts,
                transaction: None,
                ..JobContext::default()
            },
            payload,
            deleted: false,
            released: false,
            failed: false,
        })))
    }

    fn deletion(&self, id: &Value, attempts: u32) -> QueueResult<(String, [Value; 3])> {
        let table = self.table_sql()?;
        let clock = self.clock();
        let sql = format!(
            "delete from {table} where id = ? and attempts = ? and reserved_at > {clock} - ?"
        );
        Ok((
            sql,
            [
                id.clone(),
                i64::from(attempts).into(),
                self.retry_after.into(),
            ],
        ))
    }
}

impl Contract for DatabaseQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }

    fn default_queue(&self) -> &str {
        &self.queue
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move {
            let queue = queue_name(queue, &self.queue)?;
            let sql = format!(
                "select count(*) as count from {} where queue = ?",
                self.table_sql()?
            );
            let rows = self
                .connection
                .selectFromWriteConnection(&sql, &[queue.into()])
                .await
                .map_err(backend)?;
            number(field(
                rows.first().ok_or(QueueError::InvalidPayload)?,
                "count",
            )?)
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.enqueue(Delay::For(Duration::ZERO), payload, queue, None))
    }

    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.enqueue(delay, payload, queue, None))
    }

    fn push_in_transaction<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
        transaction: &'a mut Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.enqueue(
            delay.unwrap_or(Delay::For(Duration::ZERO)),
            payload,
            queue,
            Some(transaction),
        ))
    }

    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async move { self.reserve(queue_name(queue, &self.queue)?).await })
    }
}

struct DatabaseReservation {
    driver: DatabaseQueue,
    queue: String,
    id: Value,
    attempts: u32,
    raw: String,
}

impl Reservation for DatabaseReservation {
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            let (sql, bindings) = self.driver.deletion(&self.id, self.attempts)?;
            let deleted = self
                .driver
                .connection
                .affectingStatement(&sql, &bindings)
                .await
                .map_err(backend)?;
            require_one(deleted)
        })
    }

    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            self.replace(
                &self.queue,
                self.raw.clone(),
                self.attempts,
                Delay::For(delay),
            )
            .await
        })
    }

    fn advance_chain<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.replace(
                queue_name(queue, &self.queue)?,
                next.encode()?,
                0,
                delay.unwrap_or(Delay::For(Duration::ZERO)),
            )
            .await
        })
    }
}

impl DatabaseReservation {
    async fn replace(
        &self,
        queue: &str,
        raw: String,
        attempts: u32,
        delay: Delay,
    ) -> QueueResult<()> {
        let (delete, delete_bindings) = self.driver.deletion(&self.id, self.attempts)?;
        let (insert, insert_bindings) = self.driver.insert(queue, raw, attempts, delay)?;
        let mut transaction = self
            .driver
            .connection
            .beginTransaction()
            .await
            .map_err(backend)?;
        let deleted = transaction
            .affectingStatement(&delete, &delete_bindings)
            .await
            .map_err(backend)?;
        require_one(deleted)?;
        transaction
            .statement(&insert, &insert_bindings)
            .await
            .map_err(backend)?;
        transaction.commit().await.map_err(backend)
    }
}

fn require_one(count: u64) -> QueueResult<()> {
    if count == 1 {
        Ok(())
    } else {
        Err(QueueError::LostReservation)
    }
}

fn field<'a>(row: &'a Row, name: &str) -> QueueResult<&'a Value> {
    row.get(name).ok_or(QueueError::InvalidPayload)
}

fn number(value: &Value) -> QueueResult<u64> {
    match *value {
        Value::Unsigned(value) => Ok(value),
        Value::Integer(value) => u64::try_from(value).map_err(|_| QueueError::InvalidPayload),
        _ => Err(QueueError::InvalidPayload),
    }
}

fn seconds(duration: Duration) -> QueueResult<i64> {
    let seconds = duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0));
    i64::try_from(seconds).map_err(|_| QueueError::InvalidPayload)
}

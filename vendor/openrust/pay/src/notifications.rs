use crate::{Error, ExpectedPayment, Result, VerifiedNotification};
use openrust::database::{
    Connection, DatabaseFuture, DatabaseResult, Transaction, Value,
    migrations::{Migration, MigrationContext},
};
use std::sync::Arc;

/// Durable deduplication for payment-success callbacks, sharing the business SQL transaction.
#[derive(Clone)]
pub struct DatabaseNotifications {
    connection: Connection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationOutcome {
    Processed,
    Duplicate,
}
impl DatabaseNotifications {
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }
    /// The handler must use this transaction and perform only transactional SQL work.
    /// External effects belong in the application's transactional outbox/after-commit path.
    pub async fn process(
        &self,
        notification: &VerifiedNotification,
        expected: &ExpectedPayment,
        handler: impl for<'a> Fn(
            &'a mut Transaction,
            &'a VerifiedNotification,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<NotificationOutcome> {
        notification.validate(expected)?;
        let notice = notification.clone();
        let handler = Arc::new(handler);
        let (duplicate, conflict) = self
            .connection
            .transaction(
                move |tx| {
                    let notice = notice.clone();
                    let handler = handler.clone();
                    Box::pin(async move {
                        let key = notice.key();
                        let fingerprint = notice.fingerprint();
                        let record = [
                            ("id".into(), Value::from(key.clone())),
                            ("fingerprint".into(), fingerprint.clone().into()),
                            ("status".into(), "pending".into()),
                        ]
                        .into_iter()
                        .collect();
                        // A no-op upsert acquires a write lock even on an existing row. On
                        // SQLite it acquires the write lock before reading; on MySQL this
                        // avoids upgrading the shared lock taken by INSERT IGNORE.
                        tx.table("payment_notifications")
                            .upsert(&[record], &["id"], Some(&["id"]))
                            .await?;
                        let stored = tx
                            .table("payment_notifications")
                            .r#where("id", "=", key.clone())
                            .lockForUpdate()
                            .value("fingerprint")
                            .await?;
                        if stored != Value::from(fingerprint) {
                            return Ok((false, true));
                        }
                        let status = tx
                            .table("payment_notifications")
                            .r#where("id", "=", key.clone())
                            .value("status")
                            .await?;
                        if status == Value::from("done") {
                            return Ok((true, false));
                        }
                        handler(tx, &notice).await?;
                        tx.table("payment_notifications")
                            .r#where("id", "=", key)
                            .update([("status".into(), "done".into())].into_iter().collect())
                            .await?;
                        Ok((false, false))
                    })
                },
                1,
            )
            .await
            .map_err(|_| Error::Database)?;
        if conflict {
            return Err(Error::NotificationConflict);
        }
        Ok(if duplicate {
            NotificationOutcome::Duplicate
        } else {
            NotificationOutcome::Processed
        })
    }
}
/// Explicitly register with the host migrator when using DatabaseNotifications.
pub struct CreatePaymentNotificationsTable;
impl Migration for CreatePaymentNotificationsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .getSchemaBuilder()
                .create("payment_notifications", |table| {
                    table.string("id", 64).primary();
                    table.string("fingerprint", 64);
                    table.string("status", 16);
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .getSchemaBuilder()
                .dropIfExists("payment_notifications")
                .await
        })
    }
}

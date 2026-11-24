use crate::{Error, ExpectedPayment, Result, VerifiedNotification, notification_store};
use openrust::database::{
    Connection, DatabaseFuture, DatabaseResult, Transaction,
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
                        if !notification_store::claim(tx, &key, &fingerprint).await? {
                            return Ok((false, true));
                        }
                        if notification_store::is_complete(tx, &key).await? {
                            return Ok((true, false));
                        }
                        handler(tx, &notice).await?;
                        notification_store::complete(tx, &key).await?;
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
                .get_schema_builder()
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
                .get_schema_builder()
                .drop_if_exists("payment_notifications")
                .await
        })
    }
}

# Notifications

## Sending notifications

Implement `Notification::via` to select channels and implement the corresponding content methods. Each recipient implements `Notifiable` to provide channel routes and optionally a preferred locale.

```rust
use openrust::{
    notifications::{Notifiable, Notification, NotificationResult, messages::DatabaseMessage},
    support::facades::Notification as Notifications,
};

pub struct InvoicePaid;

impl Notification for InvoicePaid {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["database".into()])
    }

    fn to_database(&self, _: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(Some(DatabaseMessage::new(serde_json::json!({"invoice": 42}))))
    }
}

pub async fn notify(customer: &dyn Notifiable) -> NotificationResult<()> {
    Notifications::send(&[customer], &InvoicePaid).await
}
```

For a mail notification, return `MailDelivery::Message` from `to_mail` and supply addresses via `NotificationRoute::Mail`. `MailDelivery::Mailable` sends a complete mailable with its own recipients. `sendNow` accepts an optional channel list. Locale precedence is notification, sender, recipient, then the application locale.

Listeners can stop delivery through `NotificationSending`. The dispatcher also emits `NotificationSent`, `NotificationSkipped` and `NotificationFailed`.

## Database notifications

Run `./artisan make:notifications-table`, register the generated migration in the application's migration list, and run `./artisan migrate`. The generated table uses an unsigned integer key for the notifiable model. For UUID or ULID model keys, change the `notifiable_id` column type in the generated migration.

Return `NotificationRoute::Database` with the model type and key. The database channel stores the notification UUID and JSON data. `DatabaseRoute::query` scopes queries to the notifiable model, and `find` returns a record with `markAsRead` and `markAsUnread`.

## Queueing notifications

Return `DispatchOptions` from `Notification::queue_options` for each recipient and channel that should be queued. Each delivery gets its own job. The worker resolves the channel and emits delivery events. When using an active transaction, call `ChannelManager::sendInTransaction` with `after_commit` enabled to discard queued deliveries on rollback.

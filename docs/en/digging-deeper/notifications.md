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

Use `Notification::queue_guard` when a queued delivery depends on state that can change before the worker runs. Return a `NotificationQueueGuard` with a registered name and JSON context. Register the named async decision with `ChannelManager::extendQueueGuard` in a provider loaded by the worker. A false result skips the channel and emits `NotificationSkipped`. For immediate delivery and queued notifications without a guard, the sender uses `should_send`.

## On-demand notifications

Use `Notifications::route` to send to an address without loading a model. In this example, `PaymentReceived` is a notification whose `via` includes `mail`. Chain `route` to supply additional channel addresses. The database channel requires a persisted recipient.

```rust
use openrust::{
    notifications::{Notifiable, NotificationRoute},
    support::facades::Notification as Notifications,
};

Notifications::route(
    "mail",
    NotificationRoute::Mail(vec!["ondemand@example.test".into()]),
)?
.notifyNow(&PaymentReceived, None)
.await?;
```

## Broadcast notifications

Include `broadcast` in `via`. Return a `BroadcastMessage` from `to_broadcast`, or return an object from `to_array`. Set the message's `connection` and `queue` to choose where the broadcast event runs. By default, the event adds the notification ID and type. A model with a database route receives these events on the private channel named for its model type and key. Override `receives_broadcast_notifications_on` to customize the private channel names, or use `NotificationRoute::Broadcast` to specify channels for an on-demand recipient.

## Markdown mail notifications

To send a Markdown notification, set `MailMessage.content.markdown` to a named Blade view, or return a Markdown mailable from `to_mail`. Both paths use the same [Markdown mail renderer and theme](mail.md#markdown-mail).

## Testing notifications

Install `Notifications::fake()` in the test application to record dispatches without invoking channels or pushing queue jobs. Use `assertSentToOnce` for a model target and `assertSentOnDemandOnce` for an on-demand target. Matching assertions receive a record with the selected channels, locale, routes and any data supplied through `Notification::fake_data`.

```rust
let notifications = Notifications::fake()?;
let target = Notifications::route(
    "mail",
    NotificationRoute::Mail(vec!["ops@example.test".into()]),
)?;
target.notify(&notice).await?;
notifications.assertSentOnDemandOnce(notice.kind());
```

# Notifications

## Generating notifications {#generating-notifications}

The command writes a Rust notification under `app/notifications`, using snake_case filenames and module directories. By default the generated type uses the mail channel with plain text.

```sh
./artisan make:notification InvoicePaid
./artisan make:notification Billing/PaymentReceived --markdown=mail.billing.paid --test
```

In a terminal, run `./artisan make:notification` without a name to enter the type name and choose whether to create a Markdown view. If you accept, you can keep the suggested name or enter another. If you pass options, Artisan only asks for the type name. With `--no-interaction` or piped input, provide the name explicitly.

The Markdown option also writes a Blade template. With no view name, the command uses `mail.` followed by the kebab-case name components. The command replaces an existing Markdown template at that path when the notification source is generated or regenerated. Use `--force` to regenerate an existing Rust type, and `--test` for a matching feature test. Declare the new module in its parent `mod.rs` before rebuilding. Override source templates under `stubs/`.

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

For per-channel worker limits, return `JobOptions` from `queue_job_options`. To apply queue middleware, register a named chain with `SendQueuedNotifications::registerMiddleware` in a provider loaded by both the dispatcher and worker, then return the chain’s name from `queue_middleware`. The queue worker runs the normal middleware pipeline for that job.

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

## Mail notifications {#mail-notifications}

### Formatting mail messages {#formatting-mail-messages}

Return a `MailMessage` from `to_mail` to send a responsive HTML notification with a plain-text counterpart. Lines added before `action` appear above the button; subsequent lines appear below it. Use `lineIf` or `linesIf` for conditional text. The `error` and `success` levels select the button color. The notification’s locale supplies the default greeting, closing, and copyable link instructions. The closing uses `app.name`.

```rust
use openrust::notifications::messages::{MailDelivery, MailMessage};

let delivery = MailDelivery::Message(Box::new(
    MailMessage::new()
        .subject("Invoice paid")
        .greeting("Hello!")
        .line("Your invoice has been paid.")
        .lineIf(total > 0, format!("Amount paid: {total}"))
        .action("View invoice", invoice_url)
        .line("Thank you for your payment."),
));
```

The mail channel uses the notification type name as the default subject. Override the default message view by running `./artisan vendor:publish --tag=laravel-notifications` and editing `resources/views/vendor/notifications/email.blade.html`. To select templates for one message, call `view` for HTML and optionally follow it with `text` for the plain-text alternative. Call `text` on a new message for plain text only.

### Sender and recipients {#sender-and-recipients}

Use `from` to override the configured sender and `replyTo`, `cc`, or `bcc` to append addresses. Pass an email string or an `Address` with a display name. The mail channel gets recipients from `Notifiable::route_notification_for`. Use `NotificationRoute::Mail` for models and on-demand delivery. A custom mailable returned as `MailDelivery::Mailable` supplies its own recipients.

### Attachments and message customization {#attachments-and-message-customization}

Use `attach` or `attachMany` with [attachment sources](mail.md#attachments), or call `attachData`, `attachFromStorage`, and `attachFromStorageDisk`. The worker resolves file and disk attachments at delivery. Use `tag` and `metadata` for delivery labels. Register a customizer through `MailManager::registerMessageCustomizer` in the sender and worker applications, then pass its name and JSON data to `withMessage`.

```rust
use openrust::{
    mail::{MessageCustomization, mailables::Address},
    notifications::messages::MailMessage,
};

let message = MailMessage::new()
    .from(Address {
        address: "finance@example.test".into(),
        name: Some("Finance".into()),
    })
    .replyTo("support@example.test")
    .tag("invoice")
    .metadata("invoice-id", "42")
    .line("Your invoice is ready.")
    .attachData(b"total,42".as_slice(), "summary.csv", Some("text/csv"))
    .withMessage(MessageCustomization::new(
        "invoice.trace",
        serde_json::json!({"id": 42}),
    ));
```

The example uses an application customizer registered as `invoice.trace`. For an `Attachable` object, pass the result of its `toMailAttachment()` method to `attach`.

### Previewing mail notifications {#previewing-mail-notifications}

Use `render().await` for the body or return `toResponse().await` from a route. The response is HTML or plain text according to the selected view. Preview renders the template without sending mail, resolving ordinary attachments, or running message customizers.

```rust
use openrust::{mail::Mailable, notifications::messages::MailMessage};

let mail = MailMessage::new()
    .markdown("mail.invoice.paid", data)
    .theme("billing")
    .priority(2);

let response = mail.toResponse().await?;
```

### Mail priority {#mail-priority}

Use `priority` to set the MIME `X-Priority` header. The scale runs from 1 (highest) to 5 (lowest). Values outside that range use the nearest endpoint. Priority describes the message for the recipient’s mail client.

## Markdown mail notifications

Use `./artisan make:notification InvoicePaid --markdown=mail.invoice.paid` to generate a notification and a Blade template. Use `markdown` to select a template that renders both message bodies. `template` changes the Markdown template while respecting explicit views. `theme` overrides the configured Markdown theme for that message. Builder calls merge view data, with the latest value taking precedence for each key. A custom mailable returned from `to_mail` can use the same [Markdown renderer and components](mail.md#writing-markdown-messages).

## SMS {#sms-notifications}

Use the `sms` channel, return `SmsMessage` from `to_sms`, and route the number with `NotificationRoute::Sms`. Select Vonage or another provider through a named profile in `config/sms.rs`. See [SMS](sms.md) for configuration, templates, custom providers, and Worker delivery.

### Custom SMS providers {#custom-sms-providers}

Register a provider through `SmsManager::extend`. See [registering providers](sms.md#custom-sms-providers).

## Slack notifications {#slack-notifications}

Configure the Slack app’s bot token with `SLACK_BOT_USER_OAUTH_TOKEN` and a fallback channel with `SLACK_BOT_USER_DEFAULT_CHANNEL`. Select the `slack` channel and implement `to_slack` to send through the Bot API.

```rust
use openrust::notifications::{
    Notifiable, Notification, NotificationResult, slack::SlackMessage,
};

struct InvoicePaid;

impl Notification for InvoicePaid {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["slack".into()])
    }

    fn to_slack(&self, _: &dyn Notifiable) -> NotificationResult<Option<SlackMessage>> {
        Ok(Some(SlackMessage::new()
            .text("Invoice paid")
            .headerBlock("Invoice paid")
            .sectionBlock(|block| {
                block.field("*Amount:* 42").markdown();
            })
            .actionsBlock(|block| {
                block.button("Acknowledge payment").primary().confirm(
                    "Record this acknowledgement?",
                    |dialog| {
                        dialog.confirm("Continue").deny("Cancel");
                    },
                );
            })))
    }
}
```

Use `usingBlockKitTemplate` to append blocks from Slack’s JSON builder and `toBlockKitBuilderUrl` to inspect a payload. Interactive buttons and confirmation dialogs describe what Slack displays. Register a Slack interactivity URL and handle the incoming actions in your application.

An explicit `NotificationRoute::Slack` channel overrides the message’s `to` channel. The configured default applies when both are absent. For an external workspace, pass the channel and its bot token with `SlackRoute::make`. Queued external workspace tokens are encrypted with `APP_KEY`. Default workspace tokens are read from configuration at delivery.

```rust
use openrust::{
    notifications::NotificationRoute,
    support::facades::Notification as Notifications,
};

Notifications::route("slack", NotificationRoute::Slack("C0123456789".into()))?
    .notify(&InvoicePaid)
    .await?;
```

The Bot API uses `chat.postMessage`. For a local protocol test, configure `services.slack.notifications.endpoint` with a loopback HTTP URL. Confirming delivery in Slack requires an installed app, its bot permissions, and an authorized workspace.

### Block Kit select menus {#slack-block-kit-selects}

Use `staticSelect` to add a menu with application-defined options and `usersSelect` for Slack workspace users. The first argument sets the placeholder. Use explicit action IDs for application callbacks. Both menus can be added to an actions block or used as the accessory of a section.

```rust
use openrust::notifications::{NotificationResult, slack::SlackMessage};

fn review_message() -> NotificationResult<SlackMessage> {
    Ok(SlackMessage::new()
        .text("Invoice review")
        .tryActionsBlock(|actions| {
            actions
                .staticSelect("Choose a status")
                .id("invoice-status")
                .addOption("Needs review", "needs-review")
                .addOption("Paid", "paid")
                .initialOption("needs-review")?
                .focus(false);
            Ok(())
        })?
        .sectionBlock(|section| {
            section.text("Assign invoice");
            section
                .usersSelect("Select a teammate")
                .id("invoice-owner")
                .initialUser("U012345");
        }))
}
```

A repeated static option key replaces the previous one without changing its position. `initialOption` looks up the original key and returns an error for an unknown key. Slack receives the normalized lowercase value. `focus(false)` sends an explicit false flag. Use `toArray` to check a typed message before delivery.

### Headers, dividers, and images {#slack-block-kit-visual-blocks}

Use `headerBlockWith`, `dividerBlockWith`, and `imageBlockWith` to configure block identifiers and the image’s alternative text and title. Use `ContextBlock::imageWith` to set alternative text for an image inside a context block.

```rust
use openrust::notifications::slack::SlackMessage;

fn report_message() -> SlackMessage {
    SlackMessage::new()
        .text("Monthly invoice report")
        .headerBlockWith("Invoice report", |header| {
            header.id("invoice-report").text().emoji();
        })
        .dividerBlockWith(|divider| {
            divider.id("invoice-divider");
        })
        .imageBlockWith("https://example.test/invoices.png", |image| {
            image.id("invoice-chart").alt("Invoice counts by month");
            image.title("Monthly invoices");
        })
}
```

Block identifiers may use up to 255 bytes. Standalone image URLs may use up to 3,000 bytes, and alternative text up to 2,000 bytes. The title is truncated at its byte limit without splitting UTF-8. A missing alternative text field fails before delivery.

### Metadata and thread replies {#slack-metadata-and-replies}

Attach structured metadata with `metadata` or pass a reusable `EventMetadata` to `withMetadata`. Slack receives `event_type` and `event_payload` as JSON, with the payload represented as an object even when empty. Keep secrets out of Slack message metadata.

```rust
use openrust::notifications::slack::{EventMetadata, SlackMessage};
use serde_json::json;

fn invoice_reply(parent_timestamp: &str) -> SlackMessage {
    let event = EventMetadata::make(
        "invoice_paid",
        [("invoice_id".into(), json!(42))].into_iter().collect(),
    );

    SlackMessage::new()
        .text("Invoice 42 paid")
        .withMetadata(event)
        .threadTimestamp(parent_timestamp)
        .broadcastReply(false)
}
```

Pass the parent message’s timestamp as a string to `threadTimestamp`. With the Bot API, `broadcastReply(true)` also shares the reply to the conversation, while `broadcastReply(false)` keeps it in the thread. Use `clearThreadTimestamp()` or `broadcastReply(None)` to omit either field independently. Slack validates the event schema and applies the thread behavior.

### Incoming webhooks {#slack-incoming-webhooks}

Pass an HTTP(S) webhook URL as the Slack route to deliver the same `SlackMessage`, including Block Kit, through the incoming webhook. Slack derives the destination from the webhook installation. The request uses JSON and the URL itself is the credential.

```rust
async fn send_to_webhook(webhook_url: &str) -> NotificationResult<()> {
    Notifications::route("slack", NotificationRoute::Slack(webhook_url.into()))?
        .notify(&InvoicePaid)
        .await?;
    Ok(())
}
```

Store the URL in application secrets. Queued webhook URLs are encrypted with `APP_KEY` and decrypted by the Worker. Delivery requires HTTPS, with loopback HTTP available for local tests. Redirects and unsuccessful HTTP statuses fail the delivery.

### Legacy Slack attachments {#slack-legacy-attachments}

For an existing webhook integration that expects Slack’s legacy attachments, implement `to_legacy_slack` and return `notifications::messages::SlackMessage`. The webhook uses that message when provided and falls back to the Block Kit message from `to_slack`. The Bot API uses `to_slack`.

```rust
use openrust::notifications::{
    Notifiable, Notification, NotificationResult, messages::SlackMessage,
};

struct InvoicePending;

impl Notification for InvoicePending {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["slack".into()])
    }

    fn to_legacy_slack(&self, _: &dyn Notifiable) -> NotificationResult<Option<SlackMessage>> {
        Ok(Some(
            SlackMessage::new()
                .warning()
                .content("Invoice pending")
                .attachment(|attachment| {
                    attachment
                        .title("Invoice 42", Some("https://example.test/invoices/42"))
                        .field("Status", "pending")
                        .fieldWith(|field| {
                            field.title("Next step").content("Review payment").long();
                        });
                }),
        ))
    }
}
```

An attachment inherits the message’s level color unless it sets its own color. Use `field` for a short field and `fieldWith` with `long()` to display a field on its own line. `timestamp` accepts a date and time and `timestampAfter` adds seconds to the current time. Legacy attachments use the same webhook transport, encryption and retry behavior.

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

# 通知

## 生成通知 {#generating-notifications}

命令在 `app/notifications` 下生成 Rust 通知，文件和模块目录使用 snake_case。默认类型通过邮件渠道发送纯文本。

```sh
./artisan make:notification InvoicePaid
./artisan make:notification Billing/PaymentReceived --markdown=mail.billing.paid --test
```

在终端运行不带名称的 `./artisan make:notification`，可输入类型名称并选择是否创建 Markdown 视图。确认创建后，可接受建议的视图名称或另行输入。传入选项时，Artisan 只询问类型名称。使用 `--no-interaction` 或管道输入时，需显式提供名称。

Markdown 选项同时生成 Blade 模板。省略视图名称时，用 `mail.` 加上以连字符分词的各名称段。该命令在生成或重新生成通知源码时会替换同路径的 Markdown 模板。用 `--force` 重新生成已有 Rust 类型，用 `--test` 生成配套功能测试。重新构建前，在上级 `mod.rs` 中声明新模块。应用可在 `stubs/` 覆盖源码模板。

## 发送通知

实现 `Notification::via` 选择渠道，再实现对应的内容方法。收件人实现 `Notifiable`，提供各渠道路由和可选的语言偏好。

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

邮件通知在 `to_mail` 返回 `MailDelivery::Message`，通过 `NotificationRoute::Mail` 提供地址。`MailDelivery::Mailable` 可发送自带收件人的完整邮件。`sendNow` 可显式指定渠道。语言依次采用通知、发送器、收件人和应用的设置。

监听器可通过 `NotificationSending` 阻止投递。分派器也会发出 `NotificationSent`、`NotificationSkipped` 和 `NotificationFailed`。

## 数据库通知

执行 `./artisan make:notifications-table`，将生成的迁移加入应用迁移列表，再执行 `./artisan migrate`。生成的关联键默认是无符号整数。模型使用 UUID 或 ULID 时，在生成的迁移中调整 `notifiable_id` 的列类型。

通过 `NotificationRoute::Database` 提供模型类型和主键。数据库渠道保存通知 UUID 和 JSON 数据。`DatabaseRoute::query` 按模型限制查询。`find` 返回可调用 `markAsRead` 和 `markAsUnread` 的记录。

## 通知入队

通过 `Notification::queue_options` 为每位收件人的每个渠道返回 `DispatchOptions`。每次投递有自己的队列任务。Worker 解析渠道并触发投递事件。在活动事务中，调用 `ChannelManager::sendInTransaction` 并启用 `after_commit`，使回滚时丢弃待入队通知。

如果队列通知依赖可能在 Worker 启动前变化的状态，通过 `Notification::queue_guard` 返回带名称和 JSON 上下文的 `NotificationQueueGuard`。在 Worker 加载的服务提供者中，用 `ChannelManager::extendQueueGuard` 注册异步判定函数。结果为 false 时跳过该渠道并发出 `NotificationSkipped`。立即发送和未配置 guard 的队列通知沿用发送端的 `should_send`。

若要按渠道设置 Worker 限制，从 `queue_job_options` 返回 `JobOptions`。要为通知启用队列中间件，在发送端和 Worker 都加载的服务提供者中，用 `SendQueuedNotifications::registerMiddleware` 注册命名链，并让 `queue_middleware` 返回该名称。Worker 会对该任务运行队列的中间件管道。

## 按需通知

用 `Notifications::route` 直接向地址发送通知，无需加载模型。示例中的 `PaymentReceived` 是 `via` 包含 `mail` 的通知。可以链式调用 `route` 添加其他渠道地址。数据库渠道需要已持久化的收件人。

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

## 广播通知

在 `via` 中加入 `broadcast`。由 `to_broadcast` 返回 `BroadcastMessage`，或由 `to_array` 返回对象。用消息的 `connection` 和 `queue` 选择广播事件的队列连接和队列名。事件默认附加通知 ID 和类型。有数据库路由的模型默认在由模型类型和键组成的私有频道上接收事件。可以覆盖 `receives_broadcast_notifications_on` 自定义私有频道名，或用 `NotificationRoute::Broadcast` 为按需收件人指定频道。

## 邮件通知 {#mail-notifications}

### 格式化邮件消息 {#formatting-mail-messages}

在 `to_mail` 中返回 `MailMessage`，可发送响应式 HTML 通知和对应的纯文本正文。在 `action` 之前加入的正文行出现在按钮上方，之后加入的行则在按钮下方。用 `lineIf` 或 `linesIf` 按条件加入正文。`error` 和 `success` 决定按钮颜色。默认问候语、结束语和复制链接说明按通知语言生成，结束语使用 `app.name`。

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

未指定主题时，邮件渠道根据通知类型名生成主题。运行 `./artisan vendor:publish --tag=laravel-notifications`，可编辑 `resources/views/vendor/notifications/email.blade.html` 覆盖默认视图。 要为单条消息选择模板，可调用 `view` 设置 HTML 视图，再按需用 `text` 添加纯文本视图。对新消息直接调用 `text` 可只生成纯文本。

### 发件人和收件人 {#sender-and-recipients}

用 `from` 覆盖配置中的发件人，用 `replyTo`、`cc` 或 `bcc` 追加地址。可传邮箱字符串或带显示名的 `Address`。邮件渠道通过 `Notifiable::route_notification_for` 获取收件人，模型通知和按需通知都使用 `NotificationRoute::Mail`。通过 `MailDelivery::Mailable` 返回的自定义邮件自行声明收件人。

### 附件与消息定制 {#attachments-and-message-customization}

用 `attach` 或 `attachMany` 添加[附件来源](mail.md#attachments)，也可调用 `attachData`、`attachFromStorage` 或 `attachFromStorageDisk`。队列 Worker 投递时解析文件与磁盘附件。用 `tag` 和 `metadata` 添加投递标记。在发送端和 Worker 应用中通过 `MailManager::registerMessageCustomizer` 注册回调，再用 `withMessage` 传入注册名和 JSON 数据。

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

示例使用注册名为 `invoice.trace` 的应用回调。若附件实现了 `Attachable`，把其 `toMailAttachment()` 的返回值交给 `attach`。

### 预览通知邮件 {#previewing-mail-notifications}

用 `render().await` 获取正文，或在路由里返回 `toResponse().await`。响应会按所选视图返回 HTML 或纯文本。预览只渲染模板，不发送邮件、读取普通附件或执行消息定制回调。

```rust
use openrust::{mail::Mailable, notifications::messages::MailMessage};

let mail = MailMessage::new()
    .markdown("mail.invoice.paid", data)
    .theme("billing")
    .priority(2);

let response = mail.toResponse().await?;
```

### 邮件优先级 {#mail-priority}

用 `priority` 设置 MIME 的 `X-Priority`。1 为最高，5 为最低，超出范围时取最近的边界值。此优先级供收件方的邮件客户端使用。

## Markdown 邮件通知

运行 `./artisan make:notification InvoicePaid --markdown=mail.invoice.paid` 生成通知和 Blade 模板。用 `markdown` 选定渲染双正文的模板。`template` 调整 Markdown 模板并尊重已明确选择的视图，`theme` 可为单封邮件覆盖 Markdown 主题。构建方法按键合并视图数据，后提供的值优先。`to_mail` 返回的自定义可邮件对象也可使用同一套 [Markdown 渲染器和组件](mail.md#writing-markdown-messages)。

## 短信 {#sms-notifications}

使用 `sms` 渠道，在 `to_sms` 中返回 `SmsMessage`，并通过 `NotificationRoute::Sms` 指定号码。在 `config/sms.rs` 中按具名配置选择 Vonage 或其他供应商。配置、模板、自定义供应商和 Worker 投递见[短信](sms.md)。

### 自定义短信供应商 {#custom-sms-providers}

通过 `SmsManager::extend` 注册供应商，详见[注册供应商](sms.md#custom-sms-providers)。

## Slack 通知 {#slack-notifications}

用 `SLACK_BOT_USER_OAUTH_TOKEN` 配置 Slack 应用的 Bot 令牌，用 `SLACK_BOT_USER_DEFAULT_CHANNEL` 配置默认频道。选择 `slack` 通知渠道并实现 `to_slack`，即可通过 Bot API 发送。

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

用 `usingBlockKitTemplate` 追加 Slack JSON 构建器导出的区块，用 `toBlockKitBuilderUrl` 查看载荷。按钮和确认对话框描述 Slack 中显示的交互。应用需要注册 Slack 交互 URL 并处理回调。

显式 `NotificationRoute::Slack` 的频道优先于消息的 `to` 频道。二者都未指定时使用配置中的默认频道。外部工作区可用 `SlackRoute::make` 传入频道和 Bot 令牌。外部工作区令牌入队时通过 `APP_KEY` 加密，默认工作区令牌在投递时从配置读取。

```rust
use openrust::{
    notifications::NotificationRoute,
    support::facades::Notification as Notifications,
};

Notifications::route("slack", NotificationRoute::Slack("C0123456789".into()))?
    .notify(&InvoicePaid)
    .await?;
```

Bot API 使用 `chat.postMessage`。应用测试时，可将本地模拟服务的 HTTP URL 配置为 `services.slack.notifications.endpoint`。在 Slack 内确认送达需要安装应用、授予 Bot 权限并使用获授权的工作区。

### Block Kit 选择器 {#slack-block-kit-selects}

用 `staticSelect` 添加应用定义选项的下拉菜单，用 `usersSelect` 选择 Slack 工作区用户。第一个参数设置占位文字。对接应用回调时请设置明确的动作 ID。两种选择器都能放在动作区块，也能作为段落区块的附属组件。

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

重复的静态选项键会覆盖已有选项，位置保持原样。`initialOption` 按原始键查找，遇到未知键会返回错误。发送给 Slack 的选项值会转为规范小写形式。`focus(false)` 会发送明确的 false 值。投递前可用 `toArray` 检查类型化消息。

### 标题、分隔线与图片 {#slack-block-kit-visual-blocks}

用 `headerBlockWith`、`dividerBlockWith` 和 `imageBlockWith` 配置区块标识、图片替代文字和标题。上下文区块中的图片可通过 `ContextBlock::imageWith` 设置替代文字。

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

区块标识最多 255 字节。独立图片 URL 最多 3,000 字节，替代文字最多 2,000 字节。标题按字节上限截断并保留 UTF-8 字符边界。缺少替代文字字段会在投递前报错。

### 元数据与线程回复 {#slack-metadata-and-replies}

可用 `metadata` 添加结构化元数据，也可将可复用的 `EventMetadata` 传给 `withMetadata`。Slack 收到 JSON 格式的 `event_type` 和 `event_payload`，空载荷也发送为对象。请勿在 Slack 消息元数据中存放密钥。

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

用 `threadTimestamp` 传入父消息的时间戳字符串。通过 Bot API 发送时，`broadcastReply(true)` 也会把回复分享到会话，`broadcastReply(false)` 则只保留在线程内。可分别使用 `clearThreadTimestamp()` 或 `broadcastReply(None)` 省略对应字段。Slack 负责验证事件模式和应用线程行为。

### 传入 Webhook {#slack-incoming-webhooks}

将 HTTP(S) Webhook URL 作为 Slack 路由，可通过传入 Webhook 投递同一个 `SlackMessage`，包括 Block Kit。Slack 根据 Webhook 的安装配置决定投递目标。请求使用 JSON，URL 本身就是凭据。

```rust
async fn send_to_webhook(webhook_url: &str) -> NotificationResult<()> {
    Notifications::route("slack", NotificationRoute::Slack(webhook_url.into()))?
        .notify(&InvoicePaid)
        .await?;
    Ok(())
}
```

请在应用的密钥存储中保存 URL。Webhook URL 入队时用 `APP_KEY` 加密，由 Worker 解密。投递要求 HTTPS，本地测试可使用回环 HTTP。重定向和非成功 HTTP 状态会使投递失败。

### 旧式 Slack 附件 {#slack-legacy-attachments}

已有 Webhook 集成需要 Slack 旧式附件时，实现 `to_legacy_slack` 并返回 `notifications::messages::SlackMessage`。Webhook 优先使用该消息，未提供时回退到 `to_slack` 返回的 Block Kit 消息。Bot API 使用 `to_slack`。

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

附件可以指定自己的颜色，省略时继承消息级别的颜色。使用 `field` 添加短字段，使用 `fieldWith` 配合 `long()` 让字段独占一行。`timestamp` 接收日期时间，`timestampAfter` 在当前时间上增加秒数。旧式附件沿用相同的 Webhook 传输、加密和重试行为。

## 测试通知

在测试应用中调用 `Notifications::fake()` 记录通知分派，渠道和队列不会执行。用 `assertSentToOnce` 断言模型收件人，用 `assertSentOnDemandOnce` 断言按需收件人。匹配断言可检查渠道、语言、路由以及 `Notification::fake_data` 提供的数据。

```rust
let notifications = Notifications::fake()?;
let target = Notifications::route(
    "mail",
    NotificationRoute::Mail(vec!["ops@example.test".into()]),
)?;
target.notify(&notice).await?;
notifications.assertSentOnDemandOnce(notice.kind());
```

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

## 短信通知 {#sms-notifications}

配置 `VONAGE_KEY`、`VONAGE_SECRET` 和 `VONAGE_SMS_FROM`。在通知渠道列表加入 `vonage`，并在 `to_vonage` 中返回 `VonageMessage`。

```rust
use openrust::notifications::{
    Notifiable, Notification, NotificationResult,
    messages::VonageMessage,
};

struct InvoicePaid;

impl Notification for InvoicePaid {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["vonage".into()])
    }

    fn to_vonage(&self, _: &dyn Notifiable) -> NotificationResult<Option<VonageMessage>> {
        Ok(Some(VonageMessage::new()
            .content("Payment received — thank you!")
            .unicode()
            .clientReference("invoice-42")))
    }
}
```

用 `from` 覆盖发件人，`unicode` 发送 Unicode 正文，`clientReference` 设置业务跟踪标识，`statusCallback` 将送达回执 URL 传给 Vonage。`usingClient("partner")` 选取 `vonage.clients.partner` 中配置的凭据。可选的 `endpoint` 覆盖供应商 URL，支持 HTTPS 或回环 HTTP。

模型收件人通过 `route_notification_for` 返回 `NotificationRoute::Vonage`。按需通知可在指定渠道时传入号码。

```rust
use openrust::{
    notifications::NotificationRoute,
    support::facades::Notification as Notifications,
};

Notifications::route("vonage", NotificationRoute::Vonage("+447700900000".into()))?
    .notify(&InvoicePaid)
    .await?;
```

队列保存号码、消息和可选的客户端名称。Worker 从应用配置读取凭据，每次任务尝试只发起一次 HTTP 请求。Vonage API 接收成功和手机送达是两个独立状态。

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

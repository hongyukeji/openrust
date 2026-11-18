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

未指定主题时，邮件渠道根据通知类型名生成主题。运行 `./artisan vendor:publish --tag=laravel-notifications`，可编辑 `resources/views/vendor/notifications/email.blade.html` 覆盖默认视图。要为单条消息指定其他模板，设置其 `content.view`、`content.text_view` 或 `content.markdown`。

## Markdown 邮件通知

将 `MailMessage.content.markdown` 设为命名 Blade 视图，或从 `to_mail` 返回 Markdown mailable，即可发送 Markdown 通知。两种方式共用 [Markdown 邮件渲染和主题](mail.md#markdown-邮件)。

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

# 通知

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

# 通知

## 傳送通知

實作 `Notification::via` 選擇管道，再實作對應的內容方法。收件人實作 `Notifiable`，提供各管道路由和選用的語言偏好。

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

郵件通知在 `to_mail` 回傳 `MailDelivery::Message`，透過 `NotificationRoute::Mail` 提供地址。`MailDelivery::Mailable` 可傳送自帶收件人的完整郵件。`sendNow` 可明確指定管道。語言依序採用通知、傳送器、收件人和應用程式的設定。

監聽器可透過 `NotificationSending` 阻止投遞。派送器也會發出 `NotificationSent`、`NotificationSkipped` 和 `NotificationFailed`。

## 資料庫通知

執行 `./artisan make:notifications-table`，將產生的遷移加入應用程式遷移清單，再執行 `./artisan migrate`。產生的關聯鍵預設為無號整數。模型使用 UUID 或 ULID 時，在產生的遷移中調整 `notifiable_id` 欄位的類型。

透過 `NotificationRoute::Database` 提供模型類型和主鍵。資料庫管道儲存通知 UUID 和 JSON 資料。`DatabaseRoute::query` 依模型限制查詢。`find` 回傳可呼叫 `markAsRead` 和 `markAsUnread` 的記錄。

## 通知加入佇列

透過 `Notification::queue_options` 為每位收件人的每個管道回傳 `DispatchOptions`。每次投遞有自己的佇列任務。Worker 解析管道並觸發投遞事件。在使用中的交易裡，呼叫 `ChannelManager::sendInTransaction` 並啟用 `after_commit`，使交易回復時捨棄待加入佇列的通知。

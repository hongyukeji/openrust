# 公开 API 示例

以下示例使用框架公开接口；功能开关及编译边界由各示例标注。应用开发流程见仓库根用户指南。

```rust
use openrust::support::{MessageBag, MessageJsonOptions};

fn business_errors() -> openrust::Result<String> {
    let mut errors = MessageBag::new();
    errors.add("email", "邮箱已被使用")?;
    errors.add("title", "请填写标题")?;
    errors.to_json(MessageJsonOptions::UNESCAPED_UNICODE)
}
```

```rust
#[cfg(feature = "queue")]
fn main() -> openrust::Result<()> {
use openrust::queue::QueueServiceProvider;
use openrust::schedule::{CronOptions, MissedRunPolicy};

let queue = QueueServiceProvider::new();
queue.contributions().schedule_cron(
    "reports.daily",
    "reports",
    "reports.daily.v1",
    &serde_json::json!({}),
    "30 9 * * 1-5",
    CronOptions {
        time_zone: "Asia/Shanghai".to_owned(),
        missed_runs: MissedRunPolicy::CatchUp { max_slots: 3 },
    },
)?;
Ok(())
}
#[cfg(not(feature = "queue"))]
fn main() {}
```

```rust,ignore
let mut events = openrust::events::EventsServiceProvider::new();
events.listen_queued::<UserRegistered, WelcomeEmailJob, _>("mail", |event| {
    Ok(WelcomeEmailJob { user_id: event.user_id })
});
let mut builder = openrust::core::ProviderBuilder::new();
builder.provider(openrust::queue::QueueServiceProvider::new()).provider(events);
let app = openrust::App::from_provider_runtime(builder.start(config).await?)?;
let events = app.state().resolve::<openrust::events::Events>().await?;

db.transaction(|transaction| {
    let events = std::sync::Arc::clone(&events);
    Box::pin(async move {
        create_user(transaction).await?;
        events
            .dispatch_in_transaction(transaction, &UserRegistered { user_id: 42 })
            .await?;
        Ok(())
    })
})
.await?;
```

```rust
use openrust::http::{Inject, Json};
use openrust::i18n::{I18n, MessageArgs, RequestLocale};

async fn greeting(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "app.greeting", &args))
}
```

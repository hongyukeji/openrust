# 事件

## 產生事件與監聽器 {#generating-events-and-listeners}

使用 Artisan 產生事件和監聽器模組。`--event` 指定事件型別，`--queued` 透過佇列執行，`--test` 為監聽器產生配套功能測試。傳入 `--force` 可覆寫現有檔案。

```sh
./artisan make:event OrderPlaced
./artisan make:listener LogOrder
./artisan make:listener RecordOrder --event=OrderPlaced --test
./artisan make:listener QueueOrder --queued
./artisan make:listener NotifyOrder --event=OrderPlaced --queued
```

`--event` 接受 `OrderPlaced` 等 PascalCase 事件名稱、`App/Events/OrderPlaced` 等應用程式限定名稱，或 `crate::app::events::order_placed::OrderPlaced` 等 Rust 路徑。

在 Rust 模組樹中宣告產生的檔案，並在應用程式建構器的 `withEvents` 中傳入 `openrust::foundation::events::DiscoverEvents::new(&[register_orders])`。未使用 `--event` 的佇列監聽器在註冊時指定事件型別。產生的事件透過 Serde 衍生佇列所需的序列化能力。

```rust
use crate::app::{
    events::order_placed::OrderPlaced,
    listeners::{log_order, notify_order, queue_order, record_order},
};
use openrust::events::{EventResult, ListenerRegistry};

fn register_orders(registry: &mut ListenerRegistry) -> EventResult<()> {
    log_order::register(registry, OrderPlaced::NAME)?;
    record_order::register(registry, OrderPlaced::NAME)?;
    queue_order::register::<OrderPlaced>(registry, OrderPlaced::NAME)?;
    notify_order::register(registry, OrderPlaced::NAME)
}
```

## 註冊監聽器與訂閱者

在應用程式中宣告註冊函式，透過 `withEvents(DiscoverEvents::new(&[register_orders]))` 接入。應用程式啟動時載入已編譯的清單。實作 `Subscriber` 後，可使用 `Event::subscribe` 成組註冊監聽器。執行 `./artisan event:list --event=orders --json` 查看註冊結果。

```rust
use std::sync::Arc;

use openrust::{
    contracts::events::{Event, Listener},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse, NamedEvent,
    },
};

struct OrderListener;

impl Listener for OrderListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<NamedEvent<u64>>()
                .ok_or(EventError::InvalidPayload)?;
            Ok(Some(ListenerResponse::Value(Box::new(*event.payload()))))
        })
    }
}

fn register_orders(registry: &mut ListenerRegistry) -> EventResult<()> {
    registry.listen("orders.created", Arc::new(OrderListener), Delivery::Sync)
}
```

## 派送事件

呼叫 `Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?`，也可派送實作 `contracts::events::Event` 的型別。精確比對的監聽器依註冊順序執行，接著執行符合的萬用字元群組。`ListenerResponse::Stop` 停止傳播，`Event::until` 傳回第一個監聽回應。

## 資料庫交易提交後派送

透過 `Event::dispatchInTransaction` 明確傳入目前交易。使用 `Delivery::AfterCommit` 延後單一監聽器，或實作 `ShouldDispatchAfterCommit` 並呼叫 `Event::dispatchAfterCommit` 延後整個事件。儲存點成功提交後，回呼保留至最外層提交時執行。復原會清除受影響範圍內的回呼。沒有使用中的交易時立即執行。提交後的回呼失敗會以 `DatabaseError::AfterCommit` 回報。

## 延遲派送事件 {#deferring-events}

以 `Event::defer(None, async { … })` 收集事件，在回呼傳回 `Ok` 後依序派送。傳入 `Some(&["orders.created"])` 可僅延遲指定名稱。回呼傳回錯誤時捨棄待派送事件。延遲範圍屬於目前的非同步工作和應用程式。明確傳入資料庫交易時，使用上一節的交易派送介面。

## 測試 {#testing}

`TestApp::fakeEvents` 在目前的測試應用程式記錄選定的事件派發。處於應用程式作用域時也可呼叫 `Event::fake`。傳入空切片以選取所有事件，傳入完整事件名稱限定範圍，或呼叫 `except` 讓指定事件繼續交給監聽器。

```rust
use openrust::{
    contracts::events::Dispatcher as _,
    events::{Dispatcher, EventContext, NamedEvent},
    testing::TestApp,
};
use std::sync::Arc;

#[tokio::test]
async fn records_order_events() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    let fake = app.fakeEvents(&["orders.created"]).await?;
    let dispatcher = app.app().make::<Dispatcher>().await?;
    dispatcher.dispatch(
        Arc::new(NamedEvent::new("orders.created", 42_u64)),
        &mut EventContext::new(),
    ).await?;

    fake.assertDispatchedTimes("orders.created", 1)
        .assertNotDispatched("orders.cancelled");
    Ok(())
}
```

被 fake 的事件會略過監聽器、佇列及廣播。提交後派發的事件在交易提交後才記錄，回復會捨棄事件。使用 `dispatched` 取得記錄的酬載並檢查具體型別。替身隨測試應用程式釋放，也可透過再次安裝 fake 替換。

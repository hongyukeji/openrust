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

在 Rust 模組樹中宣告產生的檔案，並在應用程式建構器的 `with_events` 中傳入 `openrust::foundation::events::DiscoverEvents::new(&[register_orders])`。未使用 `--event` 的佇列監聽器在註冊時指定事件型別。產生的事件透過 Serde 衍生佇列所需的序列化能力。

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

在應用程式中宣告註冊函式，透過 `with_events(DiscoverEvents::new(&[register_orders]))` 接入。應用程式啟動時載入已編譯的清單。實作 `Subscriber` 後，可使用 `Event::subscribe` 成組註冊監聽器。執行 `./artisan event:list --event=orders --json` 查看註冊結果。

在清單中呼叫 `registry.listenClass::<OrderListener>("orders.created", Delivery::Sync)?` 註冊監聽器型別，也可以在應用程式作用域內呼叫 `Event::listenClass`。在應用程式容器中為該型別繫結工廠，非同步依賴可用 `bind_async`。派送到該監聽器時，框架從所屬應用程式的容器解析實例。缺少繫結會使派送失敗。使用 `Delivery::AfterCommit` 時，實例在派送時解析，交易成功提交後呼叫。

需要動態探索時，實作 `ShouldBeDiscovered`，用 `withListener` 接入監聽器註冊。用 `with_events(discover_events())` 接入產生的清單。啟動時，探索條件讀取目前應用並決定是否註冊監聽器。條件回傳錯誤時，本次清單註冊失敗。透過 `Event::listen` 手動註冊的監聽器獨立於探索條件。

```rust
use std::sync::Arc;

use openrust::{
    contracts::events::{Event, Listener, ShouldBeDiscovered},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse, NamedEvent,
    },
    foundation::{Application, events::DiscoverEvents},
};

struct OrderListener;

impl ShouldBeDiscovered for OrderListener {
    fn shouldBeDiscovered(app: &Application) -> EventResult<bool> {
        app.environment(Some("production".into()))
            .map(bool::from)
            .map_err(EventError::Unavailable)
    }
}

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

fn discover_events() -> DiscoverEvents {
    DiscoverEvents::new(&[]).withListener::<OrderListener>(register_orders)
}
```

### 正式環境的事件探索 {#event-discovery-in-production}

事件快取是選用的部署最佳化。應用程式啟動時，如果快取與編譯清單相符，就會重用有條件監聽器的啟用結果。快取遺失、損毀或不相符時，啟動會重新計算條件，且不寫入檔案。手動監聽器與無條件註冊照常執行。

在部署環境執行 `./artisan event:cache`，將探索結果寫入 `storage/framework/cache/events.json`。用 `APP_EVENTS_CACHE` 指定絕對路徑或相對於應用程式根目錄的路徑。使用設定快取時，將此項設為服務的程序環境變數。啟動後可用 `get_cached_events_path()` 查詢解析後的路徑。部署新版或修改探索條件後，重新產生快取。執行 `./artisan event:clear` 刪除檔案，下次啟動時重新判斷條件。

模型生命週期處理程式可在服務提供者啟動時註冊為 [Eloquent 觀察者](../eloquent/getting-started.md#observers)。

## 派送事件

呼叫 `Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?`，也可派送實作 `contracts::events::Event` 的型別。精確比對的監聽器依註冊順序執行，接著執行符合的萬用字元群組。`ListenerResponse::Stop` 停止傳播，`Event::until` 傳回第一個監聽回應。

型別化事件可透過 `Event::interfaces()` 宣告共用契約名稱，並用 `Event::listen` 依該名稱註冊監聽器。例如，訂單事件可回傳 `&["orders.lifecycle"]`，與其他訂單事件共用監聽器。精確契約監聽器在事件本身的精確與萬用字元監聽器之後依宣告順序執行。處理程式收到原始承載資料，萬用字元比對使用實際派送的事件名稱。

## 資料庫交易提交後派送

透過 `Event::dispatchInTransaction` 明確傳入目前交易。使用 `Delivery::AfterCommit` 延後單一監聽器，或實作 `ShouldDispatchAfterCommit` 並呼叫 `Event::dispatchAfterCommit` 延後整個事件。儲存點成功提交後，回呼保留至最外層提交時執行。復原會清除受影響範圍內的回呼。沒有使用中的交易時立即執行。提交後的回呼失敗會以 `DatabaseError::AfterCommit` 回報。

模型專屬回呼見[觀察者與資料庫交易](../eloquent/getting-started.md#observers-and-database-transactions)。

## 延遲派送事件 {#deferring-events}

以 `Event::defer(None, async { … })` 收集事件，在回呼傳回 `Ok` 後依序派送。傳入 `Some(&["orders.created"])` 可僅延遲指定名稱。回呼傳回錯誤時捨棄待派送事件。延遲範圍屬於目前的非同步工作和應用程式。明確傳入資料庫交易時，使用上一節的交易派送介面。

## 測試 {#testing}

`TestApp::fake_events` 在目前的測試應用程式記錄選定的事件派發。處於應用程式作用域時也可呼叫 `Event::fake`。傳入空切片以選取所有事件，傳入完整事件名稱限定範圍，或呼叫 `except` 讓指定事件繼續交給監聽器。

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
    let fake = app.fake_events(&["orders.created"]).await?;
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

選取所有事件時，替身也會記錄 `TransactionBeginning`、`TransactionRolledBack` 等資料庫交易生命週期事件。驗證復原是否捨棄某個延遲派發的應用程式事件時，可在安裝替身時只選取該事件，或使用 `assertNotDispatched` 驗證該事件名稱。

### 作用域內的事件替身 {#scoped-event-fakes}

在應用程式作用域內，`Event::fakeFor` 使用暫時替身執行非同步回呼，並傳回回呼的結果。空清單會選取所有事件。`Event::fakeExceptFor` 允許列出的事件交給監聽器，其餘事件由替身記錄。若要在整個應用程式使用相同的排除規則，可呼叫 `Event::fakeExcept`，直到安裝另一個替身。

```rust
use openrust::{events::NamedEvent, support::facades::Event};

let order_id = Event::fakeFor(
    async {
        Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?;
        Ok(42_u64)
    },
    &["orders.created"],
).await?;
assert_eq!(order_id, 42);
```

作用域隨回呼的 Future 生效，並繫結對應的分發器。巢狀作用域結束時會還原外層替身，傳回錯誤或取消也一樣。並行請求與獨立衍生的工作使用各自的作用域或應用程式層級的替身。

### 依事件酬載篩選 {#matching-event-payloads}

在應用程式作用域內，`Event::fakeWhen` 依事件全名或接收事件的回呼選取事件。`Event::fakeForWhen` 在非同步回呼內套用相同規則。名稱清單為空時，由篩選回呼決定。`except` 和 `exceptWhen` 讓符合排除規則的事件繼續交給一般監聽器，即使替身已選取該事件。

```rust
use openrust::{contracts::events::Event as Payload, events::NamedEvent, support::facades::Event};

let is_priority = |event: &dyn Payload| {
    event.name() == "orders.created"
        && event.downcast_ref::<NamedEvent<u64>>()
            .is_some_and(|order| *order.payload() >= 42)
};
let fake = Event::fakeWhen(&[], is_priority)?;
Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?;
Event::dispatch(NamedEvent::new("orders.created", 17_u64)).await?;

fake.assertDispatchedOnce("orders.created")
    .assertDispatchedWhere("orders.created", is_priority)
    .assertNotDispatchedWhere("orders.created", |event| !is_priority(event));
```

使用 `dispatchedWhere` 依派發順序讀取符合條件的酬載。帶有 `Where` 的方法接收篩選回呼，依名稱的方法沿用現有簽章。

## 暫存事件

`Dispatcher::push(Arc<dyn Event>)` 暫存事件，`flush(name, context)` 派發對應的 `_pushed` 監聽器。重複 flush 會再次派發，需用 `forgetPushed` 明確清除；這與 `defer`、交易提交後派發不同。應用程式中的暫存資料依請求／佇列作用域隔離；作用域內清除不會刪除其他請求的共享監聽器。

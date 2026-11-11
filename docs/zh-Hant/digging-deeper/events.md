# 事件

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

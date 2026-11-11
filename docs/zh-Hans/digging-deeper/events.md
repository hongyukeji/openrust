# 事件

## 注册监听器与订阅者

在应用中声明注册函数，通过 `withEvents(DiscoverEvents::new(&[register_orders]))` 接入。应用启动时加载编译后的清单。实现 `Subscriber` 后，可用 `Event::subscribe` 成组登记监听器。运行 `./artisan event:list --event=orders --json` 查看注册结果。

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

## 派发事件

调用 `Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?`，也可派发实现 `contracts::events::Event` 的类型。精确匹配的监听器按注册顺序执行，随后执行匹配的通配符组。`ListenerResponse::Stop` 停止传播，`Event::until` 返回首个监听响应。

## 数据库事务提交后派发

通过 `Event::dispatchInTransaction` 显式传入当前事务。用 `Delivery::AfterCommit` 延后单个监听器，或实现 `ShouldDispatchAfterCommit` 并调用 `Event::dispatchAfterCommit` 延后整个事件。保存点成功提交后，回调留到最外层提交执行。回滚会清除受影响范围内的回调。没有活动事务时立即执行。提交后的回调失败以 `DatabaseError::AfterCommit` 报告。

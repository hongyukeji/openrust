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

## 测试 {#testing}

`TestApp::fakeEvents` 在当前测试应用记录选中的事件分发。处于应用作用域时也可调用 `Event::fake`。传空切片选中所有事件，传入事件全名限定范围，或调用 `except` 让指定事件继续交给监听器。

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

被 fake 的事件跳过监听器、队列和广播。提交后分发的事件在事务提交后才记录，回滚会丢弃事件。用 `dispatched` 取得已记录的负载并检查具体类型。替身随测试应用释放，也可通过再次安装 fake 替换。

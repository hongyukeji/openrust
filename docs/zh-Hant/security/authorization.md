# 授權

## Gate

透過應用程式的 `GateRegistry` 註冊權限。`define` 要求使用者已驗證。訪客也可以參與判斷時，使用 `defineGuest`。

```rust
use openrust::auth::{AuthIdentifier, access::{GateRegistry, Response}};

fn register_settings_gate(gate: &GateRegistry) {
    gate.define("edit-settings", |user, _| {
        Box::pin(async move {
            Ok(Response::from(
                user.getAuthIdentifier() == AuthIdentifier::String("owner".into()),
            ))
        })
    });
}
```

## 策略

執行 `./artisan make:policy AccountPolicy --model=User` 產生策略。將產生的模組接入應用程式，在服務提供者中使用 `gate.policy::<User>(AccountPolicy)` 註冊。策略依 Rust 模型型別比對。沒有模型實例的權限可以傳入 `PolicyTarget::of::<User>()`。

## 請求授權

在處理函式中使用 `Gate::authorize(request, "edit-settings", &[]).await?`，或在路由上掛載 `.can("edit-settings", &[])`。模型策略可使用 `.can("update", &["user"])` 接收路由繫結後的 `user`。中介軟體在驗證和模型繫結之後執行授權。

可以在 [Blade 授權指令](../the-basics/blade#authorization-directives)中使用同一組 Gate 權限，決定是否顯示頁面控制項。

## 授權結果與回呼

`inspect` 傳回授權結果。拒絕預設回傳 HTTP 403，可使用 `Response::denyWithStatus` 或 `Response::denyAsNotFound` 更改狀態碼。`before` 可以提前給出結果。權限檢查與 `before` 都沒有給出結果時，`after` 可以補充。

## Gate 判定事件 {#gate-evaluated}

應用程式事件派發器會在 Gate 的 `after` 回呼結束後、套用預設拒絕前派發 `GateEvaluated`。事件包含可為空的使用者、權限名稱和原始 `Option<Response>`，包括明確拒絕的 HTTP 狀態。權限尚未判定時，事件結果為 `None`，即使 `inspect` 傳回已設定的預設拒絕。每項實際評估的權限各派發一次事件。`allowIf` 和 `denyIf` 的內嵌檢查會略過此事件。

註冊同步監聽器後，可以透過 `event.arguments(context)` 按原順序讀取參數。原始參數僅在呼叫期間借用，延後派發和保留的事件可透過 `argument_types` 讀取型別。

```rust
use std::sync::Arc;

use openrust::{
    auth::access::events::GateEvaluated,
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
};

struct RecordGate;

impl Listener for RecordGate {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<GateEvaluated>().ok_or(EventError::InvalidPayload)?;
            let allowed = event.result.as_ref().map(|result| result.allowed());
            let argument_count = event.arguments(context).map(|arguments| arguments.len());
            eprintln!("{} {allowed:?} {argument_count:?}", event.ability);
            Ok(None)
        })
    }
}

fn register_gate_listener(events: &Dispatcher) -> EventResult<()> {
    events.listen("GateEvaluated", Arc::new(RecordGate), Delivery::Sync)
}
```


## 回應狀態

透過 `Response::deny("原因").withStatus(StatusCode::CONFLICT)` 設定拒絕狀態，或用 `asNotFound()` 選擇 404。`withStatus(None)` 清除先前設定的狀態。這些 Builder 消費並傳回回應，保留授權結論及原因；給允許的回應設定狀態不會將其改為拒絕。

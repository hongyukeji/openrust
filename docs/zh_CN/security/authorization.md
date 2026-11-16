# 授权

## Gate

通过应用的 `GateRegistry` 注册权限。`define` 要求用户已认证。访客也可以参与判定时，使用 `defineGuest`。

```rust
use openrust::auth::{AuthIdentifier, access::{GateRegistry, Response}};

fn register_settings_gate(gate: &GateRegistry) {
    gate.define("edit-settings", |user, _| {
        Box::pin(async move {
            Ok(Response::from(
                user.get_auth_identifier() == AuthIdentifier::String("owner".into()),
            ))
        })
    });
}
```

## 策略

运行 `./artisan make:policy AccountPolicy --model=User` 生成策略。将生成的模块接入应用，在服务提供者中用 `gate.policy::<User>(AccountPolicy)` 注册。策略按 Rust 模型类型匹配。没有模型实例的权限可以传入 `PolicyTarget::of::<User>()`。

## 请求授权

在处理函数中使用 `Gate::authorize(request, "edit-settings", &[]).await?`，或在路由上挂载 `.can("edit-settings", &[])`。模型策略可用 `.can("update", &["user"])` 接收路由绑定后的 `user`。中间件在认证和模型绑定之后执行授权。

可在 [Blade 授权指令](../the-basics/blade#authorization-directives)中用同一组 Gate 能力决定是否显示页面控件。

## 授权结果与回调

`inspect` 返回授权结果。拒绝默认返回 HTTP 403，可用 `Response::denyWithStatus` 或 `Response::denyAsNotFound` 改变状态码。`before` 可以提前给出结果。权限检查与 `before` 都未给出结果时，`after` 可以补充。

## Gate 判定事件 {#gate-evaluated}

应用事件派发器在 Gate 的 `after` 回调完成后、应用默认拒绝前派发 `GateEvaluated`。载荷包含可为空的用户、能力名和原始 `Option<Response>`，包括显式拒绝的 HTTP 状态。能力未决时，事件的结果为 `None`，即使 `inspect` 返回配置的默认拒绝。每项实际求值的能力各派发一次事件。`allowIf` 和 `denyIf` 的内联检查跳过此事件。

注册同步监听器后，可通过 `event.arguments(context)` 按原顺序读取参数。原始参数只在调用期间借用，延迟派发和保留的事件可通过 `argument_types` 读取类型。

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

# 授权

## Gate

通过应用的 `GateRegistry` 注册权限。`define` 要求用户已认证。访客也可以参与判定时，使用 `define_guest`。

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

## 策略发现与查询

`get_policy_for` 接收模型实例或 `PolicyTarget::of::<User>()`，返回 `Result<Option<PolicyHandle>>`。句柄保留真实策略实例，可用 `downcast::<AccountPolicy>()` 取得 `Arc<AccountPolicy>`；`name()` 返回完整 Rust 类型名。显式 `policy` 映射优先，`policies()` 返回这些映射的独立快照，键为模型 `TypeId`，值为策略类型名。

需要发现策略时，先向应用容器登记构造工厂，再调用 `discover_policy::<User, AccountPolicy>()` 登记已编译类型。它不会提前构造策略或添加显式映射。默认名称按模型模块逐级查找 `policies::<模型名>Policy`；嵌套 `models` 模块会优先查找对应的 `models::policies` 和 `policies` 路径。策略位于单独的文件模块或采用其他布局时，使用 `guess_policy_names_using` 返回实际完整类型名；按顺序选取第一个已登记候选，空列表停用默认发现。

```rust
use std::{any::type_name, sync::Arc};
use openrust::{
    auth::access::PolicyTarget,
    contracts::container::Container as _,
    support::facades::Gate,
};

container.bind::<AccountPolicy, _>(|_| Ok(Arc::new(AccountPolicy)))?;
Gate::discover_policy::<User, AccountPolicy>()?;
Gate::guess_policy_names_using(|model| {
    if model == type_name::<User>() {
        vec![type_name::<AccountPolicy>().into()]
    } else {
        Vec::new()
    }
})?;
let selected = Gate::get_policy_for(&PolicyTarget::of::<User>())?;
let concrete = selected.and_then(|policy| policy.downcast::<AccountPolicy>());
let resolved = Gate::resolve_policy::<AccountPolicy>()?;
```

在 Provider 或其他应用 Facade 作用域内使用上述入口；`container` 是该应用的容器，`User` 和 `AccountPolicy` 是应用自身类型。需要构造依赖时，在容器工厂中解析。`resolve_policy::<AccountPolicy>()` 直接使用已有容器绑定，发现路径也遵循其瞬态、共享及错误语义；构造失败返回错误，不继续尝试后面的候选。策略发现不会在 `before` 已给出结果时构造策略。

`GateRegistry` 提供同名入口，`for_user` 返回的 Gate 提供策略查询、解析、映射及候选回调入口；Gate 契约提供 `get_policy_for`。显式 `policy` 仍使用传入的共享实例。Rust 发现只处理已登记类型，不执行 PHP 类自动加载、属性反射或父类查找；查询结果使用拥有型句柄和 `Result`，映射快照不保证迭代顺序。

## 请求授权

在处理函数中使用 `Gate::authorize(request, "edit-settings", &[]).await?`，或在路由上挂载 `.can("edit-settings", &[])`。模型策略可用 `.can("update", &["user"])` 接收路由绑定后的 `user`。中间件在认证和模型绑定之后执行授权。

可在 [Blade 授权指令](../the-basics/blade#authorization-directives)中用同一组 Gate 能力决定是否显示页面控件。

## 授权结果与回调

`inspect` 返回授权结果。拒绝默认返回 HTTP 403，可用 `Response::deny_with_status` 或 `Response::deny_as_not_found` 改变状态码。`before` 可以提前给出结果。权限检查与 `before` 都未给出结果时，`after` 可以补充。

## Gate 判定事件 {#gate-evaluated}

应用事件派发器在 Gate 的 `after` 回调完成后、应用默认拒绝前派发 `GateEvaluated`。载荷包含可为空的用户、能力名和原始 `Option<Response>`，包括显式拒绝的 HTTP 状态。能力未决时，事件的结果为 `None`，即使 `inspect` 返回配置的默认拒绝。每项实际求值的能力各派发一次事件。`allow_if` 和 `deny_if` 的内联检查跳过此事件。

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


## 响应状态

通过 `Response::deny("原因").with_status(StatusCode::CONFLICT)` 设置拒绝状态，或用 `as_not_found()` 选择 404。`with_status(None)` 清除先前设置的状态。这些 Builder 消费并返回响应，保留授权结论及原因；给允许的响应设置状态不会将其改为拒绝。

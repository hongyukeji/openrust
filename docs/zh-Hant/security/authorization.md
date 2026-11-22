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

## 策略發現與查詢

`getPolicyFor` 接收模型實例或 `PolicyTarget::of::<User>()`，傳回 `Result<Option<PolicyHandle>>`。控制代碼保留真實策略實例，可用 `downcast::<AccountPolicy>()` 取得 `Arc<AccountPolicy>`；`name()` 傳回完整 Rust 型別名稱。顯式 `policy` 對應優先，`policies()` 傳回這些對應的獨立快照，鍵為模型 `TypeId`，值為策略型別名稱。

需要發現策略時，先向應用容器登記建構工廠，再呼叫 `discoverPolicy::<User, AccountPolicy>()` 登記已編譯型別。它不會提前建構策略或新增顯式對應。預設名稱按模型模組逐級尋找 `policies::<模型名>Policy`；巢狀 `models` 模組會優先尋找對應的 `models::policies` 和 `policies` 路徑。策略位於單獨的檔案模組或採用其他配置時，使用 `guessPolicyNamesUsing` 傳回實際完整型別名稱；按順序選取第一個已登記候選，空清單停用預設發現。

```rust
use std::{any::type_name, sync::Arc};
use openrust::{
    auth::access::PolicyTarget,
    contracts::container::Container as _,
    support::facades::Gate,
};

container.bind::<AccountPolicy, _>(|_| Ok(Arc::new(AccountPolicy)))?;
Gate::discoverPolicy::<User, AccountPolicy>()?;
Gate::guessPolicyNamesUsing(|model| {
    if model == type_name::<User>() {
        vec![type_name::<AccountPolicy>().into()]
    } else {
        Vec::new()
    }
})?;
let selected = Gate::getPolicyFor(&PolicyTarget::of::<User>())?;
let concrete = selected.and_then(|policy| policy.downcast::<AccountPolicy>());
let resolved = Gate::resolvePolicy::<AccountPolicy>()?;
```

在 Provider 或其他應用 Facade 作用域內使用上述入口；`container` 是該應用的容器，`User` 和 `AccountPolicy` 是應用自身型別。需要建構相依項目時，在容器工廠中解析。`resolvePolicy::<AccountPolicy>()` 直接使用既有容器綁定，發現路徑也遵循其瞬態、共享及錯誤語義；建構失敗傳回錯誤，不繼續嘗試後面的候選。策略發現不會在 `before` 已給出結果時建構策略。

`GateRegistry` 提供同名入口，`forUser` 傳回的 Gate 提供策略查詢、解析、對應及候選回呼入口；Gate 契約提供 `getPolicyFor`。顯式 `policy` 仍使用傳入的共享實例。Rust 發現只處理已登記型別，不執行 PHP 類別自動載入、屬性反射或父類別查詢；查詢結果使用擁有型控制代碼和 `Result`，對應快照不保證迭代順序。

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

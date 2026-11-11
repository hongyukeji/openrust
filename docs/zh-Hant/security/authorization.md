# 授權

## Gate

透過應用程式的 `GateRegistry` 註冊權限。`define` 要求使用者已驗證。訪客也可以參與判斷時，使用 `defineGuest`。

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

執行 `./artisan make:policy AccountPolicy --model=User` 產生策略。將產生的模組接入應用程式，在服務提供者中使用 `gate.policy::<User>(AccountPolicy)` 註冊。策略依 Rust 模型型別比對。沒有模型實例的權限可以傳入 `PolicyTarget::of::<User>()`。

## 請求授權

在處理函式中使用 `Gate::authorize(request, "edit-settings", &[]).await?`，或在路由上掛載 `.can("edit-settings", &[])`。模型策略可使用 `.can("update", &["user"])` 接收路由繫結後的 `user`。中介軟體在驗證和模型繫結之後執行授權。

## 授權結果與回呼

`inspect` 傳回授權結果。拒絕預設回傳 HTTP 403，可使用 `Response::denyWithStatus` 或 `Response::denyAsNotFound` 更改狀態碼。`before` 可以提前給出結果。權限檢查與 `before` 都沒有給出結果時，`after` 可以補充。

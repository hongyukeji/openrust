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

## 授权结果与回调

`inspect` 返回授权结果。拒绝默认返回 HTTP 403，可用 `Response::denyWithStatus` 或 `Response::denyAsNotFound` 改变状态码。`before` 可以提前给出结果。权限检查与 `before` 都未给出结果时，`after` 可以补充。

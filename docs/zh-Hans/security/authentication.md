# 身份认证

## Guard 与用户提供器

`auth.guards` 配置选择 Guard 及其提供器。`session` Guard 使用当前 Web 请求的 Session。受保护路由使用 `auth` 中间件别名，访客路由使用 `guest`。通过 `Auth::guard(request, None)?.user().await?` 读取身份。每个请求独立保存已解析身份与选中的 Guard。

## 手动认证用户

用应用选定的查询列和密码创建 `Credentials`。`attempt` 验证凭据，按配置升级密码哈希，成功后轮换 Session ID 和 CSRF 令牌。登录处理器可使用 `attempt_throttled`，限流键由规范化账号和可信客户端地址生成。限流器原子记账，达到上限返回带 `Retry-After` 的 429，验证通过后清除计数。

```rust
use openrust::{
    auth::{AuthResult, Credentials},
    http::Request,
    support::facades::Auth,
};

async fn sign_in(
    request: &mut Request,
    credentials: &Credentials,
    throttle_key: &str,
    remember: bool,
) -> AuthResult<bool> {
    Auth::session_guard(request, None)?
        .attempt_throttled(credentials, remember, throttle_key, 5, 60)
        .await
}
```

## 记住用户与登出

将 remember 参数设为 `true` 会签发加密、HTTP-only 的 Cookie。Guard 使用持久化 remember 令牌和当前密码哈希的指纹恢复会话。`logout` 清除当前 Guard 并轮换令牌。若应用还需要丢弃整个会话并轮换 CSRF 令牌，使用 `logout_and_invalidate`。

## HTTP Basic 认证 {#http-basic-authentication}

在 Web 路由上使用 `auth.basic`，通过会话 Guard 认证。默认按 `email` 查找用户。可用 `auth.basic:web,username` 指定 Guard 和查找字段。凭据缺失或无效时，响应为 HTTP 401，并带有 `WWW-Authenticate: Basic`。已有有效会话也可通过中间件。

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_basic_routes() -> Result<(), RoutingError> {
    Route::getWith("/profile", || view("profile"), |route| {
        route.middleware_name("auth.basic");
        Ok(())
    })?;
    Route::getWith("/profile/by-name", || view("profile"), |route| {
        route.middleware_name("auth.basic:web,username");
        Ok(())
    })
}
```

在 API 处理器中调用 `Auth::once_basic(request, "email", &[]).await?` 可仅认证当前请求。请通过 HTTPS 传输 Basic 凭据。

## 认证事件 {#authentication-events}

凭据认证开始时，应用派发 `Attempting`，提供器验证通过后派发 `Validated`。会话登录依次派发 `Login` 和 `Authenticated`。读取已有会话派发 `Authenticated`，通过 remember Cookie 恢复会话时派发带 `remember = true` 的 `Login`。尝试失败时派发 `Failed`，其中可能包含提供器找到的用户。`Logout` 与 `CurrentDeviceLogout` 分别表示普通登出和当前设备登出。

`Attempting` 和 `Failed` 的同步监听器可通过 `credentials(context)` 借用凭据。请避免将密码写入日志。`Validated` 监听器可通过 `user::<T>(context)` 借用类型化用户。

登录触发限流时，`Lockout` 携带 Guard 名和重试等待时间。同步监听器可通过 `request(context)` 借用请求元数据。

```rust
use std::sync::Arc;

use openrust::{
    auth::events::{Lockout, Login},
    contracts::events::{Dispatcher as _, Event},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
};

fn record_auth_event<'a>(
    event: &'a dyn Event,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        if let Some(login) = event.downcast_ref::<Login>() {
            eprintln!("login guard={} remember={}", login.guard, login.remember);
        } else if let Some(lockout) = event.downcast_ref::<Lockout>() {
            eprintln!("lockout guard={} retry_after={}", lockout.guard, lockout.retry_after);
        }
        Ok(None)
    })
}

fn register_auth_events(dispatcher: &Dispatcher) -> EventResult<()> {
    dispatcher.listen("Login", Arc::new(record_auth_event), Delivery::Sync)?;
    dispatcher.listen("Lockout", Arc::new(record_auth_event), Delivery::Sync)
}
```

## 使其他设备的会话失效 {#invalidating-sessions-on-other-devices}

在需要执行会话失效的路由中，将 `auth.session` 放在 `auth` 之后。在已认证的 Web 处理器中验证请求，再调用 `Auth::logout_other_devices(request, password).await?`。Guard 校验当前密码、更新已存储的哈希，按需刷新当前设备的 remember Cookie，并派发 `OtherDeviceLogout`。仍带旧指纹的会话下次访问受保护路由时会被拒绝。

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register_account_routes() -> Result<(), RoutingError> {
    Route::getWith("/account", || view("account"), |route| {
        route.middleware_name("auth").middleware_name("auth.session");
        Ok(())
    })
}
```

## 密码确认 {#password-confirmation}

在敏感 Web 路由上，将 `password.confirm` 放在 `auth` 之后。中间件使用 `auth.password_timeout`，单位为秒，默认 10,800。使用 `password.confirm:password.confirm,300` 可覆盖确认路由和时限。需要确认时，浏览器跳转到命名路由，原目标地址存入会话。JSON 请求收到 HTTP 423。

密码确认表单通过受 CSRF 保护的 POST 提交。`Auth::confirm_password` 通过用户提供器校验当前账号密码，并记录成功确认。自行验证密码后，也可调用 `request.session_mut()?.password_confirmed()?` 记录确认。

```rust
use openrust::{
    auth::Auth,
    http::{HttpError, HttpFuture, Request, Response, StatusCode, abort},
    routing::{RouteGroup, RoutingError},
    support::facades::{Redirect, Route},
    view::view,
};

fn confirm(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let password = request.input(Some("password")).await?;
        let password = password.as_ref().and_then(serde_json::Value::as_str)
            .ok_or(HttpError::InvalidRequest)?;
        if !Auth::confirm_password(request, password).await? {
            return Err(abort(StatusCode::UNPROCESSABLE_ENTITY, "Invalid password.", Default::default()));
        }
        Ok(Redirect::intended(request, "/settings", Default::default())?.into())
    })
}

fn register_password_routes() -> Result<(), RoutingError> {
    Route::getWith("/confirm-password", || view("auth.confirm-password"), |route| {
        route.name("password.confirm").middleware_name("auth");
        Ok(())
    })?;
    Route::group(
        RouteGroup::new().middleware_name("auth").middleware_name("throttle:6,1"),
        |router| {
            router.post("/confirm-password", confirm)?;
            Ok(())
        },
    )?;
    Route::getWith("/settings", || view("settings"), |route| {
        route.middleware_name("auth").middleware_name("password.confirm");
        Ok(())
    })
}
```

## 自定义 Guard 和提供器

认证模型按 机制在 `config/auth.rs` 的 `auth.providers` 中选择。`openrust::auth::model!` 对应 PHP 的模型类引用：返回类名，并保留编译后的类型工厂。`AppServiceProvider` 和 `bootstrap/app.rs` 无需登记模型：

```rust
let user = openrust::auth::model!(crate::app::models::User);
let admin = openrust::auth::model!(crate::app::models::Admin);

let providers = serde_json::json!({
    "users": {"driver": "eloquent", "model": user},
    "admins": {"driver": "eloquent", "model": admin},
});
```

使用前在应用的模型模块中声明 `Admin`。新增模型类引用后须重新构建；`AUTH_MODEL` 可选择已引用且编译的模型，未知类名在解析 Eloquent 提供器时失败。即使配置缓存绕过加载函数，链接器仍保留类型工厂。框架按需解析工厂，不查询用户表，可变认证状态按应用隔离。可用第二个参数指定稳定别名，例如 `model!(Admin, "app::models::Admin")`。`AuthManager::register_model` 保留为显式运行时扩展入口。也可为 `database` 提供器配置表和字段；用 `provider` 注册提供器，用 `via_request` 注册无状态请求解析器。内置 `token` 守卫读取 Bearer 标头，支持将令牌哈希后再查询数据库。

对于通过 `AuthManager::register_model` 注册的身份，`manager.user_morph_class(user)?` 返回真实 Eloquent 多态标识，遵循模型连接及自定义 `getMorphClass`。此 Rust 适配入口保留多态映射错误，未注册身份返回 `None`；非模型操作者应显式配置，不猜测表名或类型名。

同步回调可用 `manager.resolved_user(None)` 或 `Auth::resolved_user(None)?` 读取当前 HTTP 请求中已解析的身份，无需启用日志监听。`None` 遵循 `should_use` 和默认 Guard，`Some("web")` 显式选择指定 Guard。这些 Rust 适配入口不会触发异步认证或查询用户提供器；尚未解析、访客或处于所属请求作用域之外时返回 `None`。需要惰性认证的回调应先在异步中间件中解析 Guard。

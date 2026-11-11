# 会话

## 配置

用 `SESSION_DRIVER=database` 将会话保存在配置的数据库和 `sessions` 表中。使用会话路由前先运行应用迁移。`array` 驱动将数据保存在当前应用进程中。用 `SESSION_ENCRYPT=true` 加密存储载荷。

配置会话驱动后，默认 `web` 组会在处理器运行前启动会话。自定义组需要把 `EncryptCookies` 放在 `StartSession` 前面。

## 使用会话

在路由中间件栈中按顺序配置 `EncryptCookies` 和 `StartSession`，再用 `request.session()` 或 `request.session_mut()` 访问已加载的会话。路由返回后，中间件保存会话并附加会话 Cookie。导入 `Session` trait 即可调用会话方法。

```rust
use openrust::{
    contracts::session::Session,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

fn remember(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let session = request.session_mut()?;
        session.put("profile.name", "Ada".into())?;
        session.flash("message", "Saved".into())?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
```

## 闪存数据

`flash` 写入的值在当前请求和下一次请求可用。用 `keep` 延长指定字段的可用期，或用 `reflash` 将所有闪存数据保留到再下一次请求。

## 重新生成会话 ID

`regenerate(true)` 删除存储中的旧 ID、保留会话属性并轮换 CSRF 令牌。用 `invalidate()` 清空会话属性并轮换 ID。

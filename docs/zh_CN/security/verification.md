# 邮箱验证

## 验证路由

认证用户通过 `Authenticatable` 暴露 `MustVerifyEmail`。派发 `Registered` 时，框架会给具备该能力且尚未验证的用户发送通知。

注册 `verification.notice` 提示页，以及带 `{id}` 和 `{hash}` 参数的 `verification.verify`。验证路由使用 `auth` 和 `signed`。处理器为当前登录账号加载可变模型，再调用 `fulfill`。

```rust
use openrust::{
    contracts::auth::MustVerifyEmail,
    foundation::auth::EmailVerificationRequest,
    http::{HttpError, Request},
};

pub async fn verify(
    request: &mut Request,
    user: &mut dyn MustVerifyEmail,
) -> Result<(), HttpError> {
    EmailVerificationRequest::fulfill(request, user).await
}
```

验证请求核对当前登录用户和当前邮箱摘要。首次验证会保存时间并派发 `Verified`。重复打开有效链接时，时间与事件次数保持不变。链接有效期取 `auth.verification.expire`，默认 60 分钟。

## 重发和保护路由

将重发入口命名为 `verification.send`，使用 `auth` 和 `throttle:6,1`，对尚未验证的用户调用 `sendEmailVerificationNotification`。

使用 `verified` 保护路由。未验证用户请求 HTML 时重定向到 `verification.notice`，请求 JSON 时返回 403。可用 `verified:your.route` 指定提示页。

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

<span id="verification-after-email-change"></span>

验证链接只适用于当前邮箱的准确写法，包括大小写。应用更换邮箱时，应在同一次数据库更新中清除 `email_verified_at`，并发送新链接。保存验证状态时，应以当前邮箱和未设置的验证时间作为更新条件。用 `whereExactText` 匹配邮箱可避免排序规则影响。并发请求中只有写入时间戳的一次会派送 `Verified`。

<span id="default-email-verification-mail"></span>

默认验证邮件提供本地化按钮，纯文本版本包含临时签名链接。

### 自定义验证邮件 {#custom-verification-email}

在服务提供器的启动方法中，用 `VerifyEmail::toMailUsing` 替换邮件内容。回调接收用户和已生成的链接。可通过 `VerifyEmail::createUrlUsing` 在渲染邮件前提供其他链接。目标路由使用签名中间件时，应生成相应的签名 URL。

```rust
use openrust::{
    auth::{AuthResult, notifications::VerifyEmail},
    notifications::messages::{MailDelivery, MailMessage},
};

fn customize_verification_mail() -> AuthResult<()> {
    VerifyEmail::toMailUsing(|_, url| {
        Ok(MailDelivery::Message(Box::new(
            MailMessage::new()
                .subject("Confirm your email")
                .action("Verify email", url),
        )))
    })
}
```

## 重发和保护路由

将重发入口命名为 `verification.send`，使用 `auth` 和 `throttle:6,1`，对尚未验证的用户调用 `sendEmailVerificationNotification`。

使用 `verified` 保护路由。未验证用户请求 HTML 时重定向到 `verification.notice`，请求 JSON 时返回 403。可用 `verified:your.route` 指定提示页。

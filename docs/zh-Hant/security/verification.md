# 信箱驗證

## 驗證路由

認證使用者透過 `Authenticatable` 提供 `MustVerifyEmail`。派送 `Registered` 時，框架會對具備該能力且尚未驗證的使用者傳送通知。

註冊 `verification.notice` 提示頁，以及帶 `{id}` 和 `{hash}` 參數的 `verification.verify`。驗證路由使用 `auth` 和 `signed`。處理器為目前登入的帳號載入可變模型，再呼叫 `fulfill`。

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

驗證請求核對目前登入的使用者和目前的信箱摘要。首次驗證會儲存時間並派送 `Verified`。重複開啟有效連結時，時間與事件次數維持不變。連結有效期取 `auth.verification.expire`，預設 60 分鐘。

<span id="verification-after-email-change"></span>

驗證連結只適用於目前信箱的精確拼法，包括大小寫。應用程式更換信箱時，應在同一次資料庫更新中清除 `email_verified_at`，並傳送新連結。儲存驗證狀態時，應以目前信箱和未設定的驗證時間作為更新條件。用 `whereExactText` 比對信箱可避免定序影響。並行請求中只有寫入時間戳記的一次會派送 `Verified`。

<span id="default-email-verification-mail"></span>

預設驗證郵件提供本地化按鈕，純文字版本包含暫時簽署的連結。

### 自訂驗證郵件 {#custom-verification-email}

在服務提供者的啟動方法中，用 `VerifyEmail::to_mail_using` 替換郵件內容。回呼接收使用者和已產生的連結。可透過 `VerifyEmail::create_url_using` 在算繪郵件前提供其他連結。目標路由使用簽署中介軟體時，應產生相應的簽署 URL。

```rust
use openrust::{
    auth::{AuthResult, notifications::VerifyEmail},
    notifications::messages::{MailDelivery, MailMessage},
};

fn customize_verification_mail() -> AuthResult<()> {
    VerifyEmail::to_mail_using(|_, url| {
        Ok(MailDelivery::Message(Box::new(
            MailMessage::new()
                .subject("Confirm your email")
                .action("Verify email", url),
        )))
    })
}
```

## 重送和保護路由

將重送入口命名為 `verification.send`，使用 `auth` 和 `throttle:6,1`，對尚未驗證的使用者呼叫 `send_email_verification_notification`。

使用 `verified` 保護路由。未驗證使用者請求 HTML 時重新導向到 `verification.notice`，請求 JSON 時回傳 403。可用 `verified:your.route` 指定提示頁。

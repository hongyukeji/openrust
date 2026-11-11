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

## 重送和保護路由

將重送入口命名為 `verification.send`，使用 `auth` 和 `throttle:6,1`，對尚未驗證的使用者呼叫 `sendEmailVerificationNotification`。

使用 `verified` 保護路由。未驗證使用者請求 HTML 時重新導向到 `verification.notice`，請求 JSON 時回傳 403。可用 `verified:your.route` 指定提示頁。

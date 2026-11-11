# 密碼重設

## 請求密碼重設連結

由應用程式註冊接收重設請求和呈現表單的路由。將表單路由命名為 `password.reset` 並宣告 `{token}` 參數。密碼代理透過設定的使用者提供者查找信箱。使用者透過 `Authenticatable` 提供 `CanResetPassword` 能力。

```rust
use openrust::{
    auth::{AuthResult, Credentials, passwords::PasswordResetStatus},
    database::Value,
    support::facades::Password,
};

pub async fn request_reset(email: &str) -> AuthResult<PasswordResetStatus> {
    let credentials = Credentials::new([("email", Value::from(email))], "");
    Password::sendResetLink(&credentials, None).await
}
```

預設使用者透過 `ResetPassword` 傳送連結。可在 `sendResetLink` 的第二個參數傳入自訂傳送回呼。透過回傳狀態的 `as_str()` 取得翻譯鍵。代理對請求連結和未成功的重設套用設定的最短回應時間。

## 重設密碼

應用程式先驗證新密碼和確認欄位，再向 `Password::reset` 傳入查找憑證、權杖、新密碼和持久化回呼。回呼負責雜湊並儲存新密碼、輪替記住我權杖，並可派送 `PasswordReset`。寫入成功後回傳成功。

儲存庫在回呼前保留權杖，使並行請求無法重複使用。回呼成功後刪除保留，失敗時釋放仍屬於這次請求的保留。回呼中斷後，保留可能持續到權杖過期或使用者重新申請連結。

## 權杖儲存

用 `Password::broker(Some("users"))` 選擇代理，或設定 `auth.defaults.passwords`。預設資料表為 `password_reset_tokens`。`expire` 的單位為分鐘，`throttle` 的單位為秒。多執行個體使用快取權杖時，設定 `cache` 驅動和共用儲存。可執行 `./artisan auth:clear-resets` 清除過期的資料庫權杖，命令接受選用的代理名稱。

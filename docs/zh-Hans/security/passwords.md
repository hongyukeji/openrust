# 密码重置

## 请求密码重置链接

由应用注册接收重置请求及渲染表单的路由。将表单路由命名为 `password.reset` 并声明 `{token}` 参数。密码代理通过配置的用户提供者查找邮箱。用户通过 `Authenticatable` 暴露 `CanResetPassword` 能力。

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

默认用户通过 `ResetPassword` 发送链接。可在 `sendResetLink` 的第二个参数传入自定义发送回调。用返回状态的 `as_str()` 获取翻译键。代理对发链请求和未成功的重置应用配置的最短响应时间。

## 重置密码

应用先验证新密码及确认字段，再向 `Password::reset` 传入查找凭据、令牌、新密码和持久化回调。回调负责哈希并保存新密码、轮换记住我令牌，并可派发 `PasswordReset`。写入成功后返回成功。

仓库在回调前预留令牌，使并发请求无法重复使用。回调成功后删除预留，失败时释放仍属于本次请求的预留。回调中断后，预留可能持续到令牌过期或用户重新申请链接。

## 令牌存储

用 `Password::broker(Some("users"))` 选择代理，或配置 `auth.defaults.passwords`。默认数据库表为 `password_reset_tokens`。`expire` 的单位为分钟，`throttle` 的单位为秒。多实例使用缓存令牌时，配置 `cache` 驱动和共享存储。可运行 `./artisan auth:clear-resets` 清理过期数据库令牌，命令接受可选代理名称。

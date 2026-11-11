# Password Reset

## Requesting the password reset link

Register the application routes that accept reset requests and render the reset form. Name the reset form route `password.reset` and give it a `{token}` parameter. The broker looks up the email through its configured user provider. The user exposes `CanResetPassword` through `Authenticatable`.

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

The default user sends the link through `ResetPassword`. Pass a callback as the second argument of `sendResetLink` to customize delivery. Convert the returned status with `as_str()` to translate it. The broker uses the configured response time floor for link requests and unsuccessful resets.

## Resetting the password

Validate the password and confirmation in the application. Pass the lookup credentials, token, new password and persistence callback to `Password::reset`. The callback hashes and saves the password, rotates the remember token, and may dispatch `PasswordReset`. Return success after the write succeeds.

The repository reserves the token before the callback so concurrent requests cannot reuse it. A successful callback removes the reservation. An error releases it unless a newer token has replaced it. An interrupted callback can leave a reservation until expiry or a new link request.

## Token storage

Choose a broker with `Password::broker(Some("users"))` or configure `auth.defaults.passwords`. The default database table is `password_reset_tokens`. The broker uses `expire` in minutes and `throttle` in seconds. Select the `cache` driver and a shared store for cache-backed tokens across instances. Run `./artisan auth:clear-resets` to clear expired database tokens, optionally passing a broker name.

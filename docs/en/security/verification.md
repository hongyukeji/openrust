# Email Verification

## Verification routing

Expose `MustVerifyEmail` through the authenticatable user. Dispatching `Registered` sends a verification notification to an unverified user who exposes that capability.

Register `verification.notice` for the prompt and `verification.verify` with `{id}` and `{hash}` parameters. Apply `auth` and `signed` to the verification route. In the handler, load a mutable model for the authenticated account and call `fulfill`.

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

The verification request checks the authenticated user and the current email digest. A new verification stores the timestamp and dispatches `Verified`. Reusing a valid link leaves the timestamp and event count unchanged. Verification links expire after `auth.verification.expire` minutes, defaulting to 60.

## Resending and protecting routes

Register `verification.send` for the resend endpoint. Apply `auth` and `throttle:6,1`, then call `sendEmailVerificationNotification` for an unverified user.

Apply `verified` to require verification. An unverified user receives a redirect to `verification.notice` for HTML requests and a 403 for JSON requests. To use another prompt route, pass `verified:your.route`.

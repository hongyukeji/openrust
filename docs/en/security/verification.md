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

<span id="verification-after-email-change"></span>

A verification link applies to the exact current email address, including case. When an application changes the address, clear `email_verified_at` in the same database update and send a new link. Persist verification with a conditional update for the current address and an unset verification timestamp. Use `whereExactText` to match the address independently of the database collation. Concurrent requests dispatch `Verified` only for the request that writes the timestamp.

<span id="default-email-verification-mail"></span>

The default verification email uses a localized action button and includes the temporary signed URL in its plain-text version.

### Custom verification emails {#custom-verification-email}

In your service provider’s boot method, register `VerifyEmail::toMailUsing` to replace the email. The callback receives the user and the generated link. Use `VerifyEmail::createUrlUsing` to supply a different link before rendering the email. If the destination route uses signed middleware, generate an appropriately signed URL.

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

## Resending and protecting routes

Register `verification.send` for the resend endpoint. Apply `auth` and `throttle:6,1`, then call `sendEmailVerificationNotification` for an unverified user.

Apply `verified` to require verification. An unverified user receives a redirect to `verification.notice` for HTML requests and a 403 for JSON requests. To use another prompt route, pass `verified:your.route`.

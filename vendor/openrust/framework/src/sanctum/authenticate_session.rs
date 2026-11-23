#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use subtle::ConstantTimeEq as _;

use crate::{
    auth::{Auth, AuthError, AuthManager, session_fingerprint::SessionFingerprint},
    contracts::{config::Repository, http::Middleware, session::Session as _},
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// Invalidates first-party sessions when the account password changes.
pub struct AuthenticateSession {
    guards: Vec<String>,
    signer: SessionFingerprint,
}

impl AuthenticateSession {
    pub fn new(config: &dyn Repository) -> Result<Self, AuthError> {
        let guards = super::guard::session_guards(config)?;
        Ok(Self {
            guards,
            signer: SessionFingerprint::from_config(config),
        })
    }
}

impl Middleware for AuthenticateSession {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !request.hasSession() || Auth::user(request).await?.is_none() {
                return Ok(next.run(request).await);
            }
            let auth = Auth::manager()?;
            let mut session_guards = Vec::new();
            for guard in &self.guards {
                if auth.guard_uses_driver(request, guard, "session")? {
                    session_guards.push(guard);
                }
            }
            let mut invalid = false;
            for guard in &session_guards {
                let key = format!("password_hash_{guard}");
                let Some(stored) = request.session()?.get(&key).cloned() else {
                    continue;
                };
                let user = auth.userForGuard(request, Some(guard.as_str())).await?;
                let valid = user.is_some_and(|user| {
                    let password = user.getAuthPassword();
                    serde_json::from_value::<Vec<u8>>(stored)
                        .is_ok_and(|stored| bool::from(stored.ct_eq(&self.signer.sign(password))))
                });
                if !valid {
                    auth.sessionGuard(request, Some(guard.as_str()))?
                        .logoutCurrentDevice()
                        .await?;
                    invalid = true;
                }
            }
            if invalid {
                request.session_mut()?.flush();
                crate::auth::reset_request(request);
                return Err(HttpError::Unauthenticated);
            }

            let response = next.run(request).await;
            for guard in session_guards {
                if let Some(user) = AuthManager::resolved_user(request, guard) {
                    let fingerprint = self.signer.sign(user.getAuthPassword());
                    request.session_mut()?.put(
                        &format!("password_hash_{guard}"),
                        serde_json::json!(fingerprint),
                    )?;
                    break;
                }
            }
            Ok(response)
        })
    }
}

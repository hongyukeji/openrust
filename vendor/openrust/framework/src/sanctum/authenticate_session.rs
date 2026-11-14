use aws_lc_rs::hmac;
use subtle::ConstantTimeEq as _;

use crate::{
    auth::{Auth, AuthError, AuthManager},
    contracts::{config::Repository, http::Middleware, session::Session as _},
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// Invalidates first-party sessions when the account password changes.
pub struct AuthenticateSession {
    guards: Vec<String>,
    key: hmac::Key,
}

impl AuthenticateSession {
    pub fn new(config: &dyn Repository) -> Result<Self, AuthError> {
        let guards = super::guard::session_guards(config)?;
        let key = config.get("app.key");
        let key = key
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .unwrap_or("base-key-for-password-hash-mac");
        Ok(Self {
            guards,
            key: hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
        })
    }

    fn fingerprint(&self, password: Option<&str>) -> Vec<u8> {
        hmac::sign(&self.key, password.unwrap_or_default().as_bytes())
            .as_ref()
            .to_vec()
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
            let mut invalid = false;
            for guard in &self.guards {
                let key = format!("password_hash_{guard}");
                let Some(stored) = request
                    .session()
                    .map_err(AuthError::backend)?
                    .get(&key)
                    .cloned()
                else {
                    continue;
                };
                let user = auth.userForGuard(request, Some(guard)).await?;
                let expected =
                    self.fingerprint(user.as_ref().and_then(|user| user.get_auth_password()));
                let valid = serde_json::from_value::<Vec<u8>>(stored)
                    .is_ok_and(|stored| user.is_some() && bool::from(stored.ct_eq(&expected)));
                if !valid {
                    auth.sessionGuard(request, Some(guard))?
                        .logoutCurrentDevice()?;
                    invalid = true;
                }
            }
            if invalid {
                request.session_mut().map_err(AuthError::backend)?.flush();
                crate::auth::reset_request(request);
                return Err(HttpError::Unauthenticated);
            }

            let response = next.run(request).await;
            for guard in &self.guards {
                if let Some(user) = AuthManager::resolved_user(request, guard) {
                    let fingerprint = self.fingerprint(user.get_auth_password());
                    request
                        .session_mut()
                        .map_err(AuthError::backend)?
                        .put(
                            &format!("password_hash_{guard}"),
                            serde_json::json!(fingerprint),
                        )
                        .map_err(AuthError::backend)?;
                    break;
                }
            }
            Ok(response)
        })
    }
}

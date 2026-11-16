use subtle::ConstantTimeEq as _;

use crate::{
    auth::{Auth, AuthError, AuthManager, session_fingerprint::SessionFingerprint},
    contracts::{http::Middleware, session::Session as _},
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// Rejects sessions issued before the account password hash changed.
#[derive(Default)]
pub struct AuthenticateSession;

impl Middleware for AuthenticateSession {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !request.hasSession() {
                return Ok(next.run(request).await);
            }
            let manager = Auth::manager()?;
            let guard = AuthManager::selected_guard(request)
                .map(str::to_owned)
                .unwrap_or_else(|| manager.getDefaultDriver());
            if !manager.guard_uses_driver(request, &guard, "session")? {
                return Ok(next.run(request).await);
            }
            let user = manager.userForGuard(request, Some(&guard)).await?;
            let Some(password) = user.as_deref().and_then(|user| user.get_auth_password()) else {
                return Ok(next.run(request).await);
            };

            let signer = SessionFingerprint::from_config(manager.config());
            let fingerprint = signer.sign(Some(password));
            let key = SessionFingerprint::session_key(&guard);
            let session = request.session_mut().map_err(AuthError::backend)?;
            match session.get(&key) {
                Some(stored)
                    if !serde_json::from_value::<Vec<u8>>(stored.clone())
                        .is_ok_and(|stored| bool::from(stored.ct_eq(&fingerprint))) =>
                {
                    let result = manager
                        .sessionGuard(request, Some(&guard))?
                        .logoutCurrentDevice()
                        .await;
                    request.session_mut().map_err(AuthError::backend)?.flush();
                    result?;
                    return Err(HttpError::Unauthenticated);
                }
                Some(_) => {}
                None => session
                    .put(&key, serde_json::json!(fingerprint))
                    .map_err(AuthError::backend)?,
            }

            let response = next.run(request).await;
            let user = AuthManager::resolved_user(request, &guard);
            if let Some(password) = user.as_deref().and_then(|user| user.get_auth_password()) {
                let value = signer.sign(Some(password));
                request
                    .session_mut()
                    .map_err(AuthError::backend)?
                    .put(&key, serde_json::json!(value))
                    .map_err(AuthError::backend)?;
            }
            Ok(response)
        })
    }
}

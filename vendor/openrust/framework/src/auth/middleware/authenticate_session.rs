use subtle::ConstantTimeEq as _;

use crate::{
    auth::{Auth, AuthManager, session_fingerprint::SessionFingerprint},
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
            if !request.has_session() {
                return Ok(next.run(request).await);
            }
            let manager = Auth::manager()?;
            let guard = AuthManager::selected_guard(request)
                .map(str::to_owned)
                .unwrap_or_else(|| manager.get_default_driver());
            if !manager.guard_uses_driver(request, &guard, "session")? {
                return Ok(next.run(request).await);
            }
            let user = manager.user_for_guard(request, Some(&guard)).await?;
            let Some(user) = user else {
                return Ok(next.run(request).await);
            };
            let signer = SessionFingerprint::for_guard(manager.config(), &guard)?;
            if user.get_auth_password().is_none()
                && signer.credential_version(user.as_ref())?.is_none()
            {
                return Ok(next.run(request).await);
            }
            let fingerprint = signer.sign_for_guard(user.as_ref(), &guard)?;
            let key = SessionFingerprint::session_key(&guard);
            let session = request.session_mut()?;
            match session.get(&key) {
                Some(stored)
                    if !serde_json::from_value::<Vec<u8>>(stored.clone())
                        .is_ok_and(|stored| bool::from(stored.ct_eq(&fingerprint))) =>
                {
                    let result = manager
                        .session_guard(request, Some(&guard))?
                        .logout_current_device()
                        .await;
                    request.session_mut()?.flush();
                    result?;
                    return Err(HttpError::Unauthenticated);
                }
                Some(_) => {}
                None if manager.config().get("auth.session_fingerprint_on_login")
                    == Some(serde_json::json!(true))
                    && crate::auth::credential_scope::guard_applies(manager.config(), &guard)? =>
                {
                    manager
                        .session_guard(request, Some(&guard))?
                        .logout_current_device()
                        .await?;
                    request.session_mut()?.flush();
                    return Err(HttpError::Unauthenticated);
                }
                None => session.put(&key, serde_json::json!(fingerprint))?,
            }

            let response = next.run(request).await;
            let user = AuthManager::resolved_user_from_request(request, &guard);
            if let Some(user) = user {
                let value = signer.sign_for_guard(user.as_ref(), &guard)?;
                request.session_mut()?.put(&key, serde_json::json!(value))?;
            }
            Ok(response)
        })
    }
}

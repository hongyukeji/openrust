//! Cookie与Token登录共用已验的因素验证；Cookie阶段额外绑定当前会话。
use super::{
    challenges::{ChallengeStore, Pending},
    client, credentials,
    passkey_confirmation::CredentialProof,
};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::{
        auth::{Authenticatable, StatefulGuard as _},
        session::Session as _,
    },
    http::{HttpError, HttpFuture, Request, Response},
    sanctum::HasApiTokens,
    support::facades::{Auth, Facade as _},
};
use serde_json::{Value, json};
use std::sync::Arc;
const STAGE: &str = "login.user_challenge";
#[derive(Clone)]
pub(super) struct CookieLogin {
    binding: String,
}
pub(super) fn is_cookie(request: &Request) -> bool {
    request.parts().extensions.get::<CookieLogin>().is_some()
}
pub(super) fn check(
    request: &Request,
    pending: &Pending,
    fortify: &Fortify,
) -> Result<(), HttpError> {
    let cookie = pending.payload.get("cookie") == Some(&Value::Bool(true));
    let Some(context) = request.parts().extensions.get::<CookieLogin>() else {
        return if cookie {
            Err(HttpError::Unauthenticated)
        } else {
            Ok(())
        };
    };
    if !cookie
        || pending
            .payload
            .get("cookie_client")
            .and_then(Value::as_str)
            .is_none_or(|binding| !client::same_binding(binding, &context.binding))
        || !client::same_binding(
            &context.binding,
            &super::confirmation::session_binding(request, fortify)?,
        )
    {
        return Err(HttpError::Unauthenticated);
    }
    Ok(())
}
pub(super) fn record<'a, M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &'a mut Request,
    user: &'a dyn Authenticatable,
) -> HttpFuture<'a, Result<(), HttpError>> {
    Box::pin(async move {
        let user = (user as &dyn std::any::Any)
            .downcast_ref::<M>()
            .ok_or(HttpError::Unauthenticated)?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let evidence = request
            .parts()
            .extensions
            .get::<super::primary::Evidence>()
            .cloned()
            .or_else(|| {
                request
                    .parts()
                    .extensions
                    .get::<CredentialProof>()
                    .cloned()
                    .map(Into::into)
            });
        let mut payload = json!({"cookie":true,"cookie_client":super::confirmation::session_binding(request,&fortify)?,"remember":request.session().map_err(internal)?.get("login.remember").cloned().unwrap_or(false.into()),"passkey":null});
        if let Some(proof) = &evidence {
            proof.write(&mut payload)?;
        }
        let pending = Pending {
            user: user.get_auth_identifier(),
            owner: user.get_morph_class()?.into(),
            purpose: evidence
                .as_ref()
                .map_or("login", super::primary::Evidence::purpose)
                .into(),
            credential: credentials::stamp(user)?,
            factor: client::security(user, &fortify)?,
            token: None,
            payload,
        };
        let lifetime = match request
            .parts()
            .extensions
            .get::<super::socialite::Deadline>()
        {
            Some(end) => end
                .0
                .checked_sub(
                    openrust::support::facades::Date::now(None)
                        .map_err(internal)?
                        .as_date_time()
                        .timestamp()
                        .as_second(),
                )
                .filter(|seconds| *seconds > 0)
                .ok_or(HttpError::Unauthenticated)?
                .min(300),
            None => 300,
        };
        let stage = ChallengeStore::new(user.get_connection()?)
            .create(&pending, lifetime)
            .await?;
        request
            .session_mut()
            .map_err(internal)?
            .put(STAGE, stage.key.as_str().into())
            .map_err(internal)?;
        Ok(())
    })
}
pub(super) fn complete<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let raw = request
            .session()
            .map_err(internal)?
            .get(STAGE)
            .and_then(Value::as_str)
            .ok_or(HttpError::Unauthenticated)?
            .to_owned();
        let binding = super::confirmation::session_binding(request, &fortify)?;
        request
            .parts_mut()
            .extensions
            .insert(CookieLogin { binding });
        request.merge(json!({"challenge":raw})).await?;
        client::challenge::<M>(request).await
    })
}
pub(super) async fn finish<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    user: &mut M,
    fortify: &Arc<Fortify>,
    pending: &Pending,
    deadline: i64,
) -> Result<Response, HttpError> {
    check(request, pending, fortify)?;
    let binding = pending.factor.clone();
    let policy = fortify.clone();
    let evidence = super::primary::Evidence::read(pending)?;
    credentials::with_locked(user, move |locked, transaction| {
        Box::pin(async move {
            let now = openrust::support::facades::Date::now(None)
                .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?
                .as_date_time()
                .timestamp()
                .as_second();
            if now >= deadline
                || !client::same_binding(&binding, &client::security(locked, &policy)?)
            {
                return Err(HttpError::Unauthenticated.into());
            }
            if let Some(proof) = evidence
                && !proof.current(locked, &policy, transaction).await?
            {
                return Err(HttpError::Unauthenticated.into());
            }
            Ok(())
        })
    })
    .await?;
    let remember = pending
        .payload
        .get("remember")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    request
        .session_mut()
        .map_err(internal)?
        .forget(&[STAGE, "login.id", "login.remember"]);
    Auth::session_guard(request, fortify.guard().as_deref())?
        .login(Box::new(user.clone()), remember)
        .await?;
    request
        .session_mut()
        .map_err(internal)?
        .regenerate(false)
        .await
        .map_err(internal)?;
    Resolver::<dyn crate::contracts::TwoFactorLoginResponse>::get_facade_root()
        .map_err(internal)?
        .to_response(request)
        .await
}

//! 一次性浏览器发起与回调状态复用套件ChallengeStore；供应商state仍由原生SDK核对。
use super::super::challenges::{ChallengeStore, Pending};
use super::{ProviderProfile, providers};
use crate::{
    Fortify,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::AuthIdentifier,
    contracts::session::Session as _,
    http::{HttpError, Request, Response},
    sanctum::HasApiTokens,
    support::facades::{App, Date, Facade as _},
};
use serde_json::{Value, json};

pub(super) const SESSION: &str = "user.socialite.flow";
pub(super) const LINK: &str = "user.socialite.link";
pub(super) fn now() -> Result<i64, HttpError> {
    Ok(Date::now(None)
        .map_err(internal)?
        .as_date_time()
        .timestamp()
        .as_second())
}
pub(super) fn provider(request: &Request, fortify: &Fortify) -> Result<String, HttpError> {
    let provider = request.route("provider").ok_or(HttpError::NotFound)?;
    if !providers(fortify)?.iter().any(|p| p == provider) {
        return Err(HttpError::NotFound);
    }
    Ok(provider.to_owned())
}
pub(super) fn store<M: HasApiTokens>() -> Result<ChallengeStore, HttpError> {
    Ok(ChallengeStore::new(
        M::from_state(Default::default()).get_connection()?,
    ))
}
pub(super) fn guest(provider: &str, mode: &str) -> Pending {
    Pending {
        user: AuthIdentifier::String("guest".into()),
        owner: "user-kit:socialite".into(),
        purpose: "socialite:flow".into(),
        credential: String::new(),
        factor: String::new(),
        token: None,
        payload: json!({"provider":provider,"mode":mode}),
    }
}
pub(super) fn lifetime(deadline: i64) -> Result<i64, HttpError> {
    deadline
        .checked_sub(now()?)
        .filter(|seconds| *seconds > 0 && *seconds <= 300)
        .ok_or(HttpError::Unauthenticated)
}
pub(super) async fn redirect<M: HasApiTokens>(
    request: &mut Request,
    mut pending: Pending,
    deadline: i64,
) -> Result<Response, HttpError> {
    let provider = pending.payload["provider"]
        .as_str()
        .ok_or(HttpError::Unauthenticated)?
        .to_owned();
    pending.purpose = "socialite:flow".into();
    pending.payload["deadline"] = deadline.into();
    let challenge = store::<M>()?.create(&pending, lifetime(deadline)?).await?;
    request
        .session_mut()
        .map_err(internal)?
        .put(SESSION, challenge.key.as_str().into())
        .map_err(internal)?;
    let manager = App::make::<openrust_socialite::SocialiteManager>().await?;
    let mut sdk = manager
        .driver(&provider, request)
        .map_err(internal)?
        .oauth2()
        .map_err(internal)?;
    sdk.enable_pkce();
    Ok(sdk.redirect().map_err(internal)?.into())
}
pub(super) async fn callback<M: HasApiTokens>(
    request: &mut Request,
) -> Result<(Pending, ProviderProfile, i64), HttpError> {
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    let provider = provider(request, &fortify)?;
    let raw = request
        .session_mut()
        .map_err(internal)?
        .pull(SESSION)
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or(HttpError::Unauthenticated)?;
    let store = store::<M>()?;
    let pending = store
        .resolve(&raw, "socialite:flow")
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    if pending.payload["provider"] != provider {
        return Err(HttpError::Unauthenticated);
    }
    let deadline = store
        .consume(&raw, &pending)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    let manager = App::make::<openrust_socialite::SocialiteManager>().await?;
    let mut sdk = manager
        .driver(&provider, request)
        .map_err(internal)?
        .oauth2()
        .map_err(internal)?;
    sdk.enable_pkce();
    // 供应商拒绝、state不符、PKCE失败均返回安全的认证错误，不记录原始响应／令牌。
    let remote = sdk.user().await.map_err(|_| HttpError::Unauthenticated)?;
    lifetime(deadline)?;
    let profile = super::registration::profile(&provider, &remote)?;
    Ok((pending, profile, deadline))
}
pub(super) fn profile(pending: &Pending) -> Result<ProviderProfile, HttpError> {
    serde_json::from_value(
        pending
            .payload
            .get("profile")
            .cloned()
            .unwrap_or(Value::Null),
    )
    .map_err(internal)
}

//! 网页登录及明确确认的账户绑定；回调绝不直接绑定当前用户。
use super::super::{client, confirmation, credentials, primary};
use super::{flow, identity};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::{auth::StatefulGuard as _, session::Session as _},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::HasApiTokens,
    support::facades::{Auth, Facade as _, Redirect, View},
};
use serde_json::{Value, json};

pub(super) fn login<M: HasApiTokens>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let provider = flow::provider(request, &fortify)?;
        flow::redirect::<M>(
            request,
            flow::guest(&provider, "web-login"),
            flow::now()? + 300,
        )
        .await
    })
}
pub(super) fn start_link<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let provider = flow::provider(request, &fortify)?;
        let user = super::super::web::user::<M>(request, &fortify).await?;
        let proof = confirmation::claim_session(request, &user, &fortify).await?;
        let mut pending = flow::guest(&provider, "web-link");
        pending.user = user.get_auth_identifier();
        pending.owner = user.get_morph_class()?.into();
        pending.credential = credentials::stamp(&user)?;
        pending.factor = client::security(&user, &fortify)?;
        pending.payload["client"] = confirmation::session_binding(request, &fortify)?.into();
        let deadline = (flow::now()? + 300).min(proof.deadline());
        flow::redirect::<M>(request, pending, deadline).await
    })
}
pub(super) fn callback<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let result = callback_inner::<M>(request).await;
        if let Err(HttpError::Validation(error)) = &result {
            return Ok(client::no_store(
                Redirect::route("login", &[], Default::default())?
                    .with_errors(request, error.errors(), "default")?
                    .with(request, "status", error.errors().first(None, None).into())?
                    .into(),
            ));
        }
        if let Err(HttpError::Unauthenticated) = &result {
            // 不使用guest/back，避免把含OAuth code/state的回调保存为intended并形成循环。
            return Ok(client::no_store(
                Redirect::route("login", &[], Default::default())?
                    .with(request, "status", "user.provider_failed".into())?
                    .into(),
            ));
        }
        result
    })
}
async fn callback_inner<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> Result<Response, HttpError> {
    let (mut pending, profile, deadline) = flow::callback::<M>(request).await?;
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    match pending.payload["mode"].as_str() {
        Some("web-login") => {
            let (mut user, proof) =
                super::registration::authenticate::<M>(&profile, &fortify).await?;
            let policy = fortify.clone();
            let checked = proof.clone();
            credentials::with_locked(&mut user, move |user, tx| {
                Box::pin(async move {
                    flow::lifetime(deadline)?;
                    if !checked.current(user, &policy, tx).await? {
                        return Err(HttpError::Unauthenticated.into());
                    }
                    Ok(())
                })
            })
            .await?;
            request
                .parts_mut()
                .extensions
                .insert(primary::Evidence::from(proof));
            request
                .parts_mut()
                .extensions
                .insert(super::Deadline(deadline));
            let limiter =
                Resolver::<crate::LoginRateLimiter>::get_facade_root().map_err(internal)?;
            if let Some(response) =
                crate::actions::RedirectIfTwoFactorAuthenticatable::new(fortify.clone(), limiter)
                    .challenge(request, &user)
                    .await?
            {
                return Ok(client::no_store(response));
            }
            Auth::session_guard(request, fortify.guard().as_deref())?
                .login(Box::new(user), false)
                .await?;
            request
                .session_mut()
                .map_err(internal)?
                .regenerate(false)
                .await
                .map_err(internal)?;
            Ok(client::no_store(
                Redirect::to(&fortify.redirects("login", None), Default::default())?.into(),
            ))
        }
        Some("web-link") => {
            let user = super::super::web::user::<M>(request, &fortify).await?;
            validate(request, &user, &fortify, &pending)?;
            confirmation::claim_session(request, &user, &fortify).await?;
            pending.purpose = "socialite:web-link".into();
            pending.payload["profile"] = serde_json::to_value(&profile)?;
            let challenge = flow::store::<M>()?
                .create(&pending, flow::lifetime(deadline)?)
                .await?;
            request
                .session_mut()
                .map_err(internal)?
                .put(flow::LINK, challenge.key.as_str().into())
                .map_err(internal)?;
            let view = View::make(
                "user.connected-account-confirm",
                [("profile".into(), serde_json::to_value(profile)?.into())].into(),
            )?;
            Response::view(StatusCode::OK, view, Default::default(), request)
                .await
                .map(client::no_store)
        }
        Some("client-login" | "client-link") => {
            super::api::callback::<M>(request, pending, profile, deadline).await
        }
        _ => Err(HttpError::Unauthenticated),
    }
}
fn validate<M: HasApiTokens + TwoFactorAuthenticatable>(
    request: &Request,
    user: &M,
    fortify: &Fortify,
    pending: &super::super::challenges::Pending,
) -> Result<(), HttpError> {
    if pending.owner != user.get_morph_class()?
        || serde_json::to_value(&pending.user)? != serde_json::to_value(user.get_auth_identifier())?
        || !client::same_binding(&pending.credential, &credentials::stamp(user)?)
        || !client::same_binding(&pending.factor, &client::security(user, fortify)?)
        || pending.payload["client"].as_str().is_none_or(|client| {
            confirmation::session_binding(request, fortify)
                .map_or(true, |b| !super::super::client::same_binding(client, &b))
        })
    {
        return Err(HttpError::Unauthenticated);
    }
    Ok(())
}
pub(super) fn confirm<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = super::super::web::user::<M>(request, &fortify).await?;
        let raw = request
            .session_mut()
            .map_err(internal)?
            .pull(flow::LINK)
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?;
        let store = flow::store::<M>()?;
        let pending = store
            .resolve(&raw, "socialite:web-link")
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        validate(request, &user, &fortify, &pending)?;
        let deadline = store
            .consume(&raw, &pending)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        if request.boolean("confirm").await? {
            let proof = confirmation::claim_session(request, &user, &fortify).await?;
            let profile = flow::profile(&pending)?;
            credentials::with_locked(&mut user, move |user, tx| {
                Box::pin(async move {
                    flow::lifetime(deadline)?;
                    proof.validate_in_transaction(user, &fortify, tx).await?;
                    identity::link(user, tx, &profile.provider, &profile.subject).await?;
                    Ok(())
                })
            })
            .await?;
        }
        Ok(client::no_store(
            Redirect::route("user.providers.page", &[], Default::default())?.into(),
        ))
    })
}
pub(super) fn index<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let user = super::super::web::user::<M>(request, &fortify).await?;
        let bound = identity::for_user(&user).await?;
        let profiles=super::providers(&fortify)?.into_iter().map(|provider|json!({"linked":bound.iter().any(|i|i.provider==provider),"provider":provider})).collect::<Vec<_>>();
        let view = View::make(
            "user.connected-accounts",
            [("providers".into(), Value::Array(profiles).into())].into(),
        )?;
        Response::view(StatusCode::OK, view, Default::default(), request)
            .await
            .map(client::no_store)
    })
}
pub(super) fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let provider = flow::provider(request, &fortify)?;
        let mut user = super::super::web::user::<M>(request, &fortify).await?;
        let proof = confirmation::claim_session(request, &user, &fortify).await?;
        credentials::with_locked(&mut user, move |user, tx| {
            Box::pin(async move {
                proof.validate_in_transaction(user, &fortify, tx).await?;
                super::ensure_other_primary(user, tx, &provider, &fortify).await?;
                identity::unlink(user, tx, &provider).await?;
                Ok(())
            })
        })
        .await?;
        Ok(client::no_store(
            Redirect::route("user.providers.page", &[], Default::default())?.into(),
        ))
    })
}

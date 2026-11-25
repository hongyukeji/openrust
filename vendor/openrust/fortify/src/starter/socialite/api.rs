//! 无Cookie客户端以S256握手交换登录结果；浏览器回调不签发PAT，也不自动绑定账户。
use super::super::{
    account::{self, Actor},
    challenges::Pending,
    client,
    confirmation::{self, Purpose},
    credentials,
};
use super::{ProviderProfile, flow, identity};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::validation::Validator as _,
    http::{ApiResponse, HttpError, HttpFuture, Request, Response},
    sanctum::HasApiTokens,
    support::facades::{Facade as _, Redirect, Url, Validator},
    validation::{Data, RuleSet},
};
use serde_json::{Value, json};
use zeroize::Zeroizing;

fn verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}
fn challenge(value: &str) -> String {
    // S256使用已有URL安全编码器；不把客户端verifier保存到队列、会话或数据库。
    let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, value.as_bytes());
    {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.as_ref())
    }
}
fn redirect_allowed(fortify: &Fortify, value: &str) -> Result<(), HttpError> {
    let allowed = fortify
        .config
        .get("user.socialite.api_redirects")
        .unwrap_or(json!([]));
    let list = allowed.as_array().ok_or_else(|| {
        internal(std::io::Error::other(
            "user.socialite.api_redirects必须为地址数组",
        ))
    })?;
    if !list.iter().any(|item| item.as_str() == Some(value)) {
        return Err(client::failed(
            "redirect_uri",
            "user.provider_redirect_invalid",
        ));
    }
    let uri = url::Url::parse(value)
        .map_err(|_| client::failed("redirect_uri", "user.provider_redirect_invalid"))?;
    if uri.fragment().is_some()
        || !uri.username().is_empty()
        || uri.password().is_some()
        || value.bytes().any(|byte| byte.is_ascii_control())
        || !matches!(uri.scheme(), "http" | "https")
        || (uri.scheme() == "http"
            && !matches!(uri.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
        || (uri.scheme() == "https" && uri.host_str().is_none())
    {
        return Err(client::failed(
            "redirect_uri",
            "user.provider_redirect_invalid",
        ));
    }
    Ok(())
}
pub(super) fn begin<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    begin_flow::<M>(request, false)
}
pub(super) fn begin_link<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    begin_flow::<M>(request, true)
}
fn begin_flow<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    link: bool,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([
                (
                    "code_challenge",
                    "required|string|size:43|regex:/^[A-Za-z0-9_-]+$/",
                ),
                ("redirect_uri", "required|string|max:2048"),
                (
                    "device_name",
                    if link {
                        "sometimes|string|max:255"
                    } else {
                        "required|string|max:255"
                    },
                ),
                ("abilities", "sometimes|array|max:100"),
            ])?,
        )?
        .validate()
        .await?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let provider = flow::provider(request, &fortify)?;
        let redirect = request
            .input("redirect_uri")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::InvalidRequest)?;
        redirect_allowed(&fortify, &redirect)?;
        let mut pending = flow::guest(&provider, if link { "client-link" } else { "client-login" });
        let mut deadline = flow::now()? + 300;
        if link {
            let user = account::user::<M>(request).await?;
            let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
            let proof = confirmation::claim(request, &user, Purpose::Factors, &fortify).await?;
            deadline = deadline.min(proof.deadline());
            pending.payload["confirming_passkey"] = serde_json::to_value(proof.passkey())?;
            pending.user = user.get_auth_identifier();
            pending.owner = user.get_morph_class()?.into();
            pending.credential = credentials::stamp(&user)?;
            pending.factor = client::security(&user, &fortify)?;
            pending.token = actor.id;
            pending.payload["client"] = actor.client.into();
        }
        pending.purpose = "socialite:handoff".into();
        pending.payload["code_challenge"] = request
            .input("code_challenge")
            .await?
            .unwrap_or(Value::Null);
        pending.payload["redirect_uri"] = redirect.into();
        pending.payload["device_name"] = request.input("device_name").await?.unwrap_or(Value::Null);
        pending.payload["abilities"] = request.input("abilities").await?.unwrap_or(Value::Null);
        let ticket = flow::store::<M>()?
            .create(&pending, flow::lifetime(deadline)?)
            .await?;
        let handoff = Url::route(
            "user.providers.handoff",
            &[
                openrust::routing::RouteParameter::named("provider", provider.as_str()),
                openrust::routing::RouteParameter::named("challenge", ticket.key.as_str()),
            ],
            true,
        )?;
        Ok(client::no_store(
            ApiResponse::success(json!({"url":handoff,"expires_at":ticket.expires_at}))
                .into_response()?,
        ))
    })
}
pub(super) fn handoff<M: HasApiTokens>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let provider = flow::provider(request, &fortify)?;
        let raw = request
            .query(Some("challenge"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?;
        let store = flow::store::<M>()?;
        let pending = store
            .resolve(&raw, "socialite:handoff")
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        if pending.payload["provider"] != provider {
            return Err(HttpError::Unauthenticated);
        }
        let deadline = store
            .consume(&raw, &pending)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        flow::redirect::<M>(request, pending, deadline).await
    })
}
pub(super) async fn callback<M: HasApiTokens>(
    request: &mut Request,
    mut pending: Pending,
    profile: ProviderProfile,
    deadline: i64,
) -> Result<Response, HttpError> {
    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
    let redirect = pending.payload["redirect_uri"]
        .as_str()
        .ok_or(HttpError::Unauthenticated)?
        .to_owned();
    redirect_allowed(&fortify, &redirect)?;
    pending.purpose = "socialite:exchange".into();
    pending.payload["profile"] = serde_json::to_value(profile)?;
    let code = flow::store::<M>()?
        .create(&pending, flow::lifetime(deadline)?)
        .await?;
    let mut target = url::Url::parse(&redirect).map_err(internal)?;
    target.set_fragment(Some(&format!("code={}", code.key.as_str())));
    let _ = request;
    Ok(client::no_store(
        Redirect::to(target.as_str(), Default::default())?.into(),
    ))
}
async fn consume<M: HasApiTokens>(request: &mut Request) -> Result<(Pending, i64), HttpError> {
    let raw = Zeroizing::new(
        request
            .input("code")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?,
    );
    let verifier = Zeroizing::new(
        request
            .input("code_verifier")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?,
    );
    if !self::verifier(&verifier) {
        return Err(HttpError::Unauthenticated);
    }
    let store = flow::store::<M>()?;
    let pending = store
        .resolve(&raw, "socialite:exchange")
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    if pending.payload["code_challenge"]
        .as_str()
        .is_none_or(|expected| !client::same_binding(expected, &challenge(&verifier)))
    {
        return Err(HttpError::Unauthenticated);
    }
    let deadline = store
        .consume(&raw, &pending)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    Ok((pending, deadline))
}
pub(super) fn exchange<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let (pending, deadline) = consume::<M>(request).await?;
        if pending.payload["mode"] != "client-login" {
            return Err(HttpError::Unauthenticated);
        }
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let profile = flow::profile(&pending)?;
        if !super::providers(&fortify)?.contains(&profile.provider) {
            return Err(HttpError::Unauthenticated);
        }
        let (mut user, proof) = super::registration::authenticate::<M>(&profile, &fortify).await?;
        request.merge(json!({"device_name":pending.payload["device_name"],"abilities":pending.payload["abilities"]})).await?;
        client::complete_primary(
            request,
            &mut user,
            &fortify,
            Some(proof.into()),
            Some(deadline),
        )
        .await
    })
}
pub(super) fn preview<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let (mut pending, deadline) = consume::<M>(request).await?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        if pending.payload["mode"] != "client-link" {
            return Err(HttpError::Unauthenticated);
        }
        actor.validate(&user, &fortify, &pending)?;
        let profile = flow::profile(&pending)?;
        pending.purpose = "socialite:confirm-link".into();
        let code = flow::store::<M>()?
            .create(&pending, flow::lifetime(deadline)?)
            .await?;
        Ok(client::no_store(ApiResponse::success(json!({"challenge":code.key.as_str(),"provider":profile.provider,"name":profile.name,"email":profile.verified_email,"expires_at":code.expires_at})).into_response()?))
    })
}
pub(super) fn confirm<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        let raw = request
            .input("challenge")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?;
        let store = flow::store::<M>()?;
        let pending = store
            .resolve(&raw, "socialite:confirm-link")
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        actor.validate(&user, &fortify, &pending)?;
        let deadline = store
            .consume(&raw, &pending)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        if request.boolean("confirm").await? {
            let profile = flow::profile(&pending)?;

            credentials::with_locked(&mut user, move |user, tx| {
                Box::pin(async move {
                    flow::lifetime(deadline)?;
                    actor.check(user, tx, Some("account:update")).await?;
                    actor.validate(user, &fortify, &pending)?;
                    if let Some(value) = pending
                        .payload
                        .get("confirming_passkey")
                        .filter(|v| !v.is_null())
                    {
                        let proof: super::super::passkey_confirmation::CredentialProof =
                            serde_json::from_value(value.clone())?;
                        if !proof.current(user, &fortify, Some(tx)).await? {
                            return Err(HttpError::Unauthenticated.into());
                        }
                    }
                    if !super::providers(&fortify)?.contains(&profile.provider) {
                        return Err(HttpError::Unauthenticated.into());
                    }
                    identity::link(user, tx, &profile.provider, &profile.subject).await?;
                    Ok(())
                })
            })
            .await?;
        }
        Ok(client::no_store(
            ApiResponse::success(json!({"confirmed":request.boolean("confirm").await?}))
                .into_response()?,
        ))
    })
}
pub(super) fn index<M: HasApiTokens + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let user = account::user::<M>(request).await?;
        account::actor(request, &user, Some("account:read"))?;
        let linked = identity::for_user(&user).await?;
        let items=super::providers(&fortify)?.iter().map(|provider|json!({"provider":provider,"linked":linked.iter().any(|i|&i.provider==provider)})).collect::<Vec<_>>();
        Ok(client::no_store(
            ApiResponse::success(items).into_response()?,
        ))
    })
}
pub(super) fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let provider = flow::provider(request, &fortify)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        let proof = confirmation::claim(request, &user, Purpose::Factors, &fortify).await?;
        credentials::with_locked(&mut user, move |user, tx| {
            Box::pin(async move {
                actor.check(user, tx, Some("account:update")).await?;
                proof.validate_in_transaction(user, &fortify, tx).await?;
                super::ensure_other_primary(user, tx, &provider, &fortify).await?;
                identity::unlink(user, tx, &provider).await?;
                Ok(())
            })
        })
        .await?;
        Ok(client::no_store(Response::no_content()))
    })
}

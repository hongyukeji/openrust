//! 账户资料与密码API；应用Action与证明共用同连接用户事务。
use super::{
    account, client,
    confirmation::{self, Purpose},
    credentials,
};
use crate::{
    Fortify, TwoFactorAuthenticatable, contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{AuthError, AuthIdentifier, passwords::Password},
    contracts::auth::Guard as _,
    database::DatabaseManager,
    http::{HttpError, HttpFuture, Request, Response},
    sanctum::{HasApiTokens, Sanctum},
    support::facades::{App, Auth, Facade as _},
};
use std::sync::Arc;

fn json_only(request: &mut Request) {
    request
        .parts_mut()
        .headers
        .insert("accept", "application/json".parse().expect("静态头值"));
}
pub(super) async fn cookie_guard<M: HasApiTokens>(
    request: &mut Request,
    user: &M,
    cookie: bool,
    fortify: &Fortify,
) -> Result<Option<String>, HttpError> {
    if !cookie {
        return Ok(None);
    }
    let name = fortify.guard().unwrap_or_else(|| {
        fortify
            .config
            .get("auth.defaults.guard")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "web".into())
    });
    let mut guard = Auth::session_guard(request, Some(&name))?;
    let current = guard.user().await?.ok_or(HttpError::Unauthenticated)?;
    if current.get_auth_identifier() != user.get_auth_identifier()
        || (current as &dyn std::any::Any).type_id() != std::any::TypeId::of::<M>()
    {
        return Err(HttpError::Unauthenticated);
    }
    Ok(Some(name))
}
async fn synchronize<M: HasApiTokens + Clone>(
    request: &mut Request,
    name: Option<&str>,
    before: &AuthIdentifier,
    user: &M,
) -> Result<(), HttpError> {
    if let Some(name) = name {
        Auth::session_guard(request, Some(name))?
            .synchronize_user(before, Box::new(user.clone()))
            .await?;
        Auth::guard(request, Some("sanctum"))?.set_user(Box::new(user.clone()));
    }
    Ok(())
}
fn result_model<M: HasApiTokens + Clone>(
    result: &dyn contracts::User,
    locked: &M,
    transaction: &openrust::database::Transaction,
) -> Result<M, HttpError> {
    let updated = (result as &dyn std::any::Any)
        .downcast_ref::<M>()
        .cloned()
        .ok_or(HttpError::Unauthenticated)?;
    if !updated.state().exists()
        || updated.get_key() != locked.get_key()
        || !transaction.belongs_to(&updated.get_connection()?)
    {
        return Err(internal(std::io::Error::other(
            "账户Action返回了不同用户或连接",
        )));
    }
    Ok(updated)
}

pub fn profile<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    profile_inner::<M>(request, false)
}
pub(super) fn web_profile<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    profile_inner::<M>(request, true)
}
fn profile_inner<M>(request: &mut Request, web: bool) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        if !web {
            json_only(request);
        }
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = if web {
            super::web::user::<M>(request, &fortify).await?
        } else {
            account::user::<M>(request).await?
        };
        let actor = if web {
            None
        } else {
            account::actor(request, &user, Some("account:update"))?
        };
        let guard = cookie_guard(request, &user, actor.is_none(), &fortify).await?;
        let original = user.get_auth_identifier();
        let old_email = user.get_auth_attribute("email")?;
        if fortify.truthy("fortify.lowercase_usernames") {
            crate::actions::CanonicalizeUsername::new(fortify.clone())
                .handle(request)
                .await?;
        }
        let input = request.all().await?;
        let updater = fortify
            .profile_updater
            .read()
            .expect("Fortify actions lock")
            .clone();
        let updater = match updater {
            Some(action) => action,
            None => Resolver::<dyn contracts::UpdatesUserProfileInformation>::get_facade_root()
                .map_err(internal)?,
        };
        updater.validate_for_transaction(&user, &input).await?;
        let proof = if web {
            confirmation::claim_session(request, &user, &fortify)?
        } else {
            confirmation::claim(request, &user, Purpose::Email, &fortify).await?
        };
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let policy = fortify.clone();
        credentials::with_locked(&mut user, move |locked, transaction| {
            Box::pin(async move {
                proof
                    .validate_in_transaction(locked, &policy, transaction)
                    .await?;
                account::check_actor(
                    locked,
                    transaction,
                    &sanctum,
                    &database,
                    actor,
                    Some("account:update"),
                    &[],
                )
                .await?;
                let before = locked.clone();
                let result = updater
                    .update_in_transaction(locked, &input, transaction)
                    .await?;
                let mut updated = result_model::<M>(result.as_ref(), locked, transaction)?;
                updated.refresh_for_update(transaction).await?;
                if updated.get_auth_password() != before.get_auth_password()
                    || credentials::version(&updated)? != credentials::version(&before)?
                {
                    return Err(AuthError::Configuration("资料Action不能更改密码或凭据版本"));
                }
                if updated.get_auth_attribute("email")? != before.get_auth_attribute("email")?
                    && updated
                        .as_email_verification()
                        .is_some_and(|user| user.has_verified_email())
                {
                    return Err(AuthError::Configuration("邮箱变更后必须清除验证状态"));
                }
                // 授权的邮箱变化会改变安全摘要；用锁定前快照复核期限，避免误拒合法变更。
                proof
                    .validate_in_transaction(&before, &policy, transaction)
                    .await?;
                *locked = updated;
                Ok(())
            })
        })
        .await?;
        synchronize(request, guard.as_deref(), &original, &user).await?;
        if old_email != user.get_auth_attribute("email")?
            && let Some(verifiable) = user.as_email_verification()
        {
            verifiable.send_email_verification_notification().await?;
        }
        crate::http::controllers::respond::<dyn contracts::ProfileInformationUpdatedResponse>(
            request,
        )
        .await
        .map(client::no_store)
    })
}

pub fn password<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    password_inner::<M>(request, false)
}
pub(super) fn web_password<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    password_inner::<M>(request, true)
}
fn password_inner<M>(
    request: &mut Request,
    web: bool,
) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        if !web {
            json_only(request);
        }
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = if web {
            super::web::user::<M>(request, &fortify).await?
        } else {
            account::user::<M>(request).await?
        };
        let actor = if web {
            None
        } else {
            account::actor(request, &user, Some("account:update"))?
        };
        let guard = cookie_guard(request, &user, actor.is_none(), &fortify).await?;
        let original = user.get_auth_identifier();
        let input = request.all().await?;
        let updater = fortify
            .password_updater
            .read()
            .expect("Fortify actions lock")
            .clone();
        let updater = match updater {
            Some(action) => action,
            None => Resolver::<dyn contracts::UpdatesUserPasswords>::get_facade_root()
                .map_err(internal)?,
        };
        updater.validate_for_transaction(&user, &input).await?;
        let proof = if web {
            confirmation::claim_session(request, &user, &fortify)?
        } else {
            confirmation::claim(request, &user, Purpose::Password, &fortify).await?
        };
        if web
            && user
                .get_auth_password()
                .is_some_and(|password| !password.is_empty())
        {
            use openrust::{
                support::facades::Validator,
                validation::{Data, Rule, RuleSet},
            };
            let rules = RuleSet::new().field(
                "current_password",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::current_password(guard.as_deref()),
                ],
            )?;
            Validator::make(Data::from_request(request).await?, rules)?
                .validate_request(request)
                .await
                .map_err(|mut error| {
                    if let openrust::validation::ValidationError::Invalid(exception) = &mut error {
                        exception.error_bag("updatePassword");
                    }
                    error
                })?;
        }
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let policy = fortify.clone();
        credentials::with_locked(&mut user, move |locked, transaction| {
            Box::pin(async move {
                proof
                    .validate_in_transaction(locked, &policy, transaction)
                    .await?;
                account::check_actor(
                    locked,
                    transaction,
                    &sanctum,
                    &database,
                    actor,
                    Some("account:update"),
                    &[],
                )
                .await?;
                let before = locked.clone();
                let result = updater
                    .update_in_transaction(locked, &input, transaction)
                    .await?;
                let mut updated = result_model::<M>(result.as_ref(), locked, transaction)?;
                updated.refresh_for_update(transaction).await?;
                if updated.get_auth_identifier() != before.get_auth_identifier()
                    || updated.get_auth_password().is_none()
                    || updated.get_auth_password() == before.get_auth_password()
                    || credentials::version(&updated)? <= credentials::version(&before)?
                    || updated.get_remember_token().is_some()
                {
                    return Err(AuthError::Configuration(
                        "密码Action必须保存新密码、递增凭据版本并清空remember token",
                    ));
                }
                proof
                    .validate_in_transaction(&before, &policy, transaction)
                    .await?;
                *locked = updated;
                Ok(())
            })
        })
        .await?;
        synchronize(request, guard.as_deref(), &original, &user).await?;
        let broker = fortify
            .config
            .get("fortify.passwords")
            .and_then(|value| value.as_str().map(str::to_owned));
        Password::broker(broker.as_deref())?
            .delete_token(
                user.as_password_reset()
                    .ok_or(AuthError::Configuration("用户必须支持密码重置"))?,
            )
            .await?;
        crate::actions::emit(
            crate::events::PasswordUpdatedViaController {
                user: Arc::new(user),
            },
            &[],
        )
        .await?;
        crate::http::controllers::respond::<dyn contracts::PasswordUpdateResponse>(request)
            .await
            .map(client::no_store)
    })
}

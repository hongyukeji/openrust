//! 本人令牌管理；复用原生Sanctum模型，用户锁内重新检查当前凭据和近期证明。
use super::{
    UserResource, client,
    confirmation::{self, Purpose},
    credentials::{self, Grant},
};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{AuthIdentifier, AuthResult},
    contracts::validation::Validator as _,
    database::{DatabaseManager, Transaction},
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::{AccessToken, HasApiTokens, Sanctum},
    support::facades::{App, Auth, Facade as _, Validator},
    validation::{Data, RuleSet},
};
use serde_json::json;

async fn user<M: HasApiTokens + Clone>(request: &mut Request) -> Result<M, HttpError> {
    let user = Auth::user(request)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    (user.as_ref() as &dyn std::any::Any)
        .downcast_ref::<M>()
        .cloned()
        .ok_or_else(|| internal(std::io::Error::other("用户套件与认证Provider模型不一致")))
}
fn actor<M: HasApiTokens>(
    request: &Request,
    user: &M,
    ability: Option<&str>,
) -> Result<Option<AuthIdentifier>, HttpError> {
    match user.current_access_token(request) {
        Some(AccessToken::Personal(token)) => {
            if ability.is_some_and(|ability| !token.can(ability)) {
                return Err(HttpError::MissingAbility(
                    openrust::sanctum::MissingAbilityException::new([
                        ability.unwrap_or("tokens:manage")
                    ]),
                ));
            }
            Ok(Some(token.id.clone()))
        }
        Some(AccessToken::Transient) => Ok(None),
        _ => Err(HttpError::Unauthenticated),
    }
}
fn owner_id<M: HasApiTokens>(user: &M) -> AuthResult<AuthIdentifier> {
    AuthIdentifier::try_from(
        user.get_key()
            .ok_or(openrust::auth::AuthError::InvalidIdentity)?,
    )
}
async fn check_actor<M: HasApiTokens>(
    user: &M,
    transaction: &mut Transaction,
    sanctum: &Sanctum,
    database: &DatabaseManager,
    actor: Option<AuthIdentifier>,
    ability: Option<&str>,
    chosen: &[String],
) -> AuthResult<()> {
    let Some(actor) = actor else { return Ok(()) };
    let tokens = sanctum.personal_access_tokens(database, "")?;
    let token = tokens
        .find_owned_in_transaction(transaction, user.get_morph_class()?, owner_id(user)?, actor)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    let now = openrust::support::facades::Date::now(None)
        .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?
        .as_date_time()
        .timestamp();
    if !sanctum.validate_token_for_owner(&token, user, now)? {
        return Err(HttpError::Unauthenticated.into());
    }
    if ability.is_some_and(|ability| !token.can(ability))
        || chosen.iter().any(|ability| !token.can(ability))
    {
        return Err(
            HttpError::MissingAbility(openrust::sanctum::MissingAbilityException::new([
                ability.unwrap_or("tokens:manage")
            ]))
            .into(),
        );
    }
    Ok(())
}

/// 纯Token退出只撤销本次实际认证凭据，不接受客户端提供的目标ID。
pub fn logout<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + Clone,
{
    Box::pin(async move {
        let mut user = user::<M>(request).await?;
        let id = actor(request, &user, None)?.ok_or(HttpError::Unauthenticated)?;
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        credentials::with_locked(&mut user, move |locked, transaction| {
            Box::pin(async move {
                check_actor(
                    locked,
                    transaction,
                    &sanctum,
                    &database,
                    Some(id.clone()),
                    None,
                    &[],
                )
                .await?;
                let tokens = sanctum.personal_access_tokens(&database, "")?;
                if !tokens
                    .revoke_token_in_transaction(
                        transaction,
                        locked.get_morph_class()?,
                        owner_id(locked)?,
                        id,
                    )
                    .await?
                {
                    return Err(HttpError::Unauthenticated.into());
                }
                Ok(())
            })
        })
        .await?;
        Ok(client::no_store(Response::empty(StatusCode::NO_CONTENT)))
    })
}

pub fn tokens<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + Clone,
{
    Box::pin(async move {
        let user = user::<M>(request).await?;
        let actor = actor(request, &user, Some("tokens:manage"))?;
        let sanctum = App::make::<Sanctum>().await?;
        let db = App::make::<DatabaseManager>().await?;
        let tokens = user.tokens(&sanctum, &db)?.get().await?;
        let data=tokens.into_iter().map(|token|json!({"id":token.id,"name":token.name,"abilities":token.abilities,"created_at":token.created_at,"last_used_at":token.last_used_at,"expires_at":token.expires_at,"current":actor.as_ref().is_some_and(|id|serde_json::to_value(id).ok()==serde_json::to_value(&token.id).ok())})).collect::<Vec<_>>();
        Ok(client::no_store(
            ApiResponse::success(data).into_response()?,
        ))
    })
}

pub fn create_token<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([
                ("device_name", "required|string|max:255"),
                ("abilities", "sometimes|array|max:100"),
            ])?,
        )?
        .validate()
        .await?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = user::<M>(request).await?;
        let actor = actor(request, &user, Some("tokens:manage"))?;
        let name = request
            .input("device_name")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let requested = request.input("abilities").await?;
        let mut chosen = client::abilities(&user, &fortify, requested.as_ref())?;
        if requested.is_none()
            && let Some(AccessToken::Personal(parent)) = user.current_access_token(request)
        {
            chosen.retain(|ability| parent.can(ability));
            if chosen.is_empty() {
                return Err(HttpError::MissingAbility(
                    openrust::sanctum::MissingAbilityException::new(["tokens:manage"]),
                ));
            }
        }
        let proof = confirmation::claim(request, &user, Purpose::Tokens, &fortify).await?;
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let policy = fortify.clone();
        let checked = chosen.clone();
        let checked_sanctum = sanctum.clone();
        let checked_database = database.clone();
        let refs = chosen.iter().map(String::as_str).collect::<Vec<_>>();
        let token = credentials::issue_checked(
            &mut user,
            &sanctum,
            &database,
            fortify.config.as_ref(),
            Grant {
                name: &name,
                abilities: &refs,
                expires_at: None,
            },
            move |locked, transaction| {
                let proof = proof.clone();
                let policy = policy.clone();
                let checked = checked.clone();
                let checked_sanctum = checked_sanctum.clone();
                let checked_database = checked_database.clone();
                let actor = actor.clone();
                Box::pin(async move {
                    proof.validate(locked, &policy)?;
                    client::abilities(locked, &policy, Some(&json!(checked)))
                        .map_err(openrust::auth::AuthError::from)?;
                    check_actor(
                        locked,
                        transaction,
                        &checked_sanctum,
                        &checked_database,
                        actor,
                        Some("tokens:manage"),
                        &checked,
                    )
                    .await?;
                    proof.validate(locked, &policy)
                })
            },
        )
        .await?;
        Ok(client::no_store(ApiResponse::success(json!({"token_type":"Bearer","token":token.plain_text_token.as_str(),"expires_at":token.access_token.expires_at,"user":UserResource::make(&user)?})).with_status(StatusCode::CREATED).into_response()?))
    })
}

pub fn revoke_token<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = user::<M>(request).await?;
        let actor = actor(request, &user, Some("tokens:manage"))?;
        let id = request
            .route("token")
            .ok_or(HttpError::NotFound)?
            .to_owned();
        let proof = confirmation::claim(request, &user, Purpose::Tokens, &fortify).await?;
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        credentials::with_locked(&mut user, move |locked, transaction| {
            Box::pin(async move {
                proof.validate(locked, &fortify)?;
                check_actor(
                    locked,
                    transaction,
                    &sanctum,
                    &database,
                    actor,
                    Some("tokens:manage"),
                    &[],
                )
                .await?;
                let store = sanctum.personal_access_tokens(&database, "")?;
                if store
                    .find_owned_in_transaction(
                        transaction,
                        locked.get_morph_class()?,
                        owner_id(locked)?,
                        AuthIdentifier::String(id.clone()),
                    )
                    .await?
                    .is_none()
                {
                    return Err(HttpError::NotFound.into());
                }
                proof.validate(locked, &fortify)?;
                if !store
                    .revoke_token_in_transaction(
                        transaction,
                        locked.get_morph_class()?,
                        owner_id(locked)?,
                        AuthIdentifier::String(id),
                    )
                    .await?
                {
                    return Err(HttpError::NotFound.into());
                }
                proof.validate(locked, &fortify)
            })
        })
        .await?;
        Ok(client::no_store(Response::empty(StatusCode::NO_CONTENT)))
    })
}

/// 近期重新认证后撤销全部旧凭据；持久化版本覆盖全部节点与会话存储。
pub fn logout_all<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        use openrust::contracts::session::Session as _;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = user::<M>(request).await?;
        let actor = actor(request, &user, Some("account:update"))?;
        let cookie = actor.is_none();
        let proof = confirmation::claim(request, &user, Purpose::Devices, &fortify).await?;
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let policy = fortify.clone();
        credentials::with_locked(&mut user, move |locked, transaction| {
            Box::pin(async move {
                proof.validate(locked, &policy)?;
                check_actor(
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
                credentials::rotate_locked(locked, transaction, None).await?;
                // 自身变更有意使旧证明失效；提交前仍检查原证明的期限及配置。
                proof.validate(&before, &policy)
            })
        })
        .await?;
        if cookie {
            openrust::auth::Auth::session_guard(
                request,
                Some(
                    &fortify
                        .guard()
                        .unwrap_or_else(|| fortify.string("auth.defaults.guard", "web")),
                ),
            )?
            .logout_current_device()
            .await?;
            let session = request.session_mut()?;
            session.invalidate().await?;
            session.regenerate_token().map_err(internal)?;
        }
        Ok(client::no_store(Response::empty(StatusCode::NO_CONTENT)))
    })
}

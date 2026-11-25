//! 无会话客户端的通行密钥流程；服务器状态留在既有一次性挑战仓储。
use super::{CredentialProof, typed};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    passkeys::{self, AuthenticationOptions, Passkeys, RegistrationOptions},
    resolver::{Resolver, internal},
    starter::{
        account,
        challenges::{ChallengeStore, Pending},
        client,
        confirmation::{self, Purpose},
        credentials, devices,
    },
};
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::{session::Session as _, validation::Validator as _},
    database::{DatabaseManager, Transaction},
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::{HasApiTokens, Sanctum},
    support::facades::{App, Date, Facade as _, Validator},
    validation::{Data, RuleSet},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn now_ms() -> Result<i64, HttpError> {
    i64::try_from(
        Date::now(None)
            .map_err(internal)?
            .as_date_time()
            .timestamp()
            .as_nanosecond()
            .div_euclid(1_000_000),
    )
    .map_err(internal)
}
fn identity_equal(a: &AuthIdentifier, b: &AuthIdentifier) -> AuthResult<bool> {
    Ok(serde_json::to_value(a)? == serde_json::to_value(b)?)
}
async fn credential(request: &mut Request) -> Result<Value, HttpError> {
    request
        .input("credential")
        .await?
        .filter(Value::is_object)
        .ok_or_else(|| client::failed("credential", "validation.required"))
}
async fn raw(request: &mut Request) -> Result<String, HttpError> {
    request
        .input("challenge")
        .await?
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or(HttpError::Unauthenticated)
}
async fn stage<M: HasApiTokens>(
    user: Option<&M>,
    purpose: &str,
    mut pending: Pending,
    policy: &Passkeys,
    proof_deadline: Option<i64>,
) -> Result<Response, HttpError> {
    let model = M::from_state(Default::default());
    let connection = match user {
        Some(user) => user.get_connection()?,
        None => model.get_connection()?,
    };
    let now = now_ms()?;
    let timeout =
        i64::try_from(policy.timeout().map_err(internal)?.min(300_000)).map_err(internal)?;
    let mut expires = now.checked_add(timeout).ok_or(HttpError::InvalidRequest)?;
    if let Some(deadline) = proof_deadline {
        expires = expires.min(
            deadline
                .checked_mul(1000)
                .ok_or(HttpError::InvalidRequest)?,
        );
    }
    if expires <= now {
        return Err(HttpError::Unauthenticated);
    }
    pending.purpose = purpose.into();
    pending.payload["expires_ms"] = expires.into();
    let options = pending.payload["public_options"].clone();
    let created = ChallengeStore::new(connection)
        .create(&pending, (expires - now + 999) / 1000)
        .await?;
    Ok(client::no_store(ApiResponse::success(json!({"options":options,"challenge":created.key.as_str(),"expires_at":jiff::Timestamp::from_millisecond(expires).map_err(internal)?})).into_response()?))
}
fn pending<M: HasApiTokens + TwoFactorAuthenticatable>(
    user: &M,
    fortify: &Fortify,
    actor: &Actor,
    payload: Value,
) -> AuthResult<Pending> {
    let mut payload = payload;
    payload["client"] = actor.client.clone().into();
    Ok(Pending {
        user: user.get_auth_identifier(),
        owner: user.get_morph_class()?.into(),
        purpose: String::new(),
        credential: credentials::stamp(user)?,
        factor: client::security(user, fortify)?,
        token: actor.id.clone(),
        payload,
    })
}
async fn consume<M: HasApiTokens>(
    request: &mut Request,
    user: Option<&M>,
    purpose: &str,
) -> Result<Pending, HttpError> {
    let model = M::from_state(Default::default());
    let connection = match user {
        Some(user) => user.get_connection()?,
        None => model.get_connection()?,
    };
    let store = ChallengeStore::new(connection);
    let raw = raw(request).await?;
    let pending = store
        .resolve(&raw, purpose)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    let now = now_ms()?;
    if pending
        .payload
        .get("expires_ms")
        .and_then(Value::as_i64)
        .is_none_or(|deadline| deadline <= now)
    {
        return Err(HttpError::Unauthenticated);
    }
    store
        .consume(&raw, &pending)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    Ok(pending)
}
struct Actor {
    id: Option<AuthIdentifier>,
    client: String,
    session: Option<String>,
    sanctum: Arc<Sanctum>,
    database: Arc<DatabaseManager>,
}
impl Actor {
    async fn new<M: HasApiTokens>(
        request: &mut Request,
        user: &M,
        fortify: &Fortify,
        ability: Option<&str>,
    ) -> Result<Self, HttpError> {
        let id = account::actor(request, user, ability)?;
        let (_, client) = confirmation::binding(request, user, fortify)?;
        let session = if id.is_none() {
            Some(request.session().map_err(internal)?.get_id().into())
        } else {
            None
        };
        Ok(Self {
            id,
            client,
            session,
            sanctum: App::make::<Sanctum>().await?,
            database: App::make::<DatabaseManager>().await?,
        })
    }
    async fn check<M: HasApiTokens>(
        &self,
        user: &M,
        transaction: &mut Transaction,
        ability: Option<&str>,
    ) -> AuthResult<()> {
        account::check_actor(
            user,
            transaction,
            &self.sanctum,
            &self.database,
            self.id.clone(),
            ability,
            &[],
        )
        .await?;
        if let Some(session) = &self.session
            && let Some(policy) = devices::policy()?
        {
            policy
                .check_in_transaction(user, transaction, session)
                .await?;
        }
        Ok(())
    }
    fn validate<M: HasApiTokens + TwoFactorAuthenticatable>(
        &self,
        user: &M,
        fortify: &Fortify,
        pending: &Pending,
    ) -> AuthResult<()> {
        let tokens = match (&self.id, &pending.token) {
            (None, None) => true,
            (Some(a), Some(b)) => identity_equal(a, b)?,
            _ => false,
        };
        if !tokens
            || !identity_equal(&pending.user, &user.get_auth_identifier())?
            || pending.owner != user.get_morph_class()?
            || !client::same_binding(&pending.credential, &credentials::stamp(user)?)
            || !client::same_binding(&pending.factor, &client::security(user, fortify)?)
            || pending
                .payload
                .get("client")
                .and_then(Value::as_str)
                .is_none_or(|binding| !client::same_binding(binding, &self.client))
        {
            return Err(HttpError::Unauthenticated.into());
        }
        Ok(())
    }
}

pub(super) fn login_options<M: HasApiTokens>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
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
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let options = passkeys::actions::GenerateVerificationOptions(service.clone())
            .call(None)
            .await?;
        let body = Pending {
            user: AuthIdentifier::String("guest".into()),
            owner: "user-kit:passkey-login".into(),
            purpose: String::new(),
            credential: String::new(),
            factor: String::new(),
            token: None,
            payload: json!({"state":options,"public_options":options.options(),"device_name":request.input("device_name").await?,"abilities":request.input("abilities").await?}),
        };
        stage::<M>(None, "passkey:login", body, &service, None).await
    })
}
pub(super) fn login<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let pending = consume::<M>(request, None, "passkey:login").await?;
        let value = credential(request).await?;
        let options: AuthenticationOptions =
            serde_json::from_value(pending.payload["state"].clone()).map_err(internal)?;
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let actions = super::Actions::<M>(service.clone(), std::marker::PhantomData);
        use passkeys::contracts::PasskeyActions as _;
        let record = actions.verify(request, value, &options, None).await?;
        let mut user = typed::<M>(
            service
                .user_for(&record)
                .await?
                .ok_or(HttpError::Unauthenticated)?
                .as_ref(),
        )?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let verified = request
            .parts()
            .extensions
            .get::<super::VerifiedAccount>()
            .ok_or(HttpError::Unauthenticated)?;
        if !client::same_binding(&verified.0, &client::security(&user, &fortify)?)
            || !service.allows_login(request, &user, &record).await?
        {
            return Err(HttpError::Unauthenticated);
        }
        request.merge(json!({"device_name":pending.payload["device_name"],"abilities":pending.payload["abilities"]})).await?;
        let proof = CredentialProof::from_record(&service, &record).await?;
        client::complete_primary(request, &mut user, &fortify, Some(proof)).await
    })
}

pub(super) fn enrollment_options<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        let proof = confirmation::claim(request, &user, Purpose::Factors, &fortify).await?;
        let deadline = proof.deadline();
        let confirming_passkey = proof.passkey();
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let action = passkeys::actions::GenerateRegistrationOptions(service.clone());
        let policy = fortify.clone();
        let (options, user) = {
            let options = credentials::with_locked(&mut user, move |user, transaction| {
                Box::pin(async move {
                    proof
                        .validate_in_transaction(user, &policy, transaction)
                        .await?;
                    actor
                        .check(user, transaction, Some("account:update"))
                        .await?;
                    action
                        .call_in_transaction(user, transaction)
                        .await
                        .map_err(AuthError::from)
                })
            })
            .await?;
            (options, user)
        };
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        let body = pending(
            &user,
            &fortify,
            &actor,
            json!({"state":options,"public_options":options.options(),"confirming_passkey":confirming_passkey}),
        )?;
        stage(
            Some(&user),
            "passkey:enroll",
            body,
            &service,
            Some(deadline),
        )
        .await
    })
}
async fn staged_proof<M: HasApiTokens + TwoFactorAuthenticatable>(
    pending: &Pending,
    user: &M,
    fortify: &Fortify,
    transaction: &mut Transaction,
) -> AuthResult<()> {
    let now = now_ms()?;
    if pending.payload["expires_ms"]
        .as_i64()
        .is_none_or(|deadline| deadline <= now)
    {
        return Err(HttpError::Unauthenticated.into());
    }
    let proof = pending
        .payload
        .get("confirming_passkey")
        .filter(|v| !v.is_null())
        .cloned()
        .map(serde_json::from_value::<CredentialProof>)
        .transpose()?;
    if let Some(proof) = proof
        && !proof.current(user, fortify, Some(transaction)).await?
    {
        return Err(HttpError::Unauthenticated.into());
    }
    Ok(())
}
pub(super) fn enroll<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([("name", "required|string|max:255")])?,
        )?
        .validate()
        .await?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        let pending = consume(request, Some(&user), "passkey:enroll").await?;
        actor.validate(&user, &fortify, &pending)?;
        let name = request
            .input("name")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(HttpError::InvalidRequest)?;
        let value = credential(request).await?;
        let options: RegistrationOptions =
            serde_json::from_value(pending.payload["state"].clone()).map_err(internal)?;
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let record = credentials::with_locked(&mut user, move |user, transaction| {
            Box::pin(async move {
                actor.validate(user, &fortify, &pending)?;
                actor
                    .check(user, transaction, Some("account:update"))
                    .await?;
                staged_proof(&pending, user, &fortify, transaction).await?;
                let record = passkeys::actions::StorePasskey(service)
                    .call_in_transaction(user, transaction, &name, value, &options)
                    .await?;
                user.refresh_for_update(transaction).await?;
                actor.validate(user, &fortify, &pending)?;
                staged_proof(&pending, user, &fortify, transaction).await?;
                actor
                    .check(user, transaction, Some("account:update"))
                    .await?;
                Ok(record)
            })
        })
        .await?;
        Ok(client::no_store(
            ApiResponse::success(summary(&record).await?)
                .with_status(StatusCode::CREATED)
                .into_response()?,
        ))
    })
}
async fn summary(record: &passkeys::PasskeyRecord) -> Result<Value, HttpError> {
    let id = AuthIdentifier::try_from(&record.get_key().await.ok_or(HttpError::NotFound)?)?;
    let name = match record.get_attribute_value("name").await.map_err(internal)? {
        Some(openrust::database::Value::Text(name)) => name,
        _ => return Err(HttpError::InvalidRequest),
    };
    Ok(json!({"id":id,"name":name}))
}
pub(super) fn index<M: HasApiTokens + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = account::user::<M>(request).await?;
        account::actor(request, &user, Some("account:read"))?;
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let mut list = Vec::new();
        for record in service.for_user(&user).await? {
            list.push(summary(&record).await?);
        }
        Ok(client::no_store(
            ApiResponse::success(json!(list)).into_response()?,
        ))
    })
}
pub(super) fn delete<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, Some("account:update")).await?;
        let proof = confirmation::claim(request, &user, Purpose::Factors, &fortify).await?;
        let key = request
            .route("passkey")
            .ok_or(HttpError::NotFound)?
            .to_owned();
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        credentials::with_locked(&mut user, move |user, transaction| {
            Box::pin(async move {
                proof
                    .validate_in_transaction(user, &fortify, transaction)
                    .await?;
                actor
                    .check(user, transaction, Some("account:update"))
                    .await?;
                let record = service
                    .find_owned_in_transaction(user, transaction, key.into())
                    .await?
                    .ok_or(HttpError::NotFound)?;
                passkeys::actions::DeletePasskey
                    .call_in_transaction(user, transaction, &record)
                    .await?;
                user.refresh_for_update(transaction).await?;
                proof.validate(user, &fortify)?;
                actor
                    .check(user, transaction, Some("account:update"))
                    .await?;
                Ok(())
            })
        })
        .await?;
        Ok(client::no_store(Response::no_content()))
    })
}

pub(super) fn confirmation_options<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let purpose = request
            .input("purpose")
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .filter(|v| Purpose::parse(v).is_some())
            .ok_or_else(|| client::failed("purpose", "validation.in"))?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, None).await?;
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let action = passkeys::actions::GenerateVerificationOptions(service.clone());
        let options = credentials::with_locked(&mut user, move |user, transaction| {
            Box::pin(async move {
                actor.check(user, transaction, None).await?;
                action
                    .call_in_transaction(user, transaction)
                    .await
                    .map_err(AuthError::from)
            })
        })
        .await?;
        let actor = Actor::new(request, &user, &fortify, None).await?;
        let body = pending(
            &user,
            &fortify,
            &actor,
            json!({"state":options,"public_options":options.options(),"purpose":purpose}),
        )?;
        stage(Some(&user), "passkey:confirm", body, &service, None).await
    })
}
pub(super) fn confirm<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = account::user::<M>(request).await?;
        let actor = Actor::new(request, &user, &fortify, None).await?;
        let pending = consume(request, Some(&user), "passkey:confirm").await?;
        actor.validate(&user, &fortify, &pending)?;
        let purpose = pending
            .payload
            .get("purpose")
            .and_then(Value::as_str)
            .and_then(Purpose::parse)
            .ok_or(HttpError::Unauthenticated)?;
        let options: AuthenticationOptions =
            serde_json::from_value(pending.payload["state"].clone()).map_err(internal)?;
        let value = credential(request).await?;
        let policy = fortify.clone();
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let action = passkeys::actions::VerifyPasskey(service.clone());
        let record = credentials::with_locked(&mut user, move |user, transaction| {
            Box::pin(async move {
                actor.validate(user, &policy, &pending)?;
                actor.check(user, transaction, None).await?;
                staged_proof(&pending, user, &policy, transaction).await?;
                let record = action
                    .call_in_transaction(user, transaction, value, &options)
                    .await?;
                user.refresh_for_update(transaction).await?;
                actor.validate(user, &policy, &pending)?;
                actor.check(user, transaction, None).await?;
                staged_proof(&pending, user, &policy, transaction).await?;
                Ok(record)
            })
        })
        .await?;
        confirmation::complete(
            request,
            &user,
            &fortify,
            purpose,
            Some(CredentialProof::from_record(&service, &record).await?),
        )
        .await
    })
}
pub(super) fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>()
-> Result<(), openrust::routing::RoutingError> {
    use openrust::{
        routing::{RouteGroup, RoutingError},
        support::facades::{Config, Route},
    };
    let prefix = Config::string_with_default("api.prefix", "api").map_err(|source| {
        RoutingError::ContainerFailed {
            source: openrust::contracts::container::BindingResolutionError::FactoryFailed {
                service: "用户套件 API 前缀",
                source: Box::new(source),
            },
        }
    })?;
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    let throttle = match fortify.config.get("user.limiters.passkeys") {
        None | Some(Value::Null) => "10,1,user-passkeys".to_owned(),
        Some(Value::String(value)) => value,
        Some(Value::Bool(false)) => String::new(),
        _ => return Err(RoutingError::InvalidAction),
    };
    Route::group(
        RouteGroup::new()
            .prefix(prefix)
            .middleware_name("api")
            .middleware_name("sensitive")
            .middleware(Arc::new(crate::starter::client_middleware::NoStore))
            .middleware(Arc::new(crate::starter::client_middleware::BodyCredentials)),
        |router| {
            for (path, action, name) in [
                (
                    "/auth/passkeys/options",
                    login_options::<M>
                        as for<'a> fn(
                            &'a mut Request,
                        )
                            -> HttpFuture<'a, Result<Response, HttpError>>,
                    "user.api.passkey.login-options",
                ),
                ("/auth/passkeys", login::<M>, "user.api.passkey.login"),
            ] {
                let route = router.post(path, action)?;
                if !throttle.is_empty() {
                    route.middleware_name(format!("throttle:{throttle}"));
                }
                route.name(name);
            }
            macro_rules! route {
                ($method:ident,$path:expr,$action:ident,$name:expr) => {
                    let route = router.$method($path, $action::<M>)?;
                    route.middleware_name("auth:sanctum").middleware(Arc::new(
                        crate::starter::client_middleware::UserModel::<M>::new(),
                    ));
                    if !throttle.is_empty() {
                        route.middleware_name(format!("throttle:{throttle}"));
                    }
                    route.name($name);
                };
            }
            route!(get, "/user/passkeys", index, "user.api.passkey.index");
            route!(
                post,
                "/user/passkeys/options",
                enrollment_options,
                "user.api.passkey.registration-options"
            );
            route!(post, "/user/passkeys", enroll, "user.api.passkey.store");
            route!(
                delete,
                "/user/passkeys/{passkey}",
                delete,
                "user.api.passkey.destroy"
            );
            route!(
                post,
                "/auth/passkeys/confirm/options",
                confirmation_options,
                "user.api.passkey.confirm-options"
            );
            route!(
                post,
                "/auth/passkeys/confirm",
                confirm,
                "user.api.passkey.confirm"
            );
            Ok(())
        },
    )
}

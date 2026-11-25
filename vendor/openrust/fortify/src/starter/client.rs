//! 明确选择的无Cookie客户端入口，复用原生Provider、Fortify因素和Sanctum。
use super::{
    UserResource,
    challenges::{ChallengeStore, Pending},
    credentials::{self, Grant},
};
use crate::{
    Features, Fortify, LoginRateLimiter, TwoFactorAuthenticatable,
    actions::{CanonicalizeUsername, EnsureLoginIsNotThrottled},
    resolver::{Resolver, internal},
};
use openrust::{
    auth::AuthResult,
    contracts::{
        auth::{Authenticatable, UserProvider},
        validation::Validator as _,
    },
    database::{DatabaseManager, Value},
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::{HasApiTokens, Sanctum},
    support::{
        Timebox,
        facades::{App, Auth, Facade as _, Lang, Validator},
    },
    validation::{Data, RuleSet},
};
use serde_json::json;
use std::sync::Arc;
use zeroize::Zeroizing;

pub(super) fn provider(fortify: &Fortify) -> AuthResult<Arc<dyn UserProvider>> {
    let guard = fortify
        .guard()
        .or_else(|| {
            fortify
                .config
                .get("auth.defaults.guard")
                .and_then(|value| value.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| "web".into());
    let provider = fortify
        .config
        .get(&format!("auth.guards.{guard}.provider"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(openrust::auth::AuthError::Configuration(
            "用户套件认证Provider",
        ))?;
    Auth::manager()?.create_user_provider(&provider)
}
pub(super) fn failed(field: &str, key: &str) -> HttpError {
    match Lang::get(key, &[("attribute".into(), field.into())].into(), None) {
        Ok(message) => {
            HttpError::Validation(Box::new(crate::http::responses::invalid(field, message)))
        }
        Err(error) => internal(error),
    }
}
pub(super) fn no_store(response: Response) -> Response {
    response.header(
        "cache-control".parse().expect("静态头名称"),
        "no-store".parse().expect("静态头值"),
    )
}
pub(super) fn same_binding(actual: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    bool::from(actual.as_bytes().ct_eq(expected.as_bytes()))
}
fn attribute(user: &dyn Authenticatable, name: &'static str) -> AuthResult<Option<String>> {
    match user.get_auth_attribute(name)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => Ok(Some(value)),
        _ => Err(openrust::auth::AuthError::Configuration(name)),
    }
}
pub(super) fn security<M: TwoFactorAuthenticatable>(
    user: &M,
    fortify: &Fortify,
) -> AuthResult<String> {
    let payload = serde_json::to_vec(&json!({
        "credential":credentials::stamp(user)?,
        "email":attribute(user,"email")?,
        "verified":attribute(user,"email_verified_at")?,
        "verified_state":user.as_email_verification().is_some_and(|account|account.has_verified_email()),
        "secret":attribute(user,"two_factor_secret")?,
        "confirmed":attribute(user,"two_factor_confirmed_at")?,
        "verification":super::configuration::boolean(fortify.config.as_ref(),"user.email_verification",true)?,
        "two_factor":Features::new(fortify.config.clone()).enabled(Features::two_factor_authentication())?,
        "confirm_factor":fortify.confirms_two_factor_authentication()?,
        "active_factor":user.has_enabled_two_factor_authentication(fortify).map_err(|error|openrust::auth::AuthError::Backend(Box::new(error)))?,
    }))?;
    let encrypter = fortify
        .current_encrypter()
        .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?;
    if encrypter.get_key().is_empty() {
        return Err(openrust::auth::AuthError::Configuration(
            "认证证明需要所属应用的加密密钥",
        ));
    }
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, encrypter.get_key());
    let mut context = aws_lc_rs::hmac::Context::with_key(&key);
    context.update(b"user-auth:proof:v1\0");
    context.update(&payload);
    let hash = context.sign();
    Ok(hash
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
pub(super) fn abilities<M: Authenticatable>(
    user: &M,
    fortify: &Fortify,
    requested: Option<&serde_json::Value>,
) -> Result<Vec<String>, HttpError> {
    let whitelist = fortify
        .config
        .get("user.token_abilities")
        .unwrap_or_else(|| json!(["account:read", "account:update", "tokens:manage"]));
    let list = whitelist
        .as_array()
        .filter(|values| values.iter().all(serde_json::Value::is_string))
        .ok_or_else(|| internal(std::io::Error::other("用户令牌能力配置必须为字符串数组")))?;
    let unverified =
        super::configuration::boolean(fortify.config.as_ref(), "user.email_verification", true)?
            && !user
                .as_email_verification()
                .is_some_and(|account| account.has_verified_email());
    let restricted = fortify
        .config
        .get("user.unverified_token_abilities")
        .unwrap_or_else(|| json!(["account:read", "account:update", "tokens:manage"]));
    let restricted = restricted
        .as_array()
        .filter(|values| values.iter().all(serde_json::Value::is_string))
        .ok_or_else(|| {
            internal(std::io::Error::other(
                "未验证用户的令牌能力配置必须为字符串数组",
            ))
        })?;
    let permitted = |ability: &str| {
        !unverified
            || restricted
                .iter()
                .any(|allowed| allowed.as_str() == Some(ability))
    };
    let values = match requested {
        None => list
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|value| permitted(value))
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| failed("abilities", "validation.in"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(failed("abilities", "validation.in")),
    };
    if values.is_empty()
        || values.iter().any(|value| {
            !list.iter().any(|allowed| allowed.as_str() == Some(value)) || !permitted(value)
        })
    {
        return Err(failed("abilities", "validation.in"));
    }
    Ok(values)
}

pub fn login<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let limiter = Resolver::<LoginRateLimiter>::get_facade_root().map_err(internal)?;
        let username = fortify.username();
        Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([
                (username.as_str(), "required|string"),
                ("password", "required|string"),
                ("device_name", "required|string|max:255"),
                ("abilities", "sometimes|array|max:100"),
            ])?,
        )?
        .validate()
        .await?;
        if fortify.truthy("fortify.lowercase_usernames") {
            CanonicalizeUsername::new(fortify.clone())
                .handle(request)
                .await?;
        }
        if let Some(response) = EnsureLoginIsNotThrottled::new(fortify.clone(), limiter.clone())
            .handle(request)
            .await?
        {
            return Ok(no_store(response));
        }
        let proof = crate::actions::credentials(request, &fortify).await?;
        let selected = provider(&fortify)?;
        let user = Timebox::default()
            .call(
                async |_| {
                    if let Some(callback) = fortify.authenticate_callback() {
                        return callback(request).await;
                    }
                    let Some(mut user) = selected.retrieve_by_credentials(&proof).await? else {
                        return Ok(None);
                    };
                    if !selected.validate_credentials(user.as_ref(), &proof).await? {
                        return Ok(None);
                    }
                    let initial_version = credentials::version(user.as_ref())?;
                    if fortify.config.get("hashing.rehash_on_login").is_none()
                        || fortify.truthy("hashing.rehash_on_login")
                    {
                        selected
                            .rehash_password_if_required(user.as_mut(), &proof, false)
                            .await?;
                        let Some(fresh) =
                            selected.retrieve_by_id(&user.get_auth_identifier()).await?
                        else {
                            return Ok(None);
                        };
                        if credentials::version(fresh.as_ref())? != initial_version
                            || !selected
                                .validate_credentials(fresh.as_ref(), &proof)
                                .await?
                        {
                            return Ok(None);
                        }
                        return Ok(Some(fresh));
                    }
                    Ok(Some(user))
                },
                std::time::Duration::from_micros(200_000),
            )
            .await?;
        let Some(user) = user else {
            limiter.increment(request).await?;
            return Err(failed(&username, "auth.failed"));
        };
        let mut user = (user.as_ref() as &dyn std::any::Any)
            .downcast_ref::<M>()
            .ok_or_else(|| {
                internal(std::io::Error::other(
                    "用户套件控制器与认证Provider模型不一致",
                ))
            })?
            .clone();
        let device = request
            .input("device_name")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let requested = request.input("abilities").await?;
        let chosen = abilities(&user, &fortify, requested.as_ref())?;
        let binding = security(&user, &fortify)?;
        limiter.clear(request).await?;
        let has_factor = Features::new(fortify.config.clone())
            .enabled(Features::two_factor_authentication())?
            && user
                .has_enabled_two_factor_authentication(&fortify)
                .map_err(internal)?;
        if has_factor {
            let store = ChallengeStore::new(user.get_connection()?);
            let challenge = store
                .create(
                    &Pending {
                        user: user.get_auth_identifier(),
                        owner: user.get_morph_class()?.into(),
                        purpose: "login".into(),
                        credential: credentials::stamp(&user)?,
                        factor: binding,
                        token: None,
                        payload: json!({"device_name":device,"abilities":chosen}),
                    },
                    300,
                )
                .await?;
            crate::actions::emit(
                crate::events::TwoFactorAuthenticationChallenged {
                    user: user.get_auth_identifier(),
                },
                &[&user],
            )
            .await?;
            let response=ApiResponse::success(json!({"challenge":challenge.key.as_str(),"methods":["totp","recovery_code"],"expires_at":jiff::Timestamp::from_second(challenge.expires_at).map_err(internal)?}))
                .with_field("code","auth_challenge_required").with_message_key("user.challenge_required",Default::default()).into_response()?;
            return Ok(no_store(response));
        }
        issue(&mut user, &fortify, &device, &chosen, &binding, None).await
    })
}

async fn issue<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    user: &mut M,
    fortify: &Arc<Fortify>,
    device: &str,
    chosen: &[String],
    binding: &str,
    deadline: Option<i64>,
) -> Result<Response, HttpError> {
    let sanctum = App::make::<Sanctum>().await?;
    let database = App::make::<DatabaseManager>().await?;
    let borrowed = chosen.iter().map(String::as_str).collect::<Vec<_>>();
    let checked_fortify = fortify.clone();
    let checked_chosen = chosen.to_vec();
    let binding = binding.to_owned();
    let token = credentials::issue_checked(
        user,
        &sanctum,
        &database,
        fortify.config.as_ref(),
        Grant {
            name: device,
            abilities: &borrowed,
            expires_at: None,
        },
        move |locked, _| {
            let checked_fortify = checked_fortify.clone();
            let checked_chosen = checked_chosen.clone();
            let binding = binding.clone();
            Box::pin(async move {
                let fortify = checked_fortify.as_ref();
                let chosen = checked_chosen;
                if let Some(deadline) = deadline {
                    let now = openrust::support::facades::Date::now(None)
                        .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?
                        .as_date_time()
                        .timestamp()
                        .as_second();
                    if now >= deadline {
                        return Err(HttpError::Unauthenticated.into());
                    }
                }
                abilities(locked, fortify, Some(&json!(chosen)))
                    .map_err(openrust::auth::AuthError::from)?;
                if !same_binding(&security(locked, fortify)?, &binding) {
                    return Err(HttpError::Unauthenticated.into());
                }
                Ok(())
            })
        },
    )
    .await?;
    let response=ApiResponse::success(json!({"token_type":"Bearer","token":token.plain_text_token.as_str(),"expires_at":token.access_token.expires_at,"user":UserResource::make(user)?}))
        .with_status(StatusCode::CREATED).into_response()?;
    Ok(no_store(response))
}

pub fn challenge<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        use subtle::ConstantTimeEq as _;
        Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([
                ("challenge", "required|string|size:64"),
                ("code", "nullable|string|max:32"),
                ("recovery_code", "nullable|string|max:255"),
            ])?,
        )?
        .validate()
        .await?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let model = M::from_state(Default::default());
        let store = ChallengeStore::new(model.get_connection()?);
        let raw = request
            .input("challenge")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?;
        let pending = store
            .resolve(&raw, "login")
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let selected = provider(&fortify)?;
        let found = selected
            .retrieve_by_id(&pending.user)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let mut user = (found.as_ref() as &dyn std::any::Any)
            .downcast_ref::<M>()
            .ok_or_else(|| {
                internal(std::io::Error::other(
                    "用户套件控制器与认证Provider模型不一致",
                ))
            })?
            .clone();
        if serde_json::to_value(user.get_auth_identifier())? != serde_json::to_value(&pending.user)?
            || user.get_morph_class()? != pending.owner
            || pending.token.is_some()
            || !same_binding(&credentials::stamp(&user)?, &pending.credential)
            || !same_binding(&security(&user, &fortify)?, &pending.factor)
            || !Features::new(fortify.config.clone())
                .enabled(Features::two_factor_authentication())?
            || !user
                .has_enabled_two_factor_authentication(&fortify)
                .map_err(internal)?
        {
            return Err(HttpError::Unauthenticated);
        }
        let limiter = App::make::<openrust::cache::RateLimiter>().await?;
        let identity = serde_json::to_vec(&json!([pending.owner, pending.user]))?;
        let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &identity);
        let key = format!(
            "user-auth:factor:{}",
            hash.as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        if limiter.too_many_attempts(&key, 5).await? {
            return Err(
                openrust::auth::AuthError::Throttled(limiter.available_in(&key).await?).into(),
            );
        }
        limiter.hit(&key, 60).await?;
        let recovery = request
            .input("recovery_code")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|value| !value.is_empty() && value != "0")
            .map(Zeroizing::new);
        let code = request
            .input("code")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .map(Zeroizing::new);
        if let Some(recovery) = &recovery {
            let codes = Zeroizing::new(user.recovery_codes(&fortify).map_err(internal)?);
            if !codes
                .iter()
                .any(|stored| bool::from(stored.as_bytes().ct_eq(recovery.as_bytes())))
            {
                crate::actions::emit(
                    crate::events::TwoFactorAuthenticationFailed {
                        user: user.get_auth_identifier(),
                    },
                    &[&user],
                )
                .await?;
                return Err(failed(
                    "recovery_code",
                    "The provided two factor recovery code was invalid.",
                ));
            }
        } else {
            let Some(code) = code else {
                crate::actions::emit(
                    crate::events::TwoFactorAuthenticationFailed {
                        user: user.get_auth_identifier(),
                    },
                    &[&user],
                )
                .await?;
                return Err(failed(
                    "code",
                    "The provided two factor authentication code was invalid.",
                ));
            };
            let encrypted =
                attribute(&user, "two_factor_secret")?.ok_or(HttpError::Unauthenticated)?;
            let secret = Zeroizing::new(
                fortify
                    .current_encrypter()
                    .map_err(internal)?
                    .decrypt(&encrypted, true)
                    .map_err(internal)?,
            );
            let secret = std::str::from_utf8(&secret).map_err(internal)?;
            let provider =
                Resolver::<dyn crate::contracts::TwoFactorAuthenticationProvider>::get_facade_root(
                )
                .map_err(internal)?;
            if !provider.verify(secret, &code).await.map_err(internal)? {
                crate::actions::emit(
                    crate::events::TwoFactorAuthenticationFailed {
                        user: user.get_auth_identifier(),
                    },
                    &[&user],
                )
                .await?;
                return Err(failed(
                    "code",
                    "The provided two factor authentication code was invalid.",
                ));
            }
        }
        let deadline = store
            .consume(&raw, &pending)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        if let Some(recovery) = recovery
            && let Err(error) =
                crate::actions::consume_recovery_code(&mut user, &recovery, &fortify).await
        {
            if matches!(error, HttpError::Validation(_)) {
                crate::actions::emit(
                    crate::events::TwoFactorAuthenticationFailed {
                        user: user.get_auth_identifier(),
                    },
                    &[&user],
                )
                .await?;
            }
            return Err(error);
        }
        let device = pending
            .payload
            .get("device_name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| internal(std::io::Error::other("认证挑战缺少设备名称")))?;
        let chosen = abilities(&user, &fortify, pending.payload.get("abilities"))?;
        crate::actions::emit(
            crate::events::ValidTwoFactorAuthenticationCodeProvided {
                user: user.get_auth_identifier(),
            },
            &[&user],
        )
        .await?;
        let response = issue(
            &mut user,
            &fortify,
            device,
            &chosen,
            &pending.factor,
            Some(deadline),
        )
        .await?;
        limiter.clear(&key).await?;
        Ok(response)
    })
}

/// 由生成的应用Provider显式选择用户模型，保持独立Fortify应用的路由不变。
pub fn register_routes<M>(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    use openrust::support::ServiceProvider as _;
    super::StarterKitServiceProvider
        .commands(app, crate::console::prune_auth_challenges::register::<M>);
    super::StarterKitServiceProvider.load_routes_from(app, routes::<M>)
}
fn routes<M>() -> Result<(), openrust::routing::RoutingError>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    use openrust::{
        routing::{RouteGroup, RoutingError},
        support::facades::{Config, Route},
    };
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    let prefix = Config::string_with_default("api.prefix", "api").map_err(|source| {
        RoutingError::ContainerFailed {
            source: openrust::contracts::container::BindingResolutionError::FactoryFailed {
                service: "用户套件 API 前缀",
                source: Box::new(source),
            },
        }
    })?;
    Route::group(
        RouteGroup::new().prefix(prefix).middleware_name("api"),
        |router| {
            let login = router.post("/auth/token", login::<M>)?;
            login.name("user.tokens.login");
            if fortify.truthy("fortify.limiters.login") {
                login.middleware_name(format!(
                    "throttle:{}",
                    fortify.string("fortify.limiters.login", "")
                ));
            }
            router
                .post("/auth/token/challenge", challenge::<M>)?
                .name("user.tokens.challenge");
            router
                .post("/auth/confirm", super::confirmation::confirm::<M>)?
                .middleware_name("auth:sanctum")
                .name("user.confirmation");
            router
                .post("/auth/logout", super::account::logout::<M>)?
                .middleware_name("auth:sanctum")
                .name("user.tokens.logout");
            router
                .get("/user/tokens", super::account::tokens::<M>)?
                .middleware_name("auth:sanctum")
                .name("user.tokens.index");
            router
                .post("/user/tokens", super::account::create_token::<M>)?
                .middleware_name("auth:sanctum")
                .name("user.tokens.store");
            router
                .delete("/user/tokens/{token}", super::account::revoke_token::<M>)?
                .middleware_name("auth:sanctum")
                .name("user.tokens.destroy");
            Ok(())
        },
    )
}

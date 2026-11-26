//! 套件敏感操作的短期证明；绑定当前浏览器会话或实际PAT，不保存全局已确认状态。
use super::{
    challenges::{ChallengeStore, Pending},
    client, credentials,
};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{AuthIdentifier, AuthResult, Credentials},
    contracts::{session::Session as _, validation::Validator as _},
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::{AccessToken, HasApiTokens},
    support::{
        Timebox,
        facades::{App, Auth, Facade as _, Validator},
    },
    validation::{Data, RuleSet},
};
use serde_json::json;

#[derive(Clone, Copy, Debug)]
pub(super) enum Purpose {
    Tokens,
    Password,
    Email,
    Devices,
    Delete,
    Factors,
    Avatar,
}
impl Purpose {
    pub(super) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "tokens" => Self::Tokens,
            "password" => Self::Password,
            "email" => Self::Email,
            "devices" => Self::Devices,
            "delete" => Self::Delete,
            "factors" => Self::Factors,
            "avatar" => Self::Avatar,
            _ => return None,
        })
    }
    pub(super) fn key(self) -> String {
        format!(
            "confirm:{}",
            match self {
                Self::Tokens => "tokens",
                Self::Password => "password",
                Self::Email => "email",
                Self::Devices => "devices",
                Self::Delete => "delete",
                Self::Factors => "factors",
                Self::Avatar => "avatar",
            }
        )
    }
}
pub(super) fn binding<M: HasApiTokens>(
    request: &Request,
    user: &M,
    fortify: &Fortify,
) -> AuthResult<(Option<AuthIdentifier>, String)> {
    let (token, context) = match user.current_access_token(request) {
        Some(AccessToken::Personal(token)) => {
            (Some(token.id.clone()), serde_json::to_vec(&token.id)?)
        }
        Some(AccessToken::Transient) => {
            let session = request.session().map_err(|_| HttpError::Unauthenticated)?;
            (None, serde_json::to_vec(&("session", session.get_id()))?)
        }
        _ => return Err(HttpError::Unauthenticated.into()),
    };
    Ok((token, digest_client(fortify, &context)?))
}
pub(super) fn session_binding(request: &Request, fortify: &Fortify) -> AuthResult<String> {
    digest_client(
        fortify,
        &serde_json::to_vec(&(
            "session",
            request
                .session()
                .map_err(|_| HttpError::Unauthenticated)?
                .get_id(),
        ))?,
    )
}
fn digest_client(fortify: &Fortify, context: &[u8]) -> AuthResult<String> {
    let encrypter = fortify
        .current_encrypter()
        .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?;
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, encrypter.get_key());
    let mut signature = aws_lc_rs::hmac::Context::with_key(&key);
    signature.update(b"user-auth:client:v1\0");
    signature.update(context);
    Ok(signature
        .sign()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn lifetime(fortify: &Fortify) -> AuthResult<i64> {
    fortify
        .config
        .get("auth.password_timeout")
        .unwrap_or(json!(10800))
        .as_i64()
        .filter(|value| *value > 0)
        .ok_or(openrust::auth::AuthError::Configuration(
            "auth.password_timeout必须为正整数秒",
        ))
}

pub fn confirm<M>(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    Box::pin(async move {
        Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([
                (
                    "purpose",
                    "required|string|in:tokens,password,email,devices,delete,factors,avatar",
                ),
                ("password", "nullable|string"),
            ])?,
        )?
        .validate()
        .await?;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let found = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let user = (found.as_ref() as &dyn std::any::Any)
            .downcast_ref::<M>()
            .ok_or_else(|| internal(std::io::Error::other("用户套件与认证Provider模型不一致")))?
            .clone();
        let purpose = request
            .input("purpose")
            .await?
            .and_then(|value| value.as_str().and_then(Purpose::parse))
            .ok_or(HttpError::Unauthenticated)?;
        let password = request
            .input("password")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned));
        let original = client::security(&user, &fortify)?;
        let limiter = App::make::<openrust::cache::RateLimiter>().await?;
        let key = format!(
            "user:confirmation:{}",
            super::challenges::owner_key(user.get_morph_class()?, &user.get_auth_identifier())?
        );
        if limiter.too_many_attempts(&key, 5).await? {
            return Err(
                openrust::auth::AuthError::Throttled(limiter.available_in(&key).await?).into(),
            );
        }
        let provider = client::provider(&fortify)?;
        let confirmed = Timebox::default()
            .call(
                async |_| {
                    if let Some(callback) = fortify.confirm_password_callback() {
                        return callback(&user, password.as_deref()).await;
                    }
                    let Some(password) = password.as_deref().filter(|value| !value.is_empty())
                    else {
                        return Ok(false);
                    };
                    provider
                        .validate_credentials(
                            &user,
                            &Credentials::new(
                                [(
                                    fortify.username(),
                                    user.get_auth_attribute(&fortify.username())?
                                        .unwrap_or(openrust::database::Value::Null),
                                )],
                                password,
                            ),
                        )
                        .await
                },
                std::time::Duration::from_micros(200_000),
            )
            .await?;
        if !confirmed {
            limiter.hit(&key, 60).await?;
            return Err(client::failed("password", "auth.password"));
        }
        let fresh = provider
            .retrieve_by_id(&user.get_auth_identifier())
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let fresh = (fresh.as_ref() as &dyn std::any::Any)
            .downcast_ref::<M>()
            .ok_or(HttpError::Unauthenticated)?;
        if !client::same_binding(&original, &client::security(fresh, &fortify)?) {
            return Err(HttpError::Unauthenticated);
        }
        let response = complete(request, fresh, &fortify, purpose, None).await?;
        limiter.clear(&key).await?;
        Ok(response)
    })
}

/// 各主因素共用同一客户端／用途绑定的近期证明，不能由请求直接提交服务器状态。
pub(super) async fn complete<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    user: &M,
    fortify: &Fortify,
    purpose: Purpose,
    passkey: Option<super::passkey_confirmation::CredentialProof>,
) -> Result<Response, HttpError> {
    let (token, client_binding) = binding(request, user, fortify)?;
    if token.is_none() {
        request
            .session_mut()
            .map_err(internal)?
            .password_confirmed()
            .map_err(internal)?;
        if let Some(passkey) = passkey {
            super::web::remember_passkey(request, user, fortify, passkey)?;
        } else {
            super::web::remember(request, user, fortify)?;
        }
        return Ok(client::no_store(
            ApiResponse::empty()
                .with_status(StatusCode::CREATED)
                .into_response()?,
        ));
    }
    let challenge = ChallengeStore::new(user.get_connection()?)
        .create(
            &Pending {
                user: user.get_auth_identifier(),
                owner: user.get_morph_class()?.into(),
                purpose: purpose.key(),
                credential: credentials::stamp(user)?,
                factor: client::security(user, fortify)?,
                token,
                payload: json!({"client":client_binding,"passkey":passkey}),
            },
            lifetime(fortify)?,
        )
        .await?;
    Ok(client::no_store(ApiResponse::success(json!({"proof":challenge.key.as_str(),"expires_at":jiff::Timestamp::from_second(challenge.expires_at).map_err(internal)?})).with_status(StatusCode::CREATED).into_response()?))
}

#[derive(Clone)]
pub(super) struct Proof {
    session: Option<String>,
    deadline: i64,
    security: String,
    passkey: Option<super::passkey_confirmation::CredentialProof>,
}
impl Proof {
    pub(super) fn passkey(&self) -> Option<super::passkey_confirmation::CredentialProof> {
        self.passkey.clone()
    }
    pub(super) fn deadline(&self) -> i64 {
        self.deadline
    }

    pub async fn validate_in_transaction<
        M: TwoFactorAuthenticatable + openrust::database::eloquent::Model,
    >(
        &self,
        user: &M,
        fortify: &Fortify,
        transaction: &mut openrust::database::Transaction,
    ) -> AuthResult<()> {
        self.validate(user, fortify)?;
        if let Some(proof) = &self.passkey
            && !proof.current(user, fortify, Some(transaction)).await?
        {
            return Err(if self.session.is_some() {
                openrust::auth::AuthError::Backend(Box::new(
                    super::passkey_confirmation::ConfirmationRequired,
                ))
            } else {
                HttpError::Unauthenticated.into()
            });
        }

        if let Some(session) = &self.session
            && let Some(policy) = super::devices::policy()?
        {
            policy
                .check_in_transaction(user, transaction, session)
                .await?;
        }
        Ok(())
    }
    pub fn validate<M: TwoFactorAuthenticatable>(
        &self,
        user: &M,
        fortify: &Fortify,
    ) -> AuthResult<()> {
        let now = openrust::support::facades::Date::now(None)
            .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?
            .as_date_time()
            .timestamp()
            .as_second();
        if now >= self.deadline
            || !client::same_binding(&self.security, &client::security(user, fortify)?)
        {
            return Err(HttpError::Unauthenticated.into());
        }
        Ok(())
    }
}
pub(super) async fn claim<M>(
    request: &mut Request,
    user: &M,
    purpose: Purpose,
    fortify: &Fortify,
) -> Result<Proof, HttpError>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    if request.query(Some("proof"))?.is_some() {
        let response =
            ApiResponse::failure(openrust::http::StatusCode::BAD_REQUEST, "bad_request", "")
                .with_message_key("user.proof_in_url", Default::default())
                .into_response()?;
        return Err(openrust::http::abort_response(client::no_store(response)));
    }
    if matches!(
        user.current_access_token(request),
        Some(AccessToken::Transient)
    ) {
        return claim_session(request, user, fortify).await;
    }
    let raw = match request.header("x-auth-confirmation")? {
        Some(value) => value.to_owned(),
        None => request
            .input("proof")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(HttpError::Unauthenticated)?,
    };
    let (token, client_binding) = binding(request, user, fortify)?;
    let store = ChallengeStore::new(user.get_connection()?);
    let pending = store
        .resolve(&raw, &purpose.key())
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    if pending.owner != user.get_morph_class()?
        || serde_json::to_value(&pending.user)? != serde_json::to_value(user.get_auth_identifier())?
        || serde_json::to_value(&pending.token)? != serde_json::to_value(token)?
        || pending
            .payload
            .get("client")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|value| !client::same_binding(value, &client_binding))
        || !client::same_binding(&pending.credential, &credentials::stamp(user)?)
        || !client::same_binding(&pending.factor, &client::security(user, fortify)?)
    {
        return Err(HttpError::Unauthenticated);
    }
    let deadline = store
        .consume(&raw, &pending)
        .await?
        .ok_or(HttpError::Unauthenticated)?;
    Ok(Proof {
        session: None,
        deadline,
        security: pending.factor,
        passkey: pending
            .payload
            .get("passkey")
            .filter(|value| !value.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(internal)?,
    })
}

pub(super) async fn claim_session<M: TwoFactorAuthenticatable>(
    request: &mut Request,
    user: &M,
    fortify: &Fortify,
) -> Result<Proof, HttpError> {
    let confirmed = request
        .session()
        .map_err(internal)?
        .get("auth.password_confirmed_at")
        .and_then(serde_json::Value::as_i64);
    let now = openrust::support::facades::Date::now(None)
        .map_err(internal)?
        .as_date_time()
        .timestamp()
        .as_second();
    let timeout = lifetime(fortify)?;
    let deadline = confirmed.and_then(|at| at.checked_add(timeout)?.checked_add(1));
    let passkey = request
        .session()
        .map_err(internal)?
        .get(super::web::CONFIRMATION)
        .and_then(|value| value.get("passkey"))
        .cloned()
        .map(serde_json::from_value::<super::passkey_confirmation::CredentialProof>)
        .transpose()
        .map_err(internal)?;
    let current = match &passkey {
        Some(proof) => proof.current(user, fortify, None).await?,
        None => true,
    };
    if !current
        || confirmed.is_some_and(|at| at > now)
        || deadline.is_none_or(|end| now >= end)
        || !super::web::confirmed(request, user, fortify)?
    {
        super::web::forget(request)?;
        if !request.expects_json()
            && let Some(response) =
                openrust::auth::middleware::RequirePassword::default().challenge(request)?
        {
            return Err(openrust::http::abort_response(client::no_store(response)));
        }
        let response =
            ApiResponse::failure(StatusCode::LOCKED, "password_confirmation_required", "")
                .with_message_key("user.confirmation_required", Default::default())
                .into_response()?;
        return Err(openrust::http::abort_response(client::no_store(response)));
    }
    Ok(Proof {
        session: Some(request.session().map_err(internal)?.get_id().to_owned()),
        deadline: deadline.expect("已核对期限"),
        security: client::security(user, fortify)?,
        passkey,
    })
}

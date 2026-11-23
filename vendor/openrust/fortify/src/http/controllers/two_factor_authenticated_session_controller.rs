use crate::{
    Fortify, FortifyError, contracts,
    http::requests::TwoFactorLoginRequest,
    resolver::{Resolver, internal},
    two_factor_model::{Account, AccountEvent, Mutation, text},
};
use openrust::{
    auth::Auth,
    contracts::{auth::StatefulGuard as _, session::Session as _},
    database::Value,
    foundation::http::ValidatedFormRequest,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{Facade as _, Redirect},
};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

pub struct TwoFactorAuthenticatedSessionController;
async fn challenged(
    request: &mut Request,
    fortify: &Fortify,
) -> Result<Option<Box<dyn Account>>, HttpError> {
    let provider = Auth::session_guard(request, fortify.guard().as_deref())?.get_provider();
    let id = provider
        .get_model()
        .ok_or_else(|| internal(FortifyError::UnregisteredTwoFactorModel))?;
    let key = request
        .session()
        .map_err(internal)?
        .get("login.id")
        .cloned();
    let key = match key {
        Some(serde_json::Value::String(v)) => Value::Text(v),
        Some(serde_json::Value::Number(v)) if v.as_i64().is_some() => {
            Value::Integer(v.as_i64().expect("checked integer"))
        }
        Some(serde_json::Value::Number(v)) if v.as_u64().is_some() => {
            Value::Unsigned(v.as_u64().expect("checked integer"))
        }
        _ => return Ok(None),
    };
    fortify
        .registered_model(id)
        .map_err(internal)?
        .find(key)
        .await
}
impl TwoFactorAuthenticatedSessionController {
    pub fn create<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            if challenged(request, &fortify).await?.is_none() {
                return Ok(Redirect::route("login", &[], Default::default())?.into());
            }
            super::respond::<dyn contracts::TwoFactorChallengeViewResponse>(request).await
        })
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
        _: ValidatedFormRequest<TwoFactorLoginRequest>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let Some(mut user) = challenged(request, &fortify).await? else {
                return super::respond::<dyn contracts::FailedTwoFactorLoginResponse>(request)
                    .await;
            };
            let recovery = input(request, "recovery_code").await?;
            let recovery = if let Some(recovery) = recovery.filter(|v| !v.is_empty() && v != "0") {
                let codes = Zeroizing::new(user.recovery_codes(&fortify).map_err(internal)?);
                codes
                    .iter()
                    .find(|code| bool::from(code.as_bytes().ct_eq(recovery.as_bytes())))
                    .cloned()
            } else {
                None
            };
            if let Some(recovery) = recovery.filter(|v| !v.is_empty() && v != "0") {
                let recovery = Zeroizing::new(recovery);
                request
                    .session_mut()
                    .map_err(internal)?
                    .forget(&["login.id"]);
                user.mutate(Mutation::Replace(&recovery)).await?;
            } else {
                let code = input(request, "code").await?;
                let valid = if let Some(code) = code.filter(|v| !v.is_empty() && v != "0") {
                    let encrypted = text(user.attribute("two_factor_secret").map_err(internal)?)
                        .map_err(internal)?
                        .ok_or_else(|| {
                            internal(FortifyError::InvalidAttribute("two_factor_secret"))
                        })?;
                    let clear = Zeroizing::new(
                        fortify
                            .current_encrypter()
                            .map_err(internal)?
                            .decrypt(&encrypted, true)
                            .map_err(internal)?,
                    );
                    let secret = std::str::from_utf8(&clear).map_err(internal)?;
                    Resolver::<dyn contracts::TwoFactorAuthenticationProvider>::get_facade_root()
                        .map_err(internal)?
                        .verify(secret, &code)
                        .await
                        .map_err(internal)?
                } else {
                    false
                };
                if !valid {
                    user.emit(AccountEvent::Failed).await?;
                    return super::respond::<dyn contracts::FailedTwoFactorLoginResponse>(request)
                        .await;
                }
                request
                    .session_mut()
                    .map_err(internal)?
                    .forget(&["login.id"]);
            }
            user.emit(AccountEvent::Valid).await?;
            let remember = request
                .session_mut()
                .map_err(internal)?
                .pull("login.remember")
                .is_some_and(|v| match v {
                    serde_json::Value::Null => false,
                    serde_json::Value::Bool(v) => v,
                    serde_json::Value::String(v) => !v.is_empty() && v != "0",
                    serde_json::Value::Number(v) => v.as_f64() != Some(0.0),
                    serde_json::Value::Array(v) => !v.is_empty(),
                    serde_json::Value::Object(v) => !v.is_empty(),
                });
            Auth::session_guard(request, fortify.guard().as_deref())?
                .login(user.into_user(), remember)
                .await?;
            request
                .session_mut()
                .map_err(internal)?
                .regenerate(false)
                .await
                .map_err(internal)?;
            super::respond::<dyn contracts::TwoFactorLoginResponse>(request).await
        })
    }
}
pub(crate) async fn input(request: &mut Request, name: &str) -> Result<Option<String>, HttpError> {
    Ok(request
        .input(Some(name))
        .await?
        .and_then(|v| v.as_str().map(str::to_owned)))
}

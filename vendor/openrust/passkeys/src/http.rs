use crate::{
    AuthenticationOptions, PasskeyRecord, Passkeys, RegistrationOptions, actions::*, contracts,
    internal, invalid,
};
use openrust::contracts::{
    cache::Repository as _, container::Container as _, validation::Validator as _,
};
use openrust::{
    auth::Auth,
    contracts::{
        auth::{Guard as _, StatefulGuard as _},
        session::Session as _,
        validation::Factory as _,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{
        facades::{Facade, Redirect, Validator},
        now,
    },
    validation::{Data, Rule, RuleSet},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct Resolver<T: ?Sized>(std::marker::PhantomData<fn() -> T>);
impl<T: ?Sized + Send + Sync + 'static> Facade for Resolver<T> {
    type Service = T;
    fn get_facade_accessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<T>()
    }
}
#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    state: T,
    expires: i64,
    nonce: String,
}
fn timestamp() -> Result<i64, HttpError> {
    Ok(now(None)
        .map_err(internal)?
        .as_date_time()
        .timestamp()
        .as_nanosecond()
        .div_euclid(1_000_000) as i64)
}
fn save<T: Serialize>(
    request: &mut Request,
    key: &str,
    state: &T,
    options: &Value,
    service: &Passkeys,
) -> Result<(), HttpError> {
    let envelope = Envelope {
        state,
        expires: timestamp()?
            .checked_add(i64::try_from(service.timeout().map_err(internal)?).map_err(internal)?)
            .ok_or(HttpError::InvalidRequest)?,
        nonce: options["challenge"]
            .as_str()
            .ok_or(HttpError::InvalidRequest)?
            .into(),
    };
    request
        .session_mut()?
        .put(key, serde_json::to_value(envelope).map_err(internal)?)?;
    Ok(())
}
async fn consume<T: serde::de::DeserializeOwned>(
    request: &mut Request,
    key: &str,
) -> Result<T, HttpError> {
    let value = request
        .session_mut()?
        .pull(key)
        .ok_or_else(|| expired(key))?;
    let envelope: Envelope<T> = serde_json::from_value(value).map_err(internal)?;
    let remaining = envelope.expires.saturating_sub(timestamp()?);
    if remaining <= 0 {
        return Err(expired(key));
    }
    // Session pull alone cannot exclude concurrent requests that loaded the same
    // session. An atomic cache add makes the issued challenge single-use across
    // those requests and, with a shared cache, across application processes.
    let cache = Resolver::<openrust::cache::CacheManager>::get_facade_root()
        .map_err(internal)?
        .store(None)
        .map_err(internal)?;
    let ttl = remaining.saturating_add(999) / 1000;
    if !cache
        .add(
            &format!("passkey.consumed.{}", envelope.nonce),
            &openrust::cache::Value::Json(true.into()),
            Some(openrust::cache::Ttl::Seconds(ttl)),
        )
        .await
        .map_err(internal)?
    {
        return Err(expired(key));
    }
    Ok(envelope.state)
}
fn expired(key: &str) -> HttpError {
    invalid(if key == "passkey.registration_options" {
        "Passkey registration session expired. Please try again."
    } else {
        "Passkey verification session expired. Please try again."
    })
}
async fn credential(request: &mut Request, registration: bool) -> Result<Value, HttpError> {
    let mut rules = RuleSet::new()
        .field("credential", [Rule::required(), Rule::array()])?
        .field("credential.id", [Rule::required(), Rule::string()])?
        .field("credential.rawId", [Rule::required(), Rule::string()])?
        .field("credential.type", [Rule::required(), Rule::string()])?
        .field("credential.response", [Rule::required(), Rule::array()])?;
    if registration {
        rules = rules.field("name", [Rule::required(), Rule::string(), Rule::max(255)])?;
    } else {
        rules = rules.field("remember", [Rule::boolean()])?;
    }
    Validator::get_facade_root()
        .map_err(internal)?
        .make(
            Data::from_request(request).await?,
            rules,
            Default::default(),
            Default::default(),
        )?
        .validate()
        .await?;
    let value = request
        .input(Some("credential"))
        .await?
        .ok_or_else(|| invalid("Invalid credential format."))?;
    crate::webauthn::validate_credential(&value, registration)
        .map_err(|_| invalid("Invalid credential format."))?;
    Ok(value)
}
async fn user(
    request: &mut Request,
    service: &Passkeys,
) -> Result<Box<dyn openrust::contracts::auth::Authenticatable>, HttpError> {
    let mut guard = Auth::session_guard(request, Some(&service.string("passkeys.guard", "web")))?;
    let user = guard.user().await?.ok_or(HttpError::Unauthenticated)?;
    service.model()?.clone_user(user).ok_or_else(|| {
        internal(crate::PasskeyError::Configuration(
            "User model must implement the registered PasskeyUser contract",
        ))
    })
}
async fn respond<T: contracts::Response + ?Sized + 'static>(
    request: &mut Request,
    passkey: Option<&PasskeyRecord>,
) -> Result<Response, HttpError> {
    Resolver::<T>::get_facade_root()
        .map_err(internal)?
        .to_response(request, passkey)
        .await
}

pub struct PasskeyRegistrationController;
impl PasskeyRegistrationController {
    pub fn index<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let user = user(request, &service).await?;
            let state = GenerateRegistrationOptions(service.clone())
                .call(user.as_ref())
                .await?;
            save(
                request,
                "passkey.registration_options",
                &state,
                state.options(),
                &service,
            )?;
            Response::json(StatusCode::OK, json!({"options":state.options()}))
        })
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let value = credential(request, true).await?;
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let user = user(request, &service).await?;
            let state =
                consume::<RegistrationOptions>(request, "passkey.registration_options").await?;
            let name = request
                .input(Some("name"))
                .await?
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or(HttpError::InvalidRequest)?;
            let passkey = StorePasskey(service)
                .call(user.as_ref(), &name, value, &state)
                .await?;
            respond::<dyn contracts::PasskeyRegistrationResponse>(request, Some(&passkey)).await
        })
    }
    pub fn destroy<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let user = user(request, &service).await?;
            let key = request
                .route("passkey")
                .ok_or(HttpError::NotFound)?
                .to_owned();
            let passkey = service
                .passkey_store()
                .find(key.into())
                .await
                .map_err(internal)?
                .ok_or(HttpError::NotFound)?;
            if passkey
                .get_attribute_value("user_id")
                .await
                .map_err(internal)?
                .as_ref()
                .and_then(crate::contracts::scalar)
                != crate::contracts::scalar(&service.account(user.as_ref())?.key)
            {
                return Err(HttpError::AuthorizationDenied(
                    "This action is unauthorized.".into(),
                ));
            }
            DeletePasskey.call(user.as_ref(), &passkey).await?;
            respond::<dyn contracts::PasskeyDeletedResponse>(request, None).await
        })
    }
}
pub struct PasskeyLoginController;
impl PasskeyLoginController {
    pub fn index<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let state = GenerateVerificationOptions(service.clone())
                .call(None)
                .await?;
            save(
                request,
                "passkey.verification_options",
                &state,
                state.options(),
                &service,
            )?;
            Response::json(StatusCode::OK, json!({"options":state.options()}))
        })
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let value = credential(request, false).await?;
            let state =
                consume::<AuthenticationOptions>(request, "passkey.verification_options").await?;
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let passkey = VerifyPasskey(service.clone())
                .call(value, &state, None)
                .await?;
            let owner = service
                .model()?
                .find(
                    passkey
                        .get_attribute_value("user_id")
                        .await
                        .map_err(internal)?
                        .ok_or(HttpError::NotFound)?,
                )
                .await?
                .ok_or_else(|| invalid("Unable to sign in with this account."))?;
            if !service
                .allows_login(request, owner.as_ref(), &passkey)
                .await?
            {
                return Err(invalid("Unable to sign in with this account."));
            }
            let remember = request.boolean("remember").await?;
            Auth::session_guard(request, Some(&service.string("passkeys.guard", "web")))?
                .login(owner, remember)
                .await?;
            request.session_mut()?.regenerate(false).await?;
            respond::<dyn contracts::PasskeyLoginResponse>(request, None).await
        })
    }
}
pub struct PasskeyConfirmationController;
impl PasskeyConfirmationController {
    pub fn index<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let user = user(request, &service).await?;
            let state = GenerateVerificationOptions(service.clone())
                .call(Some(user.as_ref()))
                .await?;
            save(
                request,
                "passkey.verification_options",
                &state,
                state.options(),
                &service,
            )?;
            Response::json(StatusCode::OK, json!({"options":state.options()}))
        })
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let value = credential(request, false).await?;
            let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let user = user(request, &service).await?;
            let state =
                consume::<AuthenticationOptions>(request, "passkey.verification_options").await?;
            VerifyPasskey(service)
                .call(value, &state, Some(user.as_ref()))
                .await?;
            request.session_mut()?.password_confirmed()?;
            respond::<dyn contracts::PasskeyConfirmationResponse>(request, None).await
        })
    }
}

pub struct PasskeyLoginResponse;
impl contracts::PasskeyLoginResponse for PasskeyLoginResponse {}
pub struct PasskeyConfirmationResponse;
impl contracts::PasskeyConfirmationResponse for PasskeyConfirmationResponse {}
pub struct PasskeyRegistrationResponse;
impl contracts::PasskeyRegistrationResponse for PasskeyRegistrationResponse {}
pub struct PasskeyDeletedResponse;
impl contracts::PasskeyDeletedResponse for PasskeyDeletedResponse {}
macro_rules! redirect_response {
    ($name:ident,$fallback:expr)=>{impl contracts::Response for $name {
        fn to_response<'a>(&'a self,request:&'a mut Request,_:Option<&'a PasskeyRecord>)->HttpFuture<'a,Result<Response,HttpError>> {Box::pin(async move {
            let service=Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
            let response:Response=Redirect::intended(request,&service.string($fallback.0,$fallback.1),Default::default())?.into();
            if request.wants_json() {let target=response.headers().get("location").and_then(|v|v.to_str().ok()).unwrap_or("/");return Response::json(StatusCode::OK,json!({"redirect":target}));}
            Ok(response)
        })}
    }};
}
redirect_response!(PasskeyLoginResponse, ("passkeys.redirect", "/"));
redirect_response!(
    PasskeyConfirmationResponse,
    ("passkeys.confirm_redirect", "/")
);
macro_rules! status_response {
    ($name:ident,$status:expr)=>{impl contracts::Response for $name {
        fn to_response<'a>(&'a self,request:&'a mut Request,passkey:Option<&'a PasskeyRecord>)->HttpFuture<'a,Result<Response,HttpError>> {Box::pin(async move {
            if request.wants_json() {let mut data=json!({"status":$status});if let Some(passkey)=passkey {data["id"]=passkey.get_key().await.as_ref().and_then(crate::contracts::scalar).unwrap_or_default().into();data["name"]=passkey.get_attribute_value("name").await.map_err(internal)?.as_ref().and_then(crate::contracts::scalar).unwrap_or_default().into();}return Response::json(StatusCode::OK,data);}
            Ok(Redirect::back(request,None,Default::default())?.with(request,"status",$status.into())?.into())
        })}
    }};
}
status_response!(PasskeyRegistrationResponse, "passkey-registered");
status_response!(PasskeyDeletedResponse, "passkey-deleted");

pub(crate) fn register(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult {
    use openrust::contracts::foundation::Application as _;
    let container = app.container();
    container.singleton_if::<Passkeys, _>(|c| Ok(Arc::new(Passkeys::new(c.get("config")?))))?;
    container
        .singleton_if::<PasskeyLoginController, _>(|_| Ok(Arc::new(PasskeyLoginController)))?;
    container.singleton_if::<PasskeyRegistrationController, _>(|_| {
        Ok(Arc::new(PasskeyRegistrationController))
    })?;
    container.singleton_if::<PasskeyConfirmationController, _>(|_| {
        Ok(Arc::new(PasskeyConfirmationController))
    })?;
    container.singleton_if::<dyn contracts::PasskeyLoginResponse, _>(|_| {
        Ok(Arc::new(PasskeyLoginResponse))
    })?;
    container.singleton_if::<dyn contracts::PasskeyRegistrationResponse, _>(|_| {
        Ok(Arc::new(PasskeyRegistrationResponse))
    })?;
    container.singleton_if::<dyn contracts::PasskeyConfirmationResponse, _>(|_| {
        Ok(Arc::new(PasskeyConfirmationResponse))
    })?;
    container.singleton_if::<dyn contracts::PasskeyDeletedResponse, _>(|_| {
        Ok(Arc::new(PasskeyDeletedResponse))
    })?;
    Ok(())
}

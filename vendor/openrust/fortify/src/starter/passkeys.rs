//! 将原生通行密钥动作接入套件共同用户锁及近期身份确认。
use super::{confirmation, credentials, passkey_confirmation::CredentialProof, web};
use crate::passkeys::{self, AuthenticationOptions, PasskeyRecord, Passkeys, RegistrationOptions};
use crate::{
    Features, Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::AuthError,
    contracts::{
        auth::{Authenticatable, Guard as _},
        container::Container as _,
        foundation::Application as _,
    },
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response},
    sanctum::HasApiTokens,
    support::{
        ProviderResult,
        facades::{Auth, Facade as _},
    },
};
use std::{marker::PhantomData, sync::Arc};

#[derive(Clone)]
struct VerifiedAccount(String);
struct Actions<M>(Arc<Passkeys>, PhantomData<fn() -> M>);
fn typed<M: Authenticatable + Clone>(user: &dyn Authenticatable) -> Result<M, HttpError> {
    (user as &dyn std::any::Any)
        .downcast_ref::<M>()
        .cloned()
        .ok_or(HttpError::Unauthenticated)
}
fn auth_error(error: HttpError) -> AuthError {
    error.into()
}
impl<M: HasApiTokens + TwoFactorAuthenticatable + Clone> passkeys::contracts::PasskeyActions
    for Actions<M>
{
    fn service(&self) -> Arc<Passkeys> {
        self.0.clone()
    }
    fn registration_options<'a>(
        &'a self,
        request: &'a mut Request,
        user: &'a dyn Authenticatable,
    ) -> HttpFuture<'a, Result<RegistrationOptions, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let mut user = typed::<M>(user)?;
            let proof = confirmation::claim_session(request, &user, &fortify).await?;
            let service = self.service();
            credentials::with_locked(&mut user, move |user, transaction| {
                Box::pin(async move {
                    proof
                        .validate_in_transaction(user, &fortify, transaction)
                        .await?;
                    let options = passkeys::actions::GenerateRegistrationOptions(service)
                        .call_in_transaction(user, transaction)
                        .await?;
                    proof
                        .validate_in_transaction(user, &fortify, transaction)
                        .await?;
                    Ok(options)
                })
            })
            .await
            .map_err(Into::into)
        })
    }
    fn store<'a>(
        &'a self,
        request: &'a mut Request,
        user: &'a dyn Authenticatable,
        name: &'a str,
        value: serde_json::Value,
        options: &'a RegistrationOptions,
    ) -> HttpFuture<'a, Result<PasskeyRecord, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let mut user = typed::<M>(user)?;
            let proof = confirmation::claim_session(request, &user, &fortify).await?;
            let service = self.service();
            let name = name.to_owned();
            let options = options.clone();
            credentials::with_locked(&mut user, move |user, transaction| {
                Box::pin(async move {
                    proof
                        .validate_in_transaction(user, &fortify, transaction)
                        .await?;
                    let record = passkeys::actions::StorePasskey(service)
                        .call_in_transaction(user, transaction, &name, value, &options)
                        .await?;
                    user.refresh_for_update(transaction).await?;
                    proof
                        .validate_in_transaction(user, &fortify, transaction)
                        .await?;
                    Ok(record)
                })
            })
            .await
            .map_err(Into::into)
        })
    }
    fn delete<'a>(
        &'a self,
        request: &'a mut Request,
        user: &'a dyn Authenticatable,
        record: &'a PasskeyRecord,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let mut user = typed::<M>(user)?;
            let proof = confirmation::claim_session(request, &user, &fortify).await?;
            let record = record.clone();
            credentials::with_locked(&mut user, move |user, transaction| {
                Box::pin(async move {
                    proof
                        .validate_in_transaction(user, &fortify, transaction)
                        .await?;
                    passkeys::actions::DeletePasskey
                        .call_in_transaction(user, transaction, &record)
                        .await?;
                    user.refresh_for_update(transaction).await?;
                    // 允许撤销本次确认使用的凭据；后续请求必须重新确认。
                    proof.validate(user, &fortify)?;
                    Ok(())
                })
            })
            .await
            .map_err(Into::into)
        })
    }
    fn verify<'a>(
        &'a self,
        request: &'a mut Request,
        value: serde_json::Value,
        options: &'a AuthenticationOptions,
        user: Option<&'a dyn Authenticatable>,
    ) -> HttpFuture<'a, Result<PasskeyRecord, HttpError>> {
        Box::pin(async move {
            let service = self.service();
            let discoverable = user.is_none();
            let mut user = match user {
                Some(user) => typed::<M>(user)?,
                None => {
                    let record = passkeys::actions::VerifyPasskey(service.clone())
                        .get_passkey(&value)
                        .await?;
                    let owner = service
                        .user_for(&record)
                        .await?
                        .ok_or(HttpError::Unauthenticated)?;
                    typed::<M>(owner.as_ref())?
                }
            };
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let binding = super::client::security(&user, &fortify)?;
            let options = options.clone();
            let record = credentials::with_locked(&mut user, move |user, transaction| {
                Box::pin(async move {
                    let action = passkeys::actions::VerifyPasskey(service);
                    if discoverable {
                        action
                            .call_discoverable_in_transaction(user, transaction, value, &options)
                            .await
                            .map_err(auth_error)
                    } else {
                        action
                            .call_in_transaction(user, transaction, value, &options)
                            .await
                            .map_err(auth_error)
                    }
                })
            })
            .await?;
            request
                .parts_mut()
                .extensions
                .insert(CredentialProof::from_record(&self.service(), &record).await?);
            request
                .parts_mut()
                .extensions
                .insert(VerifiedAccount(binding));
            Ok(record)
        })
    }
    fn before_login<'a>(
        &'a self,
        request: &'a mut Request,
        user: &'a dyn Authenticatable,
        _record: &'a PasskeyRecord,
    ) -> HttpFuture<'a, Result<Option<Response>, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let typed = typed::<M>(user)?;
            let binding = request
                .parts()
                .extensions
                .get::<VerifiedAccount>()
                .ok_or(HttpError::Unauthenticated)?;
            if !super::client::same_binding(&binding.0, &super::client::security(&typed, &fortify)?)
            {
                return Err(HttpError::Unauthenticated);
            }
            let limiter =
                Resolver::<crate::LoginRateLimiter>::get_facade_root().map_err(internal)?;
            crate::actions::RedirectIfTwoFactorAuthenticatable::new(fortify, limiter)
                .challenge(request, user)
                .await
        })
    }
    fn confirmed<'a>(
        &'a self,
        request: &'a mut Request,
        user: &'a dyn Authenticatable,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let mut user = typed::<M>(user)?;
            user.refresh().await.map_err(internal)?;
            let binding = request
                .parts()
                .extensions
                .get::<VerifiedAccount>()
                .ok_or(HttpError::Unauthenticated)?;
            if !super::client::same_binding(&binding.0, &super::client::security(&user, &fortify)?)
            {
                return Err(HttpError::Unauthenticated);
            }
            let proof = request
                .parts()
                .extensions
                .get::<CredentialProof>()
                .cloned()
                .ok_or(HttpError::Unauthenticated)?;
            if !proof.current(&user, &fortify, None).await? {
                return Err(HttpError::Unauthenticated);
            }
            Auth::session_guard(request, fortify.guard().as_deref())?
                .set_user(Box::new(user.clone()));
            web::remember_passkey(request, &user, &fortify, proof)
        })
    }
}
pub(super) fn register<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    app: &Application,
) -> ProviderResult {
    let fortify = Resolver::<Fortify>::get_facade_root()?;
    if !Features::new(fortify.config.clone()).enabled(Features::passkeys())? {
        return Ok(());
    }
    app.container()
        .bind::<dyn passkeys::contracts::PasskeyActions, _>(|container| {
            Ok(
                Arc::new(Actions::<M>(container.make::<Passkeys>()?, PhantomData))
                    as Arc<dyn passkeys::contracts::PasskeyActions>,
            )
        })?;
    use openrust::support::ServiceProvider as _;
    super::StarterKitServiceProvider.load_routes_from(app, routes::<M>)?;
    Ok(())
}
fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>()
-> Result<(), openrust::routing::RoutingError> {
    use openrust::{routing::RouteGroup, support::facades::Route};
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
    Route::group(RouteGroup::new(), |router| {
        router
            .get("/vendor/user-starter/passkeys.js", script)?
            .name("user.passkeys.script");
        Ok(())
    })?;
    if fortify.truthy("fortify.views") {
        Route::group(
            RouteGroup::new()
                .middleware_name("web")
                .middleware_name("auth.session")
                .middleware_name(format!(
                    "auth:{}",
                    fortify.guard().as_deref().unwrap_or("web")
                )),
            |router| {
                router
                    .get("/user/passkeys", page::<M>)?
                    .name("user.passkeys.page");
                Ok(())
            },
        )?;
    }
    Ok(())
}
fn script(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let app = openrust::support::facades::App::get_facade_application()?
            .ok_or(HttpError::InvalidRequest)?;
        let file = app.public_path().join("vendor/user-starter/passkeys.js");
        let response = if file.is_file() {
            Response::file(file, Default::default()).await?
        } else {
            Response::new(
                openrust::http::StatusCode::OK,
                openrust::http::Body::from(include_str!("../../stubs/starter/passkeys.js")),
            )
        };
        let _ = request;
        Ok(response.header(
            "content-type".parse().expect("静态头名称"),
            "text/javascript; charset=utf-8".parse().expect("静态头值"),
        ))
    })
}
fn page<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::support::facades::View;
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let user = web::user::<M>(request, &fortify).await?;
        confirmation::claim_session(request, &user, &fortify).await?;
        let service = Resolver::<Passkeys>::get_facade_root().map_err(internal)?;
        let mut records = Vec::new();
        for record in service.for_user(&user).await? {
            let id = record.get_key().await.ok_or(HttpError::NotFound)?;
            let id = openrust::auth::AuthIdentifier::try_from(&id)?;
            let name = match record.get_attribute_value("name").await.map_err(internal)? {
                Some(openrust::database::Value::Text(name)) => name,
                _ => return Err(HttpError::InvalidRequest),
            };
            records.push(serde_json::json!({"id":id,"name":name}));
        }
        let view = View::make(
            "user.passkeys",
            [("passkeys".into(), serde_json::json!(records).into())].into(),
        )?;
        Ok(super::client::no_store(
            Response::view(
                openrust::http::StatusCode::OK,
                view,
                Default::default(),
                request,
            )
            .await?,
        ))
    })
}

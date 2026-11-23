use crate::{
    Fortify, contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{Auth, AuthError, passwords::Password},
    contracts::auth::Guard as _,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Facade as _,
};

pub(crate) async fn canonicalize(
    request: &mut Request,
    fortify: &Fortify,
) -> Result<(), HttpError> {
    if fortify.truthy("fortify.lowercase_usernames") && request.has(&[&fortify.username()]).await? {
        crate::actions::CanonicalizeUsername::new(
            Resolver::<Fortify>::get_facade_root().map_err(internal)?,
        )
        .handle(request)
        .await?;
    }
    Ok(())
}
pub struct ProfileInformationController;
impl ProfileInformationController {
    pub fn update<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            canonicalize(request, &fortify).await?;
            let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
            let input = request.all().await?;
            let updater = fortify
                .profile_updater
                .read()
                .expect("Fortify actions lock")
                .clone();
            let updater = match updater {
                Some(updater) => updater,
                None => Resolver::<dyn contracts::UpdatesUserProfileInformation>::get_facade_root()
                    .map_err(internal)?,
            };
            let user = updater.update(user.as_ref(), &input).await?;
            Auth::sessionGuard(request, fortify.guard().as_deref())?.set_user(user);
            super::respond::<dyn contracts::ProfileInformationUpdatedResponse>(request).await
        })
    }
}
pub struct PasswordController;
impl PasswordController {
    pub fn update<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
            let input = request.all().await?;
            let updater = fortify
                .password_updater
                .read()
                .expect("Fortify actions lock")
                .clone();
            let updater = match updater {
                Some(updater) => updater,
                None => Resolver::<dyn contracts::UpdatesUserPasswords>::get_facade_root()
                    .map_err(internal)?,
            };
            let user = updater
                .updateFromRequest(request, user.as_ref(), &input)
                .await?;
            Auth::sessionGuard(request, fortify.guard().as_deref())?.set_user(user);
            let user = Auth::user(request).await?.ok_or(HttpError::NotFound)?;
            let name = fortify
                .config
                .get("fortify.passwords")
                .and_then(|v| v.as_str().map(str::to_owned));
            Password::broker(name.as_deref())?
                .deleteToken(user.as_password_reset().ok_or(AuthError::Configuration(
                    "user must implement CanResetPassword",
                ))?)
                .await?;
            crate::actions::emit(crate::events::PasswordUpdatedViaController { user }, &[]).await?;
            super::respond::<dyn contracts::PasswordUpdateResponse>(request).await
        })
    }
}

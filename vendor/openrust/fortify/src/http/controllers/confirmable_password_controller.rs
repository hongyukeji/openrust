use crate::{
    Fortify,
    actions::ConfirmPassword,
    contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::Auth,
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{Facade as _, Validator},
    validation::{Data, RuleSet},
};

pub struct ConfirmablePasswordController;
impl ConfirmablePasswordController {
    pub fn show<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(super::respond::<dyn contracts::ConfirmPasswordViewResponse>(request))
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let data = Data::from_request(request).await?;
            Validator::make(data, RuleSet::parse([("password", "nullable|string")])?)?
                .validate()
                .await?;
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let password = request
                .input(Some("password"))
                .await?
                .and_then(|v| v.as_str().map(str::to_owned));
            let user = Auth::user(request)
                .await?
                .ok_or(HttpError::Unauthenticated)?;
            let guard = fortify.guard();
            let confirmed = ConfirmPassword::new(fortify)
                .call(
                    &mut Auth::session_guard(request, guard.as_deref())?,
                    user.as_ref(),
                    password.as_deref(),
                )
                .await?;
            if confirmed {
                request
                    .session_mut()
                    .map_err(internal)?
                    .password_confirmed()
                    .map_err(internal)?;
                super::respond::<dyn contracts::PasswordConfirmedResponse>(request).await
            } else {
                super::respond::<dyn contracts::FailedPasswordConfirmationResponse>(request).await
            }
        })
    }
}

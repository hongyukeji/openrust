use crate::{
    Fortify, contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{Auth, events::Registered},
    contracts::{auth::StatefulGuard as _, session::Session as _},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Facade as _,
};
use std::sync::Arc;
pub struct RegisteredUserController;
impl RegisteredUserController {
    pub fn create<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(super::respond::<dyn contracts::RegisterViewResponse>(
            request,
        ))
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            super::user_updates::canonicalize(request, &fortify).await?;
            let input = request.all().await?;
            let creator = fortify
                .creator
                .read()
                .expect("Fortify actions lock")
                .clone();
            let creator = match creator {
                Some(creator) => creator,
                None => Resolver::<dyn contracts::CreatesNewUsers>::get_facade_root()
                    .map_err(internal)?,
            };
            let user = creator.create(&input).await?;
            let snapshot: Arc<dyn openrust::contracts::auth::Authenticatable> =
                user.clone_user().into();
            crate::actions::emit(Registered { user: snapshot }, &[]).await?;
            let remember = request.boolean("remember").await?;
            Auth::session_guard(request, fortify.guard().as_deref())?
                .login(user, remember)
                .await?;
            if request.has_session() {
                request
                    .session_mut()
                    .map_err(internal)?
                    .regenerate(false)
                    .await
                    .map_err(internal)?;
            }
            super::respond::<dyn contracts::RegisterResponse>(request).await
        })
    }
}

#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::{
    Fortify, LoginRateLimiter,
    actions::{credentials, emit},
    http::responses::invalid,
    resolver::internal,
    two_factor_model::{AccountEvent, present, truthy},
};
use openrust::{
    auth::{Auth, Credentials, events::Failed},
    contracts::session::Session as _,
    http::{HttpError, Request, Response, StatusCode},
    support::{
        Timebox,
        facades::{Lang, Redirect},
    },
};
use std::{sync::Arc, time::Duration};

pub struct RedirectIfTwoFactorAuthenticatable {
    fortify: Arc<Fortify>,
    limiter: Arc<LoginRateLimiter>,
}
impl RedirectIfTwoFactorAuthenticatable {
    pub fn new(fortify: Arc<Fortify>, limiter: Arc<LoginRateLimiter>) -> Self {
        Self { fortify, limiter }
    }
    pub async fn handle(&self, request: &mut Request) -> Result<Option<Response>, HttpError> {
        let credentials = credentials(request, &self.fortify).await?;
        let user = if let Some(callback) = self.fortify.authenticate_callback() {
            match callback(request).await? {
                Some(user) => user,
                None => return self.failed(request, &credentials, None).await,
            }
        } else {
            let provider =
                Auth::session_guard(request, self.fortify.guard().as_deref())?.get_provider();
            Timebox::default()
                .call(
                    async |_| {
                        let Some(mut user) = provider.retrieve_by_credentials(&credentials).await?
                        else {
                            return self.failed(request, &credentials, None).await;
                        };
                        let password = Credentials::new(
                            std::iter::empty::<(String, openrust::database::Value)>(),
                            credentials.password(),
                        );
                        if !provider
                            .validate_credentials(user.as_ref(), &password)
                            .await?
                        {
                            return self.failed(request, &credentials, Some(user)).await;
                        }
                        if self.fortify.config.get("hashing.rehash_on_login").is_none()
                            || self.fortify.truthy("hashing.rehash_on_login")
                        {
                            provider
                                .rehash_password_if_required(user.as_mut(), &password, false)
                                .await?;
                        }
                        Ok(user)
                    },
                    Duration::from_micros(200_000),
                )
                .await?
        };
        let Some(account) = self.fortify.account(user.as_ref()) else {
            return Ok(None);
        };
        if !truthy(account.attribute("two_factor_secret").map_err(internal)?)
            || (self
                .fortify
                .confirms_two_factor_authentication()
                .map_err(internal)?
                && !present(
                    account
                        .attribute("two_factor_confirmed_at")
                        .map_err(internal)?,
                ))
        {
            return Ok(None);
        }
        let remember = request.boolean("remember").await?;
        request
            .session_mut()
            .map_err(internal)?
            .put("login.id", account.key().map_err(internal)?)
            .map_err(internal)?;
        request
            .session_mut()
            .map_err(internal)?
            .put("login.remember", remember.into())
            .map_err(internal)?;
        account.emit(AccountEvent::Challenged).await?;
        if request.wants_json() {
            return Ok(Some(Response::json(
                StatusCode::OK,
                serde_json::json!({"two_factor":true}),
            )?));
        }
        Ok(Some(
            Redirect::route("two-factor.login", &[], Default::default())?.into(),
        ))
    }
    async fn failed<T>(
        &self,
        request: &mut Request,
        credentials: &Credentials,
        user: Option<Box<dyn openrust::contracts::auth::Authenticatable>>,
    ) -> Result<T, HttpError> {
        emit(
            Failed {
                guard: self
                    .fortify
                    .guard()
                    .unwrap_or(Auth::manager()?.get_default_driver()),
                user: user.map(Arc::from),
            },
            &[credentials],
        )
        .await?;
        self.limiter.increment(request).await?;
        Err(HttpError::Validation(Box::new(invalid(
            &self.fortify.username(),
            Lang::get("auth.failed", &Default::default(), None).map_err(internal)?,
        ))))
    }
}

impl crate::contracts::RedirectsIfTwoFactorAuthenticatable for RedirectIfTwoFactorAuthenticatable {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> openrust::http::HttpFuture<'a, Result<Option<Response>, HttpError>> {
        Box::pin(RedirectIfTwoFactorAuthenticatable::handle(self, request))
    }
}

use crate::{
    Features, Fortify, LoginPipe, LoginRateLimiter,
    actions::{
        AttemptToAuthenticate, CanonicalizeUsername, EnsureLoginIsNotThrottled,
        PrepareAuthenticatedSession,
    },
    contracts,
    http::requests::LoginRequest,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::Auth,
    contracts::{auth::StatefulGuard as _, session::Session as _},
    foundation::http::ValidatedFormRequest,
    http::{HttpError, HttpFuture, Request, Response},
    pipeline::Pipeline,
    support::facades::Facade as _,
};
use std::sync::Arc;

pub struct AuthenticatedSessionController;
impl AuthenticatedSessionController {
    pub fn create<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(super::respond::<dyn contracts::LoginViewResponse>(request))
    }
    pub fn store<'a>(
        &'a self,
        request: &'a mut Request,
        _: ValidatedFormRequest<LoginRequest>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            let pipes = self.login_pipeline(&fortify, request)?;
            Pipeline::send(())
                .through(pipes)
                .then_with(request, |(), request| {
                    Box::pin(super::respond::<dyn contracts::LoginResponse>(request))
                })
                .await
        })
    }
    pub fn destroy<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
            Auth::session_guard(request, fortify.guard().as_deref())?
                .logout()
                .await?;
            if request.has_session() {
                let session = request.session_mut().map_err(internal)?;
                session.invalidate().await.map_err(internal)?;
                session.regenerate_token().map_err(internal)?;
            }
            super::respond::<dyn contracts::LogoutResponse>(request).await
        })
    }
    pub fn login_pipeline(
        &self,
        fortify: &Arc<Fortify>,
        request: &Request,
    ) -> Result<Vec<LoginPipe>, HttpError> {
        if let Some(callback) = fortify.pipeline_callback() {
            return callback(request);
        }
        let configured = fortify.config.get("fortify.pipelines.login");
        let stages = if let Some(serde_json::Value::Array(values)) = configured {
            let mut names = Vec::new();
            for value in values {
                match value {
                    serde_json::Value::Null | serde_json::Value::Bool(false) => {}
                    serde_json::Value::String(name) if name.is_empty() || name == "0" => {}
                    serde_json::Value::Number(number) if number.as_f64() == Some(0.0) => {}
                    serde_json::Value::String(name) => names.push(name),
                    _ => {
                        return Err(internal(std::io::Error::other(
                            "Invalid Fortify login pipeline stage",
                        )));
                    }
                }
            }
            names
        } else {
            let mut stages = Vec::new();
            if !fortify.truthy("fortify.limiters.login") {
                stages.push("EnsureLoginIsNotThrottled".into());
            }
            if fortify.truthy("fortify.lowercase_usernames") {
                stages.push("CanonicalizeUsername".into());
            }
            if Features::new(fortify.config.clone())
                .enabled(Features::two_factor_authentication())
                .map_err(internal)?
            {
                stages.push("RedirectIfTwoFactorAuthenticatable".into());
            }
            stages.extend([
                "AttemptToAuthenticate".into(),
                "PrepareAuthenticatedSession".into(),
            ]);
            stages
        };
        let limiter = Resolver::<LoginRateLimiter>::get_facade_root().map_err(internal)?;
        stages
            .into_iter()
            .map(|name| {
                let stage = name
                    .strip_prefix("OpenRust\\Fortify\\Actions\\")
                    .or_else(|| name.strip_prefix("openrust_fortify::actions::"))
                    .unwrap_or(&name)
                    .to_owned();
                if ![
                    "EnsureLoginIsNotThrottled",
                    "CanonicalizeUsername",
                    "RedirectIfTwoFactorAuthenticatable",
                    "AttemptToAuthenticate",
                    "PrepareAuthenticatedSession",
                ]
                .contains(&stage.as_str())
                {
                    return Err(internal(std::io::Error::other(format!(
                        "Unknown Fortify login pipeline stage: {name}"
                    ))));
                }
                let fortify = fortify.clone();
                let limiter = limiter.clone();
                let pipe: LoginPipe =
                    Box::new(move |(), next| run_stage(stage, fortify, limiter, next));
                Ok(pipe)
            })
            .collect()
    }
}

fn run_stage<'a>(
    stage: String,
    fortify: Arc<Fortify>,
    limiter: Arc<LoginRateLimiter>,
    mut next: openrust::pipeline::Next<'a, (), Response, HttpError, Request>,
) -> openrust::pipeline::PipelineFuture<'a, Response, HttpError> {
    Box::pin(async move {
        let request = next.context();
        match stage.as_str() {
            "EnsureLoginIsNotThrottled" => {
                if let Some(response) = EnsureLoginIsNotThrottled::new(fortify, limiter)
                    .handle(request)
                    .await?
                {
                    return Ok(response);
                }
            }
            "RedirectIfTwoFactorAuthenticatable" => {
                if let Some(response) =
                    Resolver::<dyn contracts::RedirectsIfTwoFactorAuthenticatable>::get_facade_root(
                    )
                    .map_err(internal)?
                    .handle(request)
                    .await?
                {
                    return Ok(response);
                }
            }
            "CanonicalizeUsername" => CanonicalizeUsername::new(fortify).handle(request).await?,
            "AttemptToAuthenticate" => {
                AttemptToAuthenticate::new(fortify, limiter)
                    .handle(request)
                    .await?
            }
            "PrepareAuthenticatedSession" => {
                PrepareAuthenticatedSession::new(limiter)
                    .handle(request)
                    .await?
            }
            _ => {
                return Err(internal(std::io::Error::other(
                    "Unknown Fortify login pipeline stage",
                )));
            }
        }
        next.run(()).await
    })
}

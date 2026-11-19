use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::{
    auth::access::Argument,
    contracts::session::Session as _,
    http::Request,
    log::context::Context,
    support::facades::{Auth, Gate},
};

use super::{ViewError, ViewResult, ViewValue, rendering::render_error};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum AuthorizationMode {
    All,
    Denied,
    Any,
}

pub(super) enum GateArgument {
    Value(ViewValue),
    Typed(Arc<Argument>),
}

enum RequestCheck {
    Authenticated(Option<String>),
    Authorized(AuthorizationMode, Vec<String>, Vec<GateArgument>),
}

pub(super) struct ViewRequestQuery {
    check: RequestCheck,
    answer: oneshot::Sender<ViewResult<(bool, RequestSnapshot)>>,
}

impl ViewRequestQuery {
    pub async fn resolve(self, request: &mut Request) {
        let result = async {
            match self.check {
                RequestCheck::Authenticated(guard) => Ok(Auth::manager()?
                    .userForGuard(request, guard.as_deref())
                    .await?
                    .is_some()),
                RequestCheck::Authorized(mode, abilities, arguments) => {
                    let gate = Gate::forRequest(request).await?;
                    let abilities: Vec<_> = abilities.iter().map(String::as_str).collect();
                    let arguments: Vec<&Argument> = arguments
                        .iter()
                        .map(|argument| match argument {
                            GateArgument::Value(value) => value as &Argument,
                            GateArgument::Typed(value) => value.as_ref(),
                        })
                        .collect();
                    match mode {
                        AuthorizationMode::All => gate.check(&abilities, &arguments).await,
                        AuthorizationMode::Denied => gate
                            .check(&abilities, &arguments)
                            .await
                            .map(|allowed| !allowed),
                        AuthorizationMode::Any => gate.any(&abilities, &arguments).await,
                    }
                }
            }
        }
        .await
        .map_err(|error: crate::auth::AuthError| ViewError::RenderFailed(Box::new(error)));
        let _ = self
            .answer
            .send(result.map(|allowed| (allowed, RequestSnapshot::capture(request))));
    }
}

#[derive(Default)]
pub(super) struct ViewRequestData {
    snapshot: Mutex<RequestSnapshot>,
    checks: Option<mpsc::Sender<ViewRequestQuery>>,
    pub(super) url_uri: Option<crate::http::Uri>,
}

#[derive(Default)]
struct RequestSnapshot {
    csrf_token: Option<Arc<str>>,
    session: BTreeMap<String, serde_json::Value>,
    context: Option<serde_json::Map<String, serde_json::Value>>,
}

impl RequestSnapshot {
    fn capture(request: &Request) -> Self {
        let session = request.session().ok();
        Self {
            csrf_token: session.and_then(|session| session.token()).map(Arc::from),
            session: session.map_or_else(BTreeMap::new, |session| session.all().clone()),
            context: Context::all().ok(),
        }
    }
}

impl ViewRequestData {
    pub fn capture(request: &Request, checks: mpsc::Sender<ViewRequestQuery>) -> Self {
        Self {
            snapshot: Mutex::new(RequestSnapshot::capture(request)),
            checks: Some(checks),
            url_uri: crate::http::request_context::with_url_uri(),
        }
    }

    pub fn csrf_token(&self) -> Option<Arc<str>> {
        self.snapshot.lock().unwrap().csrf_token.clone()
    }

    pub fn session(&self, name: &str) -> Option<ViewValue> {
        crate::session::get_attribute(&self.snapshot.lock().unwrap().session, name)
            .filter(|value| !value.is_null())
            .cloned()
            .map(Into::into)
    }

    pub fn context(&self, name: &str) -> Option<ViewValue> {
        match &self.snapshot.lock().unwrap().context {
            Some(context) => context.get(name).cloned().map(Into::into),
            None => Context::get(name).ok().flatten().map(Into::into),
        }
    }

    pub fn authenticated(&self, guard: Option<&str>) -> ViewResult<bool> {
        if self.checks.is_none() {
            return Ok(false);
        }
        self.check(RequestCheck::Authenticated(guard.map(str::to_owned)))
    }

    pub fn authorized(
        &self,
        mode: AuthorizationMode,
        abilities: Vec<String>,
        arguments: Vec<GateArgument>,
    ) -> ViewResult<bool> {
        self.check(RequestCheck::Authorized(mode, abilities, arguments))
    }

    fn check(&self, check: RequestCheck) -> ViewResult<bool> {
        let sender = self
            .checks
            .as_ref()
            .ok_or_else(|| render_error("Blade authorization requires an HTTP request."))?;
        let (answer, result) = oneshot::channel();
        sender
            .blocking_send(ViewRequestQuery { check, answer })
            .map_err(|_| render_error("The Blade request is no longer available."))?;
        let (authenticated, snapshot) = result
            .blocking_recv()
            .map_err(|_| render_error("The Blade request ended without a result."))??;
        *self.snapshot.lock().unwrap() = snapshot;
        Ok(authenticated)
    }
}

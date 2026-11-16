use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use tokio::sync::{mpsc, oneshot};

use crate::{
    contracts::session::Session as _, http::Request, log::context::Context, support::facades::Auth,
};

use super::{ViewData, ViewError, ViewResult, ViewValue, rendering::render_error};

pub(super) struct ViewGuardQuery {
    guard: Option<String>,
    answer: oneshot::Sender<ViewResult<(bool, RequestSnapshot)>>,
}

impl ViewGuardQuery {
    pub async fn resolve(self, request: &mut Request) {
        let result = async {
            let manager = Auth::manager()?;
            Ok(manager
                .userForGuard(request, self.guard.as_deref())
                .await?
                .is_some())
        }
        .await
        .map_err(|error: crate::auth::AuthError| ViewError::RenderFailed(Box::new(error)));
        let _ = self
            .answer
            .send(result.map(|authenticated| (authenticated, RequestSnapshot::capture(request))));
    }
}

#[derive(Default)]
pub(super) struct ViewRequestData {
    snapshot: Mutex<RequestSnapshot>,
    guards: Option<mpsc::Sender<ViewGuardQuery>>,
}

#[derive(Default)]
struct RequestSnapshot {
    csrf_token: Option<Arc<str>>,
    session: BTreeMap<String, ViewValue>,
    context: Option<ViewData>,
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
    pub fn capture(request: &Request, guards: mpsc::Sender<ViewGuardQuery>) -> Self {
        Self {
            snapshot: Mutex::new(RequestSnapshot::capture(request)),
            guards: Some(guards),
        }
    }

    pub fn csrf_token(&self) -> Option<Arc<str>> {
        self.snapshot.lock().unwrap().csrf_token.clone()
    }

    pub fn session(&self, name: &str) -> Option<ViewValue> {
        crate::session::get_attribute(&self.snapshot.lock().unwrap().session, name)
            .filter(|value| !value.is_null())
            .cloned()
    }

    pub fn context(&self, name: &str) -> Option<ViewValue> {
        match &self.snapshot.lock().unwrap().context {
            Some(context) => context.get(name).cloned(),
            None => Context::get(name).ok().flatten(),
        }
    }

    pub fn authenticated(&self, guard: Option<&str>) -> ViewResult<bool> {
        let Some(guards) = &self.guards else {
            return Ok(false);
        };
        let (answer, result) = oneshot::channel();
        guards
            .blocking_send(ViewGuardQuery {
                guard: guard.map(str::to_owned),
                answer,
            })
            .map_err(|_| {
                render_error("The Blade authentication request is no longer available.")
            })?;
        let (authenticated, snapshot) = result.blocking_recv().map_err(|_| {
            render_error("The Blade authentication request ended without a result.")
        })??;
        *self.snapshot.lock().unwrap() = snapshot;
        Ok(authenticated)
    }
}

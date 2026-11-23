#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{
    io,
    sync::{Arc, Weak},
};

use crate::Files;
use openrust::{
    container::Container,
    contracts::{
        auth::Authenticatable,
        container::{BindingResolutionError, Container as _},
        events::{Event, Listener},
    },
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    log::{
        Level, LogError, LogResult,
        context::{Origin, Repository},
        events::MessageLogged,
    },
    queue::events::{JobAttempted, JobExceptionOccurred, JobProcessed, JobProcessing},
};
use serde_json::{Value, json};

tokio::task_local! { static RESOLVING_AUTH: (); }

/// Copies accepted logs to this application's active Pail sessions.
pub struct Handler {
    files: Arc<Files>,
    owner: Weak<Container>,
}

impl Handler {
    pub fn new(files: Arc<Files>, owner: Weak<Container>) -> Self {
        Self { files, owner }
    }

    /// Resolves authentication only at an explicitly awaited request log boundary.
    #[allow(non_snake_case, reason = "OpenRust asynchronous request log adapter")]
    pub async fn logWithRequest(
        &self,
        event: &MessageLogged,
        request: &mut openrust::http::Request,
    ) -> LogResult<()> {
        let active = self
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &Container::getInstance()));
        if active
            && matches!(Origin::current(), Some(Origin::Http(_)))
            && RESOLVING_AUTH.try_with(|()| ()).is_err()
            && !self.files.all().map_err(write_error)?.is_empty()
            && !suppressed(event)
        {
            RESOLVING_AUTH
                .scope((), async {
                    openrust::support::facades::Auth::user(request)
                        .await
                        .map_err(|error| LogError::WriteFailed(Box::new(error)))
                })
                .await?;
        }
        self.log(event)
    }

    pub fn log(&self, event: &MessageLogged) -> LogResult<()> {
        let files = self.files.all().map_err(write_error)?;
        if files.is_empty() {
            return Ok(());
        }
        if suppressed(event) {
            return Ok(());
        }
        let owner = self
            .owner
            .upgrade()
            .filter(|owner| Arc::ptr_eq(owner, &Container::getInstance()));
        let mut context = event.context.clone();
        let mut origin = match owner.as_ref().and_then(|_| Origin::current()) {
            Some(Origin::Http(request)) => {
                let user = request.user();
                let id = user.as_ref().map(|user| json!(user.getAuthIdentifier()));
                let email = user.as_deref().and_then(email);
                json!({"type":"http","method":request.method(),"path":request.path(),"auth_id":id,"auth_email":email})
            }
            Some(Origin::Console { command }) => json!({"type":"console","command":command}),
            Some(Origin::Queue {
                command,
                queue,
                job,
            }) => json!({"type":"queue","command":command,"queue":queue,"job":job}),
            None => json!({"type":"console","command":null}),
        };
        origin["trace"] = context
            .get("exception")
            .and_then(|exception| exception.get("trace"))
            .filter(|trace| trace.is_array())
            .cloned()
            .unwrap_or(Value::Null);
        context.insert("__pail".into(), json!({"origin":origin}));
        if let Some(owner) = owner
            && let Ok(repository) = owner.make::<Repository>()
        {
            context.extend(repository.all());
        }
        for file in files {
            file.log(event.level, &event.message, &context)
                .map_err(write_error)?;
        }
        Ok(())
    }
}

fn suppressed(event: &MessageLogged) -> bool {
    event.level == Level::Warning
        && ["deprecated", "Deprecated", "[\\ReturnTypeWillChange]"]
            .iter()
            .any(|needle| event.message.contains(needle))
}

fn email(user: &dyn Authenticatable) -> Option<String> {
    match user.getAuthAttribute("email").ok().flatten() {
        Some(openrust::database::Value::Text(value)) => Some(value),
        _ => None,
    }
}

fn write_error(error: io::Error) -> LogError {
    LogError::WriteFailed(Box::new(error))
}

pub(crate) struct QueueLifecycle(pub Weak<Container>);

impl Listener for QueueLifecycle {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let owner = self
                .0
                .upgrade()
                .ok_or(EventError::Unavailable(BindingResolutionError::ScopeClosed))?;
            let _handler = owner.make::<Handler>()?;
            let current = Origin::current();
            let command = match current {
                Some(Origin::Console { command }) | Some(Origin::Queue { command, .. }) => command,
                _ => return Ok(None),
            };
            let job = event
                .downcast_ref::<JobProcessing>()
                .map(|event| &event.job)
                .or_else(|| {
                    event
                        .downcast_ref::<JobExceptionOccurred>()
                        .map(|event| &event.job)
                });
            if let Some(job) = job {
                Origin::set(Origin::Queue {
                    command,
                    queue: job.queue.clone(),
                    job: job.payload.name().into(),
                });
            } else if event.downcast_ref::<JobProcessed>().is_some()
                || event.downcast_ref::<JobAttempted>().is_some()
            {
                Origin::set(Origin::Console { command });
            }
            Ok(None)
        })
    }
}

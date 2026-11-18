use std::{
    io,
    sync::{Arc, Weak},
};

use crate::Files;
use openrust::{
    container::Container,
    contracts::{
        auth::Authenticatable,
        container::Container as _,
        events::{Event, Listener},
    },
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
    log::{
        Level, LogError, LogResult,
        context::{Origin, Repository},
        events::MessageLogged,
    },
    queue::events::{JobAttempted, JobExceptionOccurred, JobProcessed, JobProcessing},
};
use serde_json::{Value, json};

/// Copies accepted logs to this application's active Pail sessions.
pub struct Handler {
    files: Arc<Files>,
    owner: Weak<Container>,
}

impl Handler {
    pub fn new(files: Arc<Files>, owner: Weak<Container>) -> Self {
        Self { files, owner }
    }

    pub fn log(&self, event: &MessageLogged) -> LogResult<()> {
        let files = self.files.all().map_err(write_error)?;
        if files.is_empty() {
            return Ok(());
        }
        if event.level == Level::Warning
            && ["deprecated", "Deprecated", "[\\ReturnTypeWillChange]"]
                .iter()
                .any(|needle| event.message.contains(needle))
        {
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
                let id = user.as_ref().map(|user| json!(user.get_auth_identifier()));
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

fn email(user: &dyn Authenticatable) -> Option<String> {
    match user.getAuthAttribute("email").ok().flatten() {
        Some(openrust::database::Value::Text(value)) => Some(value),
        _ => None,
    }
}

fn write_error(error: io::Error) -> LogError {
    LogError::WriteFailed(Box::new(error))
}

pub(crate) struct QueueLifecycle;

impl Listener for QueueLifecycle {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
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

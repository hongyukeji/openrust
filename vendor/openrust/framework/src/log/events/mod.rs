//! Events emitted by the logging service.

mod message_logged;

pub use message_logged::MessageLogged;

use std::sync::{Arc, RwLock};

use super::{Level, LogContext, LogResult};

type Listener = dyn Fn(&MessageLogged) -> LogResult<()> + Send + Sync;

#[derive(Default)]
pub(super) struct Listeners(RwLock<Vec<Arc<Listener>>>);

impl Listeners {
    pub fn listen(
        &self,
        listener: impl Fn(&MessageLogged) -> LogResult<()> + Send + Sync + 'static,
    ) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(listener));
    }

    pub fn fire(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        exception: Option<&serde_json::Value>,
    ) -> LogResult<()> {
        let listeners = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if listeners.is_empty() {
            return Ok(());
        }
        let mut context = context.clone();
        if let Some(exception) = exception {
            context.insert("exception".into(), exception.clone());
        }
        let event = MessageLogged {
            level,
            message: message.into(),
            context,
        };
        for listener in listeners {
            listener(&event)?;
        }
        Ok(())
    }
}

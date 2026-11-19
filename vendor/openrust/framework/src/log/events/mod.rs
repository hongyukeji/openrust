//! Events emitted by the logging service.

mod message_logged;

pub use message_logged::MessageLogged;

use std::sync::{Arc, RwLock};

use super::{Level, LogContext, LogResult};

type Listener = dyn Fn(&MessageLogged) -> LogResult<()> + Send + Sync;
/// Observes an accepted log while borrowing its current HTTP request.
pub type RequestListener = dyn for<'a> Fn(
        &'a MessageLogged,
        &'a mut crate::http::Request,
    ) -> crate::http::HttpFuture<'a, LogResult<()>>
    + Send
    + Sync;

#[derive(Clone)]
struct Observer {
    synchronous: Arc<Listener>,
    request: Option<Arc<RequestListener>>,
}

#[derive(Default)]
pub(super) struct Listeners(RwLock<Vec<Observer>>);

impl Listeners {
    pub fn has_listeners(&self) -> bool {
        !self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    pub fn listen(
        &self,
        listener: impl Fn(&MessageLogged) -> LogResult<()> + Send + Sync + 'static,
    ) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Observer {
                synchronous: Arc::new(listener),
                request: None,
            });
    }

    pub fn listen_for_request(
        &self,
        synchronous: impl Fn(&MessageLogged) -> LogResult<()> + Send + Sync + 'static,
        request: Arc<RequestListener>,
    ) {
        self.0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Observer {
                synchronous: Arc::new(synchronous),
                request: Some(request),
            });
    }

    pub async fn fire_for_request(
        &self,
        request: &mut crate::http::Request,
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
            match listener.request {
                Some(observer) => observer(&event, request).await?,
                None => (listener.synchronous)(&event)?,
            }
        }
        Ok(())
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
            (listener.synchronous)(&event)?;
        }
        Ok(())
    }
}

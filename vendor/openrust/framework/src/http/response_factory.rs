use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
};

use serde_json::Value;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::{ProviderResult, ServiceProvider},
};

use super::{HttpError, HttpFuture, Response};

type Callback =
    dyn Fn(Vec<Value>) -> HttpFuture<'static, Result<Response, HttpError>> + Send + Sync;

/// An asynchronous response macro that can also be registered as part of a mixin.
pub struct ResponseMacro(Arc<Callback>);

impl ResponseMacro {
    pub fn new<F, Fut>(callback: F) -> Self
    where
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, HttpError>> + Send + 'static,
    {
        Self(Arc::new(move |arguments| Box::pin(callback(arguments))))
    }
}

/// Stores the response macros for one application.
#[derive(Default)]
pub struct ResponseFactory {
    macros: RwLock<HashMap<String, ResponseMacro>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's response macro API")]
impl ResponseFactory {
    pub fn r#macro<Fut>(
        &self,
        name: impl Into<String>,
        callback: impl Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
    ) where
        Fut: Future<Output = Result<Response, HttpError>> + Send + 'static,
    {
        self.mixin([(name, ResponseMacro::new(callback))], true);
    }

    /// Adds explicitly listed macros. When replacement is disabled, existing names are kept.
    pub fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, ResponseMacro)>,
        replace: bool,
    ) {
        use std::collections::hash_map::Entry;
        for (name, callback) in macros {
            let name = name.into();
            let mut registered = self
                .macros
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let released = match registered.entry(name) {
                Entry::Vacant(entry) => {
                    entry.insert(callback);
                    None
                }
                Entry::Occupied(mut entry) if replace => Some(entry.insert(callback)),
                Entry::Occupied(_) => Some(callback),
            };
            drop(registered);
            drop(released);
        }
    }

    pub fn hasMacro(&self, name: &str) -> bool {
        self.macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(name)
    }

    pub fn flushMacros(&self) {
        let removed = std::mem::take(
            &mut *self
                .macros
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        drop(removed);
    }

    pub async fn call(&self, name: &str, arguments: Vec<Value>) -> Result<Response, HttpError> {
        let callback = self
            .macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .map(|entry| Arc::clone(&entry.0))
            .ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other(format!("response macro '{name}' does not exist"))
                    .into(),
            })?;
        callback(arguments).await
    }
}

pub(crate) struct ResponseServiceProvider;

impl ServiceProvider for ResponseServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singleton::<ResponseFactory, _>(|_| Ok(Arc::new(ResponseFactory::default())))?;
            Ok(())
        })
    }
}

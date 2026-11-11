use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::http::HttpFuture;

use super::{Error, Result};

/// The name and serialized input of a compiled application task.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    pub(super) name: String,
    pub(super) input: Value,
}

impl Task {
    pub fn new(name: impl Into<String>, input: impl Serialize) -> Result<Self> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self {
            name,
            input: serde_json::to_value(input).map_err(Error::Encoding)?,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

type Handler =
    dyn Fn(Value) -> HttpFuture<'static, std::result::Result<Value, String>> + Send + Sync;

/// Application-local handlers installed by compiled providers.
#[derive(Default)]
pub struct Registry {
    handlers: RwLock<HashMap<String, Arc<Handler>>>,
}

impl Registry {
    pub fn register<I, O, E, F>(
        &self,
        name: impl Into<String>,
        handler: impl Fn(I) -> F + Send + Sync + 'static,
    ) -> Result<()>
    where
        I: DeserializeOwned,
        O: Serialize,
        E: Display,
        F: Future<Output = std::result::Result<O, E>> + Send + 'static,
    {
        let name = name.into();
        validate_name(&name)?;
        let mut handlers = self
            .handlers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if handlers.contains_key(&name) {
            return Err(Error::DuplicateTask(name));
        }
        handlers.insert(
            name,
            Arc::new(move |input| {
                let invocation = serde_json::from_value(input).map(&handler);
                Box::pin(async move {
                    let invocation = invocation.map_err(|error| error.to_string())?;
                    let value = invocation.await.map_err(|error| error.to_string())?;
                    serde_json::to_value(value).map_err(|error| error.to_string())
                })
            }),
        );
        Ok(())
    }

    pub async fn invoke(&self, task: Task) -> Result<Value> {
        validate_name(&task.name)?;
        let handler = self
            .handlers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&task.name)
            .cloned()
            .ok_or_else(|| Error::UnknownTask(task.name.clone()))?;
        handler(task.input)
            .await
            .map_err(|message| Error::TaskFailed {
                task: task.name,
                message,
            })
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return Err(Error::InvalidTask);
    }
    Ok(())
}

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use serde_json::Value;

use crate::log::LogContext;

#[derive(Clone, Default)]
struct Fields {
    public: LogContext,
    hidden: LogContext,
}

type Overlay = (usize, Arc<Mutex<Fields>>);

tokio::task_local! {
    static OVERLAYS: Vec<Overlay>;
}

/// Stores public and hidden context for the current execution scope.
#[derive(Default)]
pub struct Repository(Mutex<Fields>);

/// Failure to access or update context.
#[derive(Debug)]
#[non_exhaustive]
pub enum ContextError {
    Unavailable(crate::contracts::container::BindingResolutionError),
    InvalidStack,
    EmptyStack,
    InvalidCounter,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable(_) => "application context is unavailable",
            Self::InvalidStack => "context value is not a list",
            Self::EmptyStack => "context stack is empty",
            Self::InvalidCounter => "context counter is not a representable integer",
        })
    }
}

impl std::error::Error for ContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

macro_rules! field_methods {
    ($field:ident, $add:ident, $many:ident, $add_if:ident, $remember:ident, $get:ident, $has:ident,
     $missing:ident, $all:ident, $forget:ident, $pull:ident, $only:ident, $except:ident,
     $push:ident, $pop:ident, $contains:ident) => {
        pub fn $add(&self, key: impl Into<String>, value: Value) {
            let key = key.into();
            self.with_fields(|fields| fields.$field.insert(key, value));
        }

        pub fn $many(&self, values: LogContext) {
            self.with_fields(|fields| fields.$field.extend(values));
        }

        pub fn $add_if(&self, key: impl Into<String>, value: Value) -> bool {
            let key = key.into();
            self.with_fields(|fields| match fields.$field.entry(key) {
                serde_json::map::Entry::Vacant(slot) => {
                    slot.insert(value);
                    true
                }
                serde_json::map::Entry::Occupied(_) => false,
            })
        }

        pub fn $remember(
            &self,
            key: impl Into<String>,
            factory: impl FnOnce() -> Value,
        ) -> Option<Value> {
            let key = key.into();
            if let Some(value) = self.with_fields(|fields| fields.$field.get(&key).cloned()) {
                return (!value.is_null()).then_some(value);
            }
            let value = factory();
            self.with_fields(|fields| {
                let stored = fields.$field.entry(key).or_insert(value);
                (!stored.is_null()).then(|| stored.clone())
            })
        }

        pub fn $get(&self, key: &str) -> Option<Value> {
            self.with_fields(|fields| {
                fields
                    .$field
                    .get(key)
                    .filter(|value| !value.is_null())
                    .cloned()
            })
        }

        pub fn $has(&self, key: &str) -> bool {
            self.with_fields(|fields| fields.$field.contains_key(key))
        }

        pub fn $missing(&self, key: &str) -> bool {
            !self.$has(key)
        }

        pub fn $all(&self) -> LogContext {
            self.with_fields(|fields| fields.$field.clone())
        }

        pub fn $forget(&self, key: &str) {
            self.with_fields(|fields| fields.$field.remove(key));
        }

        pub fn $pull(&self, key: &str) -> Option<Value> {
            self.with_fields(|fields| fields.$field.remove(key).filter(|value| !value.is_null()))
        }

        pub fn $only(&self, keys: &[&str]) -> LogContext {
            self.with_fields(|fields| {
                keys.iter()
                    .filter_map(|key| {
                        fields
                            .$field
                            .get(*key)
                            .map(|value| ((*key).to_owned(), value.clone()))
                    })
                    .collect()
            })
        }

        pub fn $except(&self, keys: &[&str]) -> LogContext {
            self.with_fields(|fields| {
                fields
                    .$field
                    .iter()
                    .filter(|(key, _)| !keys.contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
        }

        pub fn $push(
            &self,
            key: impl Into<String>,
            values: impl IntoIterator<Item = Value>,
        ) -> Result<(), ContextError> {
            let key = key.into();
            let values: Vec<_> = values.into_iter().collect();
            self.with_fields(|fields| {
                let stack = fields
                    .$field
                    .entry(key)
                    .or_insert_with(|| Value::Array(Vec::new()));
                stack
                    .as_array_mut()
                    .ok_or(ContextError::InvalidStack)?
                    .extend(values);
                Ok(())
            })
        }

        pub fn $pop(&self, key: &str) -> Result<Value, ContextError> {
            self.with_fields(|fields| {
                fields
                    .$field
                    .get_mut(key)
                    .ok_or(ContextError::EmptyStack)?
                    .as_array_mut()
                    .ok_or(ContextError::InvalidStack)?
                    .pop()
                    .ok_or(ContextError::EmptyStack)
            })
        }

        pub fn $contains(&self, key: &str, value: &Value) -> bool {
            self.with_fields(|fields| {
                fields
                    .$field
                    .get(key)
                    .and_then(Value::as_array)
                    .is_some_and(|stack| stack.contains(value))
            })
        }
    };
}

#[allow(non_snake_case, reason = "Matches Laravel's Context repository API")]
impl Repository {
    fn with_fields<T>(&self, operation: impl FnOnce(&mut Fields) -> T) -> T {
        let identity = std::ptr::from_ref(self).addr();
        let overlay = OVERLAYS
            .try_with(|overlays| {
                overlays
                    .iter()
                    .rev()
                    .find(|(key, _)| *key == identity)
                    .map(|(_, fields)| fields.clone())
            })
            .ok()
            .flatten();
        let fields = overlay.as_deref().unwrap_or(&self.0);
        operation(
            &mut fields
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    field_methods!(
        public,
        add,
        addMany,
        addIf,
        remember,
        get,
        has,
        missing,
        all,
        forget,
        pull,
        only,
        except,
        push,
        pop,
        stackContains
    );
    field_methods!(
        hidden,
        addHidden,
        addManyHidden,
        addHiddenIf,
        rememberHidden,
        getHidden,
        hasHidden,
        missingHidden,
        allHidden,
        forgetHidden,
        pullHidden,
        onlyHidden,
        exceptHidden,
        pushHidden,
        popHidden,
        hiddenStackContains
    );

    pub fn increment(&self, key: &str, amount: i64) -> Result<i64, ContextError> {
        self.counter(key, |current| current.checked_add(amount))
    }

    pub fn decrement(&self, key: &str, amount: i64) -> Result<i64, ContextError> {
        self.counter(key, |current| current.checked_sub(amount))
    }

    fn counter(
        &self,
        key: &str,
        calculate: impl FnOnce(i64) -> Option<i64>,
    ) -> Result<i64, ContextError> {
        self.with_fields(|fields| {
            let current = match fields.public.get(key) {
                None | Some(Value::Null) => 0,
                Some(value) => value.as_i64().ok_or(ContextError::InvalidCounter)?,
            };
            let next = calculate(current).ok_or(ContextError::InvalidCounter)?;
            fields.public.insert(key.to_owned(), Value::from(next));
            Ok(next)
        })
    }

    pub fn flush(&self) {
        self.with_fields(|fields| *fields = Fields::default());
    }

    pub fn isEmpty(&self) -> bool {
        self.with_fields(|fields| fields.public.is_empty() && fields.hidden.is_empty())
    }

    fn overlays(&self, data: LogContext, hidden: LogContext) -> Vec<Overlay> {
        let mut fields = self.with_fields(|fields| fields.clone());
        fields.public.extend(data);
        fields.hidden.extend(hidden);
        let mut overlays = OVERLAYS.try_with(Clone::clone).unwrap_or_default();
        overlays.push((
            std::ptr::from_ref(self).addr(),
            Arc::new(Mutex::new(fields)),
        ));
        overlays
    }

    /// Runs a future with temporary public and hidden fields.
    pub async fn scope<F: Future>(
        &self,
        future: F,
        data: LogContext,
        hidden: LogContext,
    ) -> F::Output {
        OVERLAYS.scope(self.overlays(data, hidden), future).await
    }

    /// Runs a synchronous callback with temporary fields.
    pub fn scopeSync<T>(
        &self,
        callback: impl FnOnce() -> T,
        data: LogContext,
        hidden: LogContext,
    ) -> T {
        OVERLAYS.sync_scope(self.overlays(data, hidden), callback)
    }
}

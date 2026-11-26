use std::{future::Future, sync::Arc};

use serde_json::Value;

use crate::{
    container::ServiceKey,
    contracts::container::Container as _,
    log::LogContext,
    support::facades::{Facade, current_container},
};

use super::{ContextError, Repository};

/// Accesses the current request or task's context.
pub struct Context;

impl Facade for Context {
    type Service = Repository;
    const CACHED: bool = false;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Repository>()
    }
}

macro_rules! forward {
    ($($name:ident($($param:ident: $ty:ty),*) -> $result:ty;)+) => {
        $(pub fn $name($($param: $ty),*) -> Result<$result, ContextError> {
            Ok(Self::repository()?.$name($($param),*))
        })+
    };
}

impl Context {
    pub fn repository() -> Result<Arc<Repository>, ContextError> {
        Self::get_facade_root().map_err(Into::into)
    }

    pub fn dehydrating(
        hook: impl Fn(&Repository) + Send + Sync + 'static,
    ) -> Result<(), ContextError> {
        Self::hooks()?.on_dehydrating(Arc::new(hook));
        Ok(())
    }

    pub fn hydrated(
        hook: impl Fn(&Repository) + Send + Sync + 'static,
    ) -> Result<(), ContextError> {
        Self::hooks()?.on_hydrated(Arc::new(hook));
        Ok(())
    }

    pub(crate) fn hooks() -> Result<Arc<super::Hooks>, ContextError> {
        current_container()
            .ok_or(ContextError::Unavailable(
                crate::contracts::container::BindingResolutionError::FacadeNotSet,
            ))?
            .make::<super::Hooks>()
            .map_err(Into::into)
    }

    forward! {
        add(key: impl Into<String>, value: Value) -> ();
        add_hidden(key: impl Into<String>, value: Value) -> ();
        add_many(values: LogContext) -> ();
        add_many_hidden(values: LogContext) -> ();
        add_if(key: impl Into<String>, value: Value) -> bool;
        add_hidden_if(key: impl Into<String>, value: Value) -> bool;
        remember(key: impl Into<String>, factory: impl FnOnce() -> Value) -> Option<Value>;
        remember_hidden(key: impl Into<String>, factory: impl FnOnce() -> Value) -> Option<Value>;
        get(key: &str) -> Option<Value>;
        get_hidden(key: &str) -> Option<Value>;
        has(key: &str) -> bool;
        has_hidden(key: &str) -> bool;
        missing(key: &str) -> bool;
        missing_hidden(key: &str) -> bool;
        all() -> LogContext;
        all_hidden() -> LogContext;
        forget(key: &str) -> ();
        forget_hidden(key: &str) -> ();
        pull(key: &str) -> Option<Value>;
        pull_hidden(key: &str) -> Option<Value>;
        only(keys: &[&str]) -> LogContext;
        only_hidden(keys: &[&str]) -> LogContext;
        except(keys: &[&str]) -> LogContext;
        except_hidden(keys: &[&str]) -> LogContext;
        stack_contains(key: &str, value: &Value) -> bool;
        hidden_stack_contains(key: &str, value: &Value) -> bool;
        flush() -> ();
        is_empty() -> bool;
    }

    pub fn push(
        key: impl Into<String>,
        values: impl IntoIterator<Item = Value>,
    ) -> Result<(), ContextError> {
        Self::repository()?.push(key, values)
    }

    pub fn push_hidden(
        key: impl Into<String>,
        values: impl IntoIterator<Item = Value>,
    ) -> Result<(), ContextError> {
        Self::repository()?.push_hidden(key, values)
    }

    pub fn pop(key: &str) -> Result<Value, ContextError> {
        Self::repository()?.pop(key)
    }
    pub fn pop_hidden(key: &str) -> Result<Value, ContextError> {
        Self::repository()?.pop_hidden(key)
    }
    pub fn increment(key: &str, amount: i64) -> Result<i64, ContextError> {
        Self::repository()?.increment(key, amount)
    }
    pub fn decrement(key: &str, amount: i64) -> Result<i64, ContextError> {
        Self::repository()?.decrement(key, amount)
    }

    pub async fn scope<F: Future>(
        future: F,
        data: LogContext,
        hidden: LogContext,
    ) -> Result<F::Output, ContextError> {
        Ok(Self::repository()?.scope(future, data, hidden).await)
    }

    pub fn scope_sync<T>(
        callback: impl FnOnce() -> T,
        data: LogContext,
        hidden: LogContext,
    ) -> Result<T, ContextError> {
        Ok(Self::repository()?.scope_sync(callback, data, hidden))
    }
}

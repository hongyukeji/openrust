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

#[allow(non_upper_case_globals)]
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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
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
        addHidden(key: impl Into<String>, value: Value) -> ();
        addMany(values: LogContext) -> ();
        addManyHidden(values: LogContext) -> ();
        add_if(key: impl Into<String>, value: Value) -> bool;
        addHiddenIf(key: impl Into<String>, value: Value) -> bool;
        remember(key: impl Into<String>, factory: impl FnOnce() -> Value) -> Option<Value>;
        rememberHidden(key: impl Into<String>, factory: impl FnOnce() -> Value) -> Option<Value>;
        get(key: &str) -> Option<Value>;
        get_hidden(key: &str) -> Option<Value>;
        has(key: &str) -> bool;
        hasHidden(key: &str) -> bool;
        missing(key: &str) -> bool;
        missingHidden(key: &str) -> bool;
        all() -> LogContext;
        allHidden() -> LogContext;
        forget(key: &str) -> ();
        forgetHidden(key: &str) -> ();
        pull(key: &str) -> Option<Value>;
        pullHidden(key: &str) -> Option<Value>;
        only(keys: &[&str]) -> LogContext;
        onlyHidden(keys: &[&str]) -> LogContext;
        except(keys: &[&str]) -> LogContext;
        exceptHidden(keys: &[&str]) -> LogContext;
        stackContains(key: &str, value: &Value) -> bool;
        hiddenStackContains(key: &str, value: &Value) -> bool;
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

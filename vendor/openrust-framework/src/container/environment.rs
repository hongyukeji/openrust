//! The base container delegates environment queries without imposing Application matching rules.
use super::Container;
use crate::contracts::container::BindingResolutionError;
use std::sync::Arc;

/// PHP's string-or-array argument, borrowed for a synchronous query without copying its strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentQuery<'a> {
    Name(&'a str),
    Names(&'a [&'a str]),
}
impl<'a> From<&'a str> for EnvironmentQuery<'a> {
    fn from(value: &'a str) -> Self {
        Self::Name(value)
    }
}
impl<'a> From<&'a String> for EnvironmentQuery<'a> {
    fn from(value: &'a String) -> Self {
        Self::Name(value.as_str())
    }
}
impl<'a> From<&'a [&'a str]> for EnvironmentQuery<'a> {
    fn from(value: &'a [&'a str]) -> Self {
        Self::Names(value)
    }
}
impl<'a, const N: usize> From<&'a [&'a str; N]> for EnvironmentQuery<'a> {
    fn from(value: &'a [&'a str; N]) -> Self {
        Self::Names(value)
    }
}
/// A resolver may return either PHP-supported result type; callers decide how to consume it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentResult {
    Boolean(bool),
    String(Arc<str>),
}
impl From<bool> for EnvironmentResult {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}
impl From<&str> for EnvironmentResult {
    fn from(value: &str) -> Self {
        Self::String(Arc::from(value))
    }
}
impl From<String> for EnvironmentResult {
    fn from(value: String) -> Self {
        Self::String(Arc::from(value))
    }
}
impl From<Arc<str>> for EnvironmentResult {
    fn from(value: Arc<str>) -> Self {
        Self::String(value)
    }
}
impl From<&EnvironmentResult> for bool {
    fn from(value: &EnvironmentResult) -> Self {
        match value {
            EnvironmentResult::Boolean(value) => *value,
            EnvironmentResult::String(value) => !value.is_empty() && value.as_ref() != "0",
        }
    }
}
impl From<EnvironmentResult> for bool {
    fn from(value: EnvironmentResult) -> Self {
        Self::from(&value)
    }
}
pub type EnvironmentResolver = Arc<
    dyn for<'a> Fn(EnvironmentQuery<'a>) -> Result<EnvironmentResult, BindingResolutionError>
        + Send
        + Sync,
>;
impl Container {
    pub(super) fn set_environment_resolver(&self, callback: Option<EnvironmentResolver>) {
        let retired = std::mem::replace(
            &mut self
                .registry
                .write()
                .expect("container lock poisoned")
                .environment_resolver,
            callback,
        );
        drop(retired);
    }
    pub(super) fn query_environment(
        &self,
        query: EnvironmentQuery<'_>,
    ) -> Result<EnvironmentResult, BindingResolutionError> {
        let callback = self
            .registry
            .read()
            .expect("container lock poisoned")
            .environment_resolver
            .clone();
        match callback {
            Some(callback) => callback(query),
            None => Ok(false.into()),
        }
    }
}

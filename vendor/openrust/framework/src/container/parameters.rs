//! Explicit heterogeneous arguments and per-resolution parameter frames.
use super::{Container, scope::Identity};
use crate::contracts::container::BindingResolutionError;
use std::{
    any::{Any, type_name},
    cell::RefCell,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// Exact Rust keys; numeric text is not implicitly converted to an integer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParameterKey {
    Name(Arc<str>),
    Index(i64),
}
impl ParameterKey {
    pub fn name(name: impl Into<Arc<str>>) -> Self {
        Self::Name(name.into())
    }
    pub fn class<T: ?Sized + 'static>() -> Self {
        Self::name(type_name::<T>())
    }
}

/// Shared typed value, or an explicit null. Debug output never includes the payload.
#[derive(Clone, Default)]
pub struct ParameterValue {
    value: Option<Arc<dyn Any + Send + Sync>>,
    output: Option<&'static str>,
}
impl ParameterValue {
    pub fn null() -> Self {
        Self::default()
    }
    pub fn new<T: Send + Sync + 'static>(value: T) -> Self {
        Self::from_arc(Arc::new(value))
    }
    pub fn from_arc<T: Send + Sync + 'static>(value: Arc<T>) -> Self {
        Self {
            value: Some(value),
            output: Some(type_name::<T>()),
        }
    }
    pub fn is_null(&self) -> bool {
        self.value.is_none()
    }
    pub fn downcast<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError> {
        self.value
            .as_ref()
            .and_then(|value| value.clone().downcast().ok())
            .ok_or(BindingResolutionError::ParameterTypeMismatch {
                expected: type_name::<T>(),
                actual: self.output,
            })
    }
}
impl fmt::Debug for ParameterValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.is_null() {
            "ParameterValue::Null"
        } else {
            "ParameterValue::Value(<redacted>)"
        })
    }
}
#[derive(Clone, Debug)]
struct Values {
    identity: Arc<()>,
    entries: Vec<(ParameterKey, ParameterValue)>,
}
/// Ordered copy-on-write snapshot. Replacing a key retains its input position.
#[derive(Clone, Debug, Default)]
pub struct ParameterOverrides(Option<Arc<Values>>);
impl ParameterOverrides {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, key: ParameterKey, value: ParameterValue) -> Option<ParameterValue> {
        let values = self.0.get_or_insert_with(|| {
            Arc::new(Values {
                identity: Arc::new(()),
                entries: Vec::new(),
            })
        });
        // A changed snapshot represents a separate explicit construction.
        let values = Arc::make_mut(values);
        values.identity = Arc::new(());
        if let Some((_, previous)) = values.entries.iter_mut().find(|(known, _)| known == &key) {
            return Some(std::mem::replace(previous, value));
        }
        values.entries.push((key, value));
        None
    }
    pub fn get(&self, key: &ParameterKey) -> Option<&ParameterValue> {
        self.0
            .as_ref()?
            .entries
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&ParameterKey, &ParameterValue)> {
        self.0
            .as_ref()
            .map_or(&[][..], |values| values.entries.as_slice())
            .iter()
            .map(|(key, value)| (key, value))
    }
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |values| values.entries.len())
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(super) fn identity(&self) -> Option<Arc<()>> {
        self.0.as_ref().map(|values| values.identity.clone())
    }
}
thread_local! { static FRAMES: RefCell<Vec<(Identity,ParameterOverrides)>> = const { RefCell::new(Vec::new()) }; }
pub(super) fn current(container: &Container) -> ParameterOverrides {
    FRAMES.with_borrow(|frames| {
        frames
            .iter()
            .rev()
            .find(|(identity, _)| identity == &container.identity)
            .map(|(_, parameters)| parameters.clone())
            .unwrap_or_default()
    })
}
struct Frame;
impl Drop for Frame {
    fn drop(&mut self) {
        let retired = FRAMES.with_borrow_mut(Vec::pop);
        if let Some((identity, parameters)) = retired {
            if parameters.is_empty() {
                return;
            }
            let _mask = enter(identity, ParameterOverrides::default());
            drop(parameters);
        }
    }
}
pub(super) fn with_snapshot<R>(
    identity: Identity,
    parameters: ParameterOverrides,
    operation: impl FnOnce() -> R,
) -> R {
    let _frame = enter(identity, parameters);
    operation()
}
fn enter(identity: Identity, parameters: ParameterOverrides) -> Frame {
    FRAMES.with_borrow_mut(|frames| frames.push((identity, parameters)));
    Frame
}
pub(super) fn run<R>(
    container: &Container,
    parameters: ParameterOverrides,
    operation: impl FnOnce() -> R,
) -> R {
    if parameters.is_empty() && current(container).is_empty() {
        return operation();
    }
    let _frame = enter(container.identity.clone(), parameters);
    operation()
}
struct Framed<F> {
    future: Option<Pin<Box<F>>>,
    identity: Identity,
    parameters: ParameterOverrides,
}
impl<F: Future> Future for Framed<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let _frame = enter(this.identity.clone(), this.parameters.clone());
        this.future
            .as_mut()
            .expect("parameter future")
            .as_mut()
            .poll(cx)
    }
}
impl<F> Drop for Framed<F> {
    fn drop(&mut self) {
        let parameters = std::mem::take(&mut self.parameters);
        with_snapshot(self.identity.clone(), parameters.clone(), || {
            drop(self.future.take())
        });
        with_snapshot(self.identity.clone(), ParameterOverrides::default(), || {
            drop(parameters)
        });
    }
}
pub(super) async fn run_async<F: Future>(
    container: &Container,
    parameters: ParameterOverrides,
    future: F,
) -> F::Output {
    if parameters.is_empty() {
        let mut future = std::pin::pin!(future);
        return std::future::poll_fn(|cx| {
            run(container, ParameterOverrides::default(), || {
                future.as_mut().poll(cx)
            })
        })
        .await;
    }
    Framed {
        future: Some(Box::pin(future)),
        identity: container.identity.clone(),
        parameters,
    }
    .await
}

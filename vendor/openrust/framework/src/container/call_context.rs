//! A callable's logical build stack is restored per poll, independently of resolution cycles.
use super::{Container, RESOLVING, ServiceKey, scope};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
struct ContextFrame {
    container: scope::Identity,
    stack: Arc<[ServiceKey]>,
    depth: usize,
}
thread_local! {static CURRENT:RefCell<Vec<ContextFrame>>=const{RefCell::new(Vec::new())};}
fn snapshot(container: &Container) -> Option<(Arc<[ServiceKey]>, usize)> {
    CURRENT.with_borrow(|contexts| {
        contexts
            .iter()
            .rev()
            .find(|context| context.container == container.identity)
            .map(|context| (context.stack.clone(), context.depth))
    })
}
pub(super) fn stack(container: &Container) -> Vec<ServiceKey> {
    let (mut keys, depth) = snapshot(container)
        .map(|(keys, depth)| (keys.to_vec(), depth))
        .unwrap_or_default();
    let id = std::ptr::from_ref(container).addr();
    RESOLVING.with_borrow(|frames| {
        keys.extend(
            frames
                .iter()
                .skip(depth)
                .filter(|frame| frame.container == id)
                .map(|frame| frame.key.clone()),
        )
    });
    keys
}
pub(super) fn consumer(container: &Container) -> Option<Option<ServiceKey>> {
    let (keys, depth) = snapshot(container)?;
    let id = std::ptr::from_ref(container).addr();
    Some(
        RESOLVING
            .with_borrow(|frames| {
                frames
                    .iter()
                    .skip(depth)
                    .rev()
                    .find(|frame| frame.container == id)
                    .map(|frame| frame.key.clone())
            })
            .or_else(|| keys.last().cloned()),
    )
}
struct Enter;
fn enter(container: &Container, stack: Arc<[ServiceKey]>) -> Enter {
    let depth = RESOLVING.with_borrow(Vec::len);
    CURRENT.with_borrow_mut(|contexts| {
        contexts.push(ContextFrame {
            container: container.identity.clone(),
            stack,
            depth,
        })
    });
    Enter
}
impl Drop for Enter {
    fn drop(&mut self) {
        let retired = CURRENT.with_borrow_mut(Vec::pop);
        drop(retired);
    }
}
struct Framed<'a, F> {
    container: &'a Container,
    stack: Arc<[ServiceKey]>,
    future: Option<Pin<Box<F>>>,
}
impl<F: Future> Future for Framed<'_, F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let _frame = enter(this.container, this.stack.clone());
        this.future.as_mut().expect("call future").as_mut().poll(cx)
    }
}
impl<F> Drop for Framed<'_, F> {
    fn drop(&mut self) {
        let _frame = enter(self.container, self.stack.clone());
        drop(self.future.take());
    }
}
pub(super) async fn run<F: Future>(
    container: &Container,
    stack: Vec<ServiceKey>,
    future: F,
) -> F::Output {
    Framed {
        container,
        stack: stack.into(),
        future: Some(Box::pin(future)),
    }
    .await
}

/// Cached lifecycle values retire without borrowing the clearing caller's consumer context.
pub(super) fn neutral<R>(container: &Container, operation: impl FnOnce() -> R) -> R {
    let _frame = enter(container, Arc::default());
    operation()
}

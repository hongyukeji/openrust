//! 普通容器回调的擦除守卫。一个同步阶段只使用实际存在的清理责任。
use super::super::resources::{ResourceRegistrar, ResourceTaskCompletion};
use super::disposal::{self, Credential};
use super::{TaskExecutionFailure, catch_isolated};
use crate::support::BoxFuture;
use crate::{Error, Result};
use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

type ErasedValue = Arc<dyn Any + Send + Sync>;
type Invoke<R, I, F, T> = fn(&F, R, I) -> BoxFuture<'static, Result<T>>;

/// Resolver 的原有责任来源。只有同步阶段真正取得票据后才具备归账资格。
#[derive(Clone)]
pub(crate) struct CallbackDisposal {
    completion: Option<ResourceTaskCompletion>,
    owner: Option<ResourceRegistrar>,
}
impl CallbackDisposal {
    pub(crate) fn new(
        completion: Option<ResourceTaskCompletion>,
        owner: Option<ResourceRegistrar>,
    ) -> Self {
        Self { completion, owner }
    }

    pub(crate) fn run<T>(&self, action: impl FnOnce() -> T) -> T {
        let mut cleanup = if self.completion.is_none() {
            self.owner
                .as_ref()
                .and_then(ResourceRegistrar::begin_cleanup)
        } else {
            None
        };
        let credential = self
            .completion
            .as_ref()
            .map(|completion| Credential::Completion(completion.clone()))
            .or_else(|| {
                cleanup
                    .as_ref()
                    .map(|cleanup| Credential::Cleanup(cleanup.disposal_ticket()))
            });
        let (output, _) = disposal::scope(credential, action);
        // scope 已恢复外层 TLS，有限 payload 和用户值都已处置。
        if let Some(cleanup) = &mut cleanup {
            cleanup.finish(0);
        }
        output
    }
}

/// 在返回擦除 Future 之前接管每个独立槽，直接 poll 真正的用户 Future。
pub(crate) struct CallbackFuture<R, I, F, T> {
    disposal: CallbackDisposal,
    resolver: Option<R>,
    input: Option<Result<I>>,
    erased: Option<ErasedValue>,
    callback: Option<Arc<F>>,
    future: Option<BoxFuture<'static, Result<T>>>,
    invoke: Invoke<R, I, F, T>,
}

pub(crate) fn callback_future<R, I, F, T>(
    disposal: CallbackDisposal,
    callback: Arc<F>,
    resolver: R,
    input: Result<I>,
    erased: Option<ErasedValue>,
    invoke: Invoke<R, I, F, T>,
) -> CallbackFuture<R, I, F, T> {
    CallbackFuture {
        disposal,
        resolver: Some(resolver),
        input: Some(input),
        erased,
        callback: Some(callback),
        future: None,
        invoke,
    }
}

impl<R, I, F, T> CallbackFuture<R, I, F, T> {
    fn clear(&mut self) {
        disposal::dispose(self.future.take(), None);
        disposal::dispose(self.input.take(), None);
        disposal::dispose(self.resolver.take(), None);
        disposal::dispose(self.erased.take(), None);
        disposal::dispose(self.callback.take(), None);
    }

    fn start(&mut self) -> Result<()> {
        let input = self.input.take().ok_or_else(interrupted)??;
        let Some(callback) = &self.callback else {
            return Err(interrupted());
        };
        let Some(resolver) = self.resolver.take() else {
            return Err(interrupted());
        };
        // 合法输入已有 typed Arc。Fn 接管前释放额外的擦除引用。
        disposal::dispose(self.erased.take(), None);
        let Ok(future) = catch_isolated(|| (self.invoke)(callback, resolver, input)) else {
            disposal::failure(None);
            return Err(interrupted());
        };
        self.future = Some(future);
        Ok(())
    }

    fn poll_phase(&mut self, cx: &mut Context<'_>) -> Poll<Result<T>> {
        if self.future.is_none()
            && let Err(error) = self.start()
        {
            return self.finish(Err(error));
        }
        if disposal::seen() {
            return self.finish(Err(interrupted()));
        }
        // 只能在恢复控制权后销毁用户 Future，避免 poll unwind 与 Future Drop 重叠。
        let polled = catch_isolated(|| match &mut self.future {
            Some(future) => future.as_mut().poll(cx),
            None => Poll::Ready(Err(interrupted())),
        });
        match polled {
            Ok(Poll::Pending) if !disposal::seen() => Poll::Pending,
            Ok(Poll::Pending) => self.finish(Err(interrupted())),
            Ok(Poll::Ready(result)) => self.finish(result),
            Err(()) => {
                disposal::failure(None);
                self.finish(Err(interrupted()))
            }
        }
    }

    fn finish(&mut self, result: Result<T>) -> Poll<Result<T>> {
        self.clear();
        if !disposal::seen() {
            return Poll::Ready(result);
        }
        let error = match result {
            Err(primary) => TaskExecutionFailure::error(primary, interrupted()),
            Ok(value) => {
                disposal::dispose(value, None);
                interrupted()
            }
        };
        Poll::Ready(Err(error))
    }
}

impl<R, I, F, T> Drop for CallbackFuture<R, I, F, T> {
    fn drop(&mut self) {
        if self.future.is_some() || self.callback.is_some() {
            self.disposal.clone().run(|| self.clear());
        }
    }
}
impl<R: Unpin, I: Unpin, F, T> Future for CallbackFuture<R, I, F, T> {
    type Output = Result<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.disposal.clone().run(|| this.poll_phase(cx))
    }
}

fn interrupted() -> Error {
    Error::custom("resource_task_interrupted", "外来future或其捕获析构异常")
}

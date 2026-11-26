use crate::{contracts::debug::Exception, foundation::configuration::ReportDecision};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug)]
pub struct ReportableTypeError {
    expected: &'static str,
}
impl std::fmt::Display for ReportableTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "报告回调需要异常类型 {}", self.expected)
    }
}
impl Error for ReportableTypeError {}
type Callback = dyn Fn(&Exception<'_>) -> Result<ReportDecision, ReportableTypeError> + Send + Sync;
type Matcher = dyn Fn(&Exception<'_>) -> bool + Send + Sync;

/// 注册后仍可设置停止传播；回调返回 Stop 或设置 stop 均阻止后续报告器。
#[derive(Clone)]
pub struct ReportableHandler {
    callback: Arc<Callback>,
    matches: Arc<Matcher>,
    should_stop: Arc<AtomicBool>,
}
impl ReportableHandler {
    pub fn new(
        callback: impl Fn(&Exception<'_>) -> ReportDecision + Send + Sync + 'static,
    ) -> Self {
        Self {
            callback: Arc::new(move |error| Ok(callback(error))),
            matches: Arc::new(|_| true),
            should_stop: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn for_type<E: Error + 'static>(
        callback: impl Fn(&E) -> ReportDecision + Send + Sync + 'static,
    ) -> Self {
        Self {
            callback: Arc::new(move |exception| {
                exception
                    .downcast_ref::<E>()
                    .map(&callback)
                    .ok_or(ReportableTypeError {
                        expected: std::any::type_name::<E>(),
                    })
            }),
            matches: Arc::new(|exception| exception.downcast_ref::<E>().is_some()),
            should_stop: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn handles(&self, exception: &Exception<'_>) -> bool {
        (self.matches)(exception)
    }
    pub fn invoke(&self, error: &Exception<'_>) -> Result<bool, ReportableTypeError> {
        let result = (self.callback)(error)?;
        Ok(result != ReportDecision::Stop && !self.should_stop.load(Ordering::Acquire))
    }
    pub fn stop(&self) -> &Self {
        self.should_stop.store(true, Ordering::Release);
        self
    }
}

use super::SharedException;
use crate::{
    foundation::configuration::ReportDecision,
    http::{Request, Response},
    log::LogContext,
};
use std::error::Error;

/// 异常类型自身的行为；在with_exceptions中用use_exception注册具体类型。
/// 标准Error不提供可选方法查询，因此显式建立类型元数据，方法仍在原实例上执行。
pub trait ExceptionBehavior: Error + Send + Sync + 'static {
    /// Continue采用普通报告；Stop表示类型自身已处理此次报告。
    fn report(&self) -> ReportDecision {
        ReportDecision::Continue
    }
    fn render(&self, _request: &Request) -> Option<Response> {
        None
    }
    fn context(&self) -> LogContext {
        LogContext::new()
    }
}

/// 标记不报告的异常；进入类型擦除边界前保留这一元数据。
pub trait ShouldntReport: Error + Send + Sync + Sized + 'static {
    fn into_exception(self) -> SharedException {
        SharedException::unreported(self)
    }
}

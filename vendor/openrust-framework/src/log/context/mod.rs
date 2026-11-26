//! Context associated with the current request or job.

mod facade;
mod origin;
pub use origin::{Http, Origin};
mod repository;

pub use facade::Context;
pub use repository::{ContextError, Repository};

mod dehydration;
pub use dehydration::DehydratedContext;
pub(crate) use dehydration::Hooks;

/// Context隐藏字段中的敏感操作标记；诊断采集器不得保存该作用域的业务载荷。
pub const SENSITIVE_DATA: &str = "openrust.diagnostics.sensitive";

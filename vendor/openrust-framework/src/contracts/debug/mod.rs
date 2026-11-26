//! Exception handling contract shared by the HTTP and console kernels.

mod exception_handler;

pub use exception_handler::{
    Exception, ExceptionContext, ExceptionHandler, ExceptionOwnershipError, SharedException,
};

mod exception_behavior;
pub use exception_behavior::{ExceptionBehavior, ShouldntReport};

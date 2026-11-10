//! Exception handling contract shared by the HTTP and console kernels.

mod exception_handler;

pub use exception_handler::{Exception, ExceptionHandler};

//! 对象安全 Provider、命令与领域扩展共用的异步 Future 类型。

use std::future::Future;
use std::pin::Pin;

/// 用于对象安全异步扩展的装箱 `Send` future。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

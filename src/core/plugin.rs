//! 服务插件：OpenRust 应用的统一扩展入口。

use std::any::type_name;
use std::future::Future;
use std::pin::Pin;

use crate::core::app::{App, AppBuilder};
use crate::core::error::Result;

/// 用于对象安全异步扩展的装箱 `Send` future。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 构建并完成一项能力的初始化。
///
/// 框架 crate（`DatabasePlugin`、`QueuePlugin` 等）、应用模块和第三方 package 均可实现此 trait。
/// [`build`](Self::build) 在 [`AppBuilder`] 仍可修改时同步运行，可在其中添加服务、命令和扩展。
/// 所有插件构建完成后，[`finish`](Self::finish) 会执行，并可进行打开连接池等异步工作。
///
/// ```
/// use openrust::core::{App, AppBuilder, BoxFuture, Result, Plugin};
///
/// #[derive(Clone)]
/// struct Greeter { greeting: String }
///
/// struct GreeterPlugin;
///
/// impl Plugin for GreeterPlugin {
///     fn build(&self, app: &mut AppBuilder) -> Result<()> {
///         app.insert(Greeter { greeting: "hello".into() });
///         Ok(())
///     }
///
///     fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
///         Box::pin(async move {
///             let greeter = app.state().try_get::<Greeter>()?;
///             tracing::info!(greeting = %greeter.greeting, "greeter ready");
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait Plugin: Send + Sync + 'static {
    /// 日志和错误消息中使用的名称；默认采用类型名称。
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// 构建服务、命令和扩展；按构建顺序各运行一次。
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let _ = app;
        Ok(())
    }

    /// 所有插件构建后执行异步初始化。
    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        let _ = app;
        Box::pin(async { Ok(()) })
    }
}

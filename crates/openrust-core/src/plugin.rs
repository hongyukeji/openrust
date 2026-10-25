//! Service plugins: the single extension point of an OpenRust application.

use std::any::type_name;
use std::future::Future;
use std::pin::Pin;

use crate::app::{App, AppBuilder};
use crate::error::Result;

/// Boxed, `Send` future used by object-safe async hooks.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Builds and finishes one capability.
///
/// Framework crates (`DatabasePlugin`, `QueuePlugin`, …), application modules and third-party
/// packages all implement this trait. [`build`](Self::build) runs synchronously while the
/// [`AppBuilder`] is still mutable: add services, commands and hooks there. [`finish`](Self::finish)
/// runs after every plugin has built and may perform async work such as opening pools.
///
/// ```
/// use openrust_core::{App, AppBuilder, BoxFuture, Result, Plugin};
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
    /// Name used in logs and error messages. Defaults to the type name.
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Builds services, commands and hooks. Runs once, in build order.
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let _ = app;
        Ok(())
    }

    /// Performs async initialisation after all plugins built.
    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        let _ = app;
        Box::pin(async { Ok(()) })
    }
}

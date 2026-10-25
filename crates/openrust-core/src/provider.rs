//! Service providers: the single extension point of an OpenRust application.

use std::any::type_name;
use std::future::Future;
use std::pin::Pin;

use crate::app::{App, AppBuilder};
use crate::error::Result;

/// Boxed, `Send` future used by object-safe async hooks.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Registers and boots one capability.
///
/// Framework crates (`DatabaseProvider`, `QueueProvider`, …), application modules and third-party
/// packages all implement this trait. [`register`](Self::register) runs synchronously while the
/// [`AppBuilder`] is still mutable: add services, commands and hooks there. [`boot`](Self::boot)
/// runs after every provider has registered and may perform async work such as opening pools.
///
/// ```
/// use openrust_core::{App, AppBuilder, BoxFuture, Result, ServiceProvider};
///
/// #[derive(Clone)]
/// struct Greeter { greeting: String }
///
/// struct GreeterProvider;
///
/// impl ServiceProvider for GreeterProvider {
///     fn register(&self, app: &mut AppBuilder) -> Result<()> {
///         app.provide(Greeter { greeting: "hello".into() });
///         Ok(())
///     }
///
///     fn boot<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
///         Box::pin(async move {
///             let greeter = app.state().get::<Greeter>()?;
///             tracing::info!(greeting = %greeter.greeting, "greeter ready");
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait ServiceProvider: Send + Sync + 'static {
    /// Name used in logs and error messages. Defaults to the type name.
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Registers services, commands and hooks. Runs once, in registration order.
    fn register(&self, app: &mut AppBuilder) -> Result<()> {
        let _ = app;
        Ok(())
    }

    /// Performs async initialisation after all providers registered.
    fn boot<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        let _ = app;
        Box::pin(async { Ok(()) })
    }
}

//! Application-defined database seed functions and the `db seed` command.

use std::{future::Future, sync::Arc};

use openrust_core::{App, AppBuilder, BoxFuture, Command, Plugin, Result};

use crate::Db;

/// Registers an application seed function as the `db seed` runtime command.
///
/// The application owns seed data and idempotency. Register this plugin alongside
/// [`crate::DatabasePlugin`].
pub struct SeedPlugin<F> {
    seed: Arc<F>,
}

impl<F> SeedPlugin<F> {
    /// Creates a seed command plugin from an async function that receives the database handle.
    pub fn new(seed: F) -> Self {
        Self {
            seed: Arc::new(seed),
        }
    }
}

impl<F, Fut> Plugin for SeedPlugin<F>
where
    F: Fn(Db) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.command(SeedCommand(Arc::clone(&self.seed)));
        Ok(())
    }
}

struct SeedCommand<F>(Arc<F>);

impl<F, Fut> Command for SeedCommand<F>
where
    F: Fn(Db) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn name(&self) -> &'static str {
        "db seed"
    }

    fn about(&self) -> &'static str {
        "Run the application database seed function"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let db = (*app.state().try_get::<Db>()?).clone();
            db.connection()?;
            (self.0)(db).await
        })
    }
}

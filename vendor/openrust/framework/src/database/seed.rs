//! 应用自定义数据库种子函数与 `db seed` 命令。

use std::{future::Future, sync::Arc};

use crate::console::Command;
use crate::core::Result;
use crate::foundation::Application;
use crate::foundation::BootContext;
use crate::support::BoxFuture;
use crate::support::ServiceProvider;

use crate::database::Db;

/// 在 Provider boot 阶段贡献应用自定义 `db seed` 命令。
pub struct SeedServiceProvider<F> {
    seed: Arc<F>,
}

impl<F> SeedServiceProvider<F> {
    /// 注册接收数据库句柄的异步种子函数。
    pub fn new(seed: F) -> Self {
        Self {
            seed: Arc::new(seed),
        }
    }
}

impl<F, Fut> ServiceProvider for SeedServiceProvider<F>
where
    F: Fn(Db) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(SeedCommand(Arc::clone(&self.seed)));
            Ok(())
        })
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

    fn run<'a>(&'a self, app: &'a Application, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let db = (*app.state().resolve::<Db>().await?).clone();
            db.connection()?;
            (self.0)(db).await
        })
    }
}

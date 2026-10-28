//! 应用自定义数据库种子函数与 `db seed` 命令。

use std::{future::Future, sync::Arc};

use crate::core::{App, AppBuilder, BoxFuture, Command, Plugin, Result};

use crate::db::Db;

/// 将应用种子函数注册为 `db seed` 运行时命令。
///
/// 种子数据及其幂等性由应用负责。将此插件与
/// [`DatabasePlugin`](crate::db::DatabasePlugin).
pub struct SeedPlugin<F> {
    seed: Arc<F>,
}

impl<F> SeedPlugin<F> {
    /// 根据接收数据库句柄的异步函数创建种子命令插件。
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

use std::{any::type_name, future::Future};

use futures_util::TryStreamExt as _;

use crate::{
    console::{ConsoleError, ConsoleFuture},
    contracts::debug::Exception,
    database::{DatabaseError, DatabaseResult},
    foundation::Application,
    support::facades::{App, Facade as _},
};

use super::{Builder, Model, pruning::PruningProgress};

/// 逐实例清理 Eloquent 模型，保留模型删除事件与清理钩子。
pub trait Prunable: Model {
    fn prunable() -> DatabaseResult<Builder<'static, Self>> {
        Err(DatabaseError::InvalidQuery("模型需要实现 prunable 查询"))
    }

    fn pruning(&mut self) -> impl Future<Output = DatabaseResult<()>> + Send {
        std::future::ready(Ok(()))
    }

    fn prune(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        async {
            self.pruning().await?;
            if Self::metadata().deleted_at.is_some() {
                self.force_delete().await
            } else {
                self.delete().await
            }
        }
    }

    /// 从指定应用执行清理。每个扫描批次派发一次累计进度。
    fn prune_all(app: &Application, chunk: u32) -> ConsoleFuture<'_, Result<u64, ConsoleError>> {
        Box::pin(App::set_facade_application(Some(app), async move {
            let mut progress = PruningProgress::new(app, type_name::<Self>(), None).await?;
            prune_models::<Self>(app, chunk, &mut progress).await?;
            Ok(progress.total())
        }))
    }

    fn prunable_chunk_size(requested: u32) -> u32 {
        requested
    }
}

fn candidates<M: Prunable>() -> DatabaseResult<Builder<'static, M>> {
    let query = M::prunable()?.use_write_pdo();
    Ok(if M::metadata().deleted_at.is_some() {
        query.with_trashed()
    } else {
        query
    })
}

pub(super) fn count<M: Prunable>(_: &Application) -> ConsoleFuture<'_, Result<u64, ConsoleError>> {
    Box::pin(async {
        let query = candidates::<M>()
            .and_then(Builder::scoped)
            .map_err(ConsoleError::from_source)?
            .0;
        query.count("*").await.map_err(ConsoleError::from_source)
    })
}

pub(super) fn run<'a, M: Prunable>(
    app: &'a Application,
    chunk: u32,
    progress: &'a mut PruningProgress<'_>,
) -> ConsoleFuture<'a, Result<(), ConsoleError>> {
    Box::pin(prune_models::<M>(app, chunk, progress))
}

async fn prune_models<M: Prunable>(
    app: &Application,
    chunk: u32,
    progress: &mut PruningProgress<'_>,
) -> Result<(), ConsoleError> {
    let chunk = M::prunable_chunk_size(chunk);
    if chunk == 0 {
        return Err(ConsoleError::from_source(DatabaseError::InvalidQuery(
            "模型清理批次大小必须大于零",
        )));
    }
    let query = candidates::<M>().map_err(ConsoleError::from_source)?;
    let key = query.chunk_key(None, None, false);
    let mut pages = query.chunk_collections(u64::from(chunk), Some(key), |_, _| Ok(()));
    while let Some(models) = pages.try_next().await.map_err(ConsoleError::from_source)? {
        let mut completed = 0;
        for mut model in models {
            match model.prune().await {
                Ok(_) => completed += 1,
                Err(error) => {
                    let Ok(handler) = app.exception_handler() else {
                        return Err(ConsoleError::from_source(error));
                    };
                    handler
                        .report(&Exception::Error(&error))
                        .await
                        .map_err(ConsoleError::from_source)?;
                }
            }
        }
        progress.record(completed, "已处理").await?;
    }
    Ok(())
}

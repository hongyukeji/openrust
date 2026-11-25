use std::any::type_name;

use crate::{
    console::{ConsoleError, ConsoleFuture},
    database::{DatabaseError, DatabaseResult},
    foundation::Application,
    support::facades::{App, Facade as _},
};

use super::{Builder, Model, pruning::PruningProgress};

/// 在数据库内分批删除匹配模型，派发批次进度。
pub trait MassPrunable: Model {
    fn prunable() -> DatabaseResult<Builder<'static, Self>> {
        Err(DatabaseError::InvalidQuery("模型需要实现 prunable 查询"))
    }

    fn prunable_chunk_size(requested: u32) -> u32 {
        requested
    }

    fn prune_all(app: &Application, chunk: u32) -> ConsoleFuture<'_, Result<u64, ConsoleError>> {
        Box::pin(App::set_facade_application(Some(app), async move {
            let mut progress = PruningProgress::new(app, type_name::<Self>(), None).await?;
            prune_models::<Self>(chunk, &mut progress).await?;
            Ok(progress.total())
        }))
    }
}

pub(super) fn count<M: MassPrunable>(
    _: &Application,
) -> ConsoleFuture<'_, Result<u64, ConsoleError>> {
    Box::pin(async {
        let mut query = M::prunable()
            .map_err(ConsoleError::from_source)?
            .use_write_pdo();
        if M::metadata().deleted_at.is_some() {
            query = query.with_trashed();
        }
        query
            .scoped()
            .map_err(ConsoleError::from_source)?
            .0
            .count("*")
            .await
            .map_err(ConsoleError::from_source)
    })
}

pub(super) fn run<'a, M: MassPrunable>(
    _: &'a Application,
    chunk: u32,
    progress: &'a mut PruningProgress<'_>,
) -> ConsoleFuture<'a, Result<(), ConsoleError>> {
    Box::pin(prune_models::<M>(chunk, progress))
}

async fn prune_models<M: MassPrunable>(
    chunk: u32,
    progress: &mut PruningProgress<'_>,
) -> Result<(), ConsoleError> {
    let chunk = M::prunable_chunk_size(chunk);
    if chunk == 0 {
        return Err(ConsoleError::from_source(DatabaseError::InvalidQuery(
            "模型清理批次大小必须大于零",
        )));
    }
    let query = M::prunable().map_err(ConsoleError::from_source)?;
    let query = if M::metadata().deleted_at.is_some() {
        // 与 Eloquent forceDelete 一致，直接使用显式查询约束。
        query.get_query()
    } else {
        query.scoped().map_err(ConsoleError::from_source)?.0
    };
    loop {
        let removed = query
            .delete_pruning_batch(u64::from(chunk))
            .await
            .map_err(ConsoleError::from_source)?;
        if removed == 0 {
            return Ok(());
        }
        progress.record(removed, "已清理").await?;
    }
}

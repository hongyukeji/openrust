//! 批量导入的分页、模型筛选与引擎同步；模型入口和命令共用同一实现。
use crate::{EngineManager, Result, Searchable};
use std::sync::Arc;

pub(crate) async fn import<M: Searchable>(
    manager: &Arc<EngineManager>,
    chunk: Option<u64>,
) -> Result<u64> {
    let chunk = chunk
        .or_else(|| {
            manager
                .config
                .get("scout.chunk.searchable")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(500);
    if chunk == 0 {
        return Err(crate::Error::InvalidConfiguration(
            "Searchable chunk size must be positive".into(),
        ));
    }
    let model = M::from_state(Default::default());
    let mut query = M::make_all_searchable_using(M::query()?)?;
    if manager.boolean("soft_delete") {
        query = query.with_trashed();
    }
    let count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let observed = count.clone();
    let manager = manager.clone();
    query
        .chunk_by_id(
            chunk,
            move |models, _| {
                let count = observed.clone();
                let manager = manager.clone();
                async move {
                    let models: Vec<_> = models
                        .into_iter()
                        .filter(|m| m.should_be_searchable())
                        .collect();
                    manager
                        .sync(&models, false, false)
                        .await
                        .map_err(crate::database_error)?;
                    count.fetch_add(models.len() as u64, std::sync::atomic::Ordering::Relaxed);
                    Ok(true)
                }
            },
            Some(&model.get_scout_key_name()),
            None,
        )
        .await?;
    Ok(count.load(std::sync::atomic::Ordering::Relaxed))
}

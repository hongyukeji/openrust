use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use crate::{
    console::{ConsoleError, ConsoleFuture, Output},
    contracts::events::Dispatcher as _,
    database::events::ModelsPruned,
    events::{Dispatcher, EventContext},
    foundation::Application,
};

use super::{Prunable, prunable};

/// 已编译模型的清理策略。查询、事件或附件相关副作用由模型所属的服务负责。
pub trait ModelPruner: Send + Sync {
    fn count<'a>(&'a self, app: &'a Application) -> ConsoleFuture<'a, Result<u64, ConsoleError>>;

    /// 删除至多 `chunk` 条记录并返回实际条数。返回 0 表示这一轮已经结束。
    fn prune_batch<'a>(
        &'a self,
        app: &'a Application,
        chunk: u32,
    ) -> ConsoleFuture<'a, Result<u64, ConsoleError>>;

    fn chunk_size(&self, requested: u32) -> u32 {
        requested
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RegisteredPruner {
    pub name: &'static str,
    pub source: &'static str,
    strategy: Strategy,
}

type Count = fn(&Application) -> ConsoleFuture<'_, Result<u64, ConsoleError>>;
type Run = for<'a, 'p> fn(
    &'a Application,
    u32,
    &'a mut PruningProgress<'p>,
) -> ConsoleFuture<'a, Result<(), ConsoleError>>;

#[derive(Clone, Copy)]
enum Strategy {
    Batch(&'static dyn ModelPruner),
    PerModel { count: Count, run: Run },
}

pub(crate) struct PruningProgress<'a> {
    model: &'static str,
    total: u64,
    dispatcher: Arc<Dispatcher>,
    output: Option<&'a mut Output>,
}

impl<'a> PruningProgress<'a> {
    pub(crate) async fn new(
        app: &Application,
        model: &'static str,
        output: Option<&'a mut Output>,
    ) -> Result<Self, ConsoleError> {
        Ok(Self {
            model,
            total: 0,
            dispatcher: app
                .make::<Dispatcher>()
                .await
                .map_err(ConsoleError::from_source)?,
            output,
        })
    }

    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    pub(crate) async fn record(
        &mut self,
        completed: u64,
        action: &str,
    ) -> Result<(), ConsoleError> {
        self.total = self
            .total
            .checked_add(completed)
            .ok_or_else(|| ConsoleError::new("已清理记录计数溢出"))?;
        self.dispatcher
            .dispatch(
                Arc::new(ModelsPruned {
                    model: self.model,
                    count: self.total,
                }),
                &mut EventContext::new(),
            )
            .await
            .map_err(ConsoleError::from_source)?;
        if self.total > 0
            && let Some(output) = &mut self.output
        {
            output.info(&format!("[{}] {action} {} 条记录", self.model, self.total));
        }
        Ok(())
    }
}

impl RegisteredPruner {
    pub(crate) async fn count(&self, app: &Application) -> Result<u64, ConsoleError> {
        match self.strategy {
            Strategy::Batch(pruner) => pruner.count(app).await,
            Strategy::PerModel { count, .. } => count(app).await,
        }
    }

    pub(crate) async fn run(
        &self,
        app: &Application,
        requested: u32,
        output: &mut Output,
    ) -> Result<u64, ConsoleError> {
        let mut progress = PruningProgress::new(app, self.name, Some(output)).await?;
        match self.strategy {
            Strategy::Batch(pruner) => {
                let chunk = pruner.chunk_size(requested);
                if chunk == 0 {
                    return Err(ConsoleError::new("模型清理批次大小必须大于零"));
                }
                loop {
                    let removed = pruner.prune_batch(app, chunk).await?;
                    if removed == 0 {
                        break;
                    }
                    progress.record(removed, "已清理").await?;
                }
            }
            Strategy::PerModel { run, .. } => run(app, requested, &mut progress).await?,
        }
        Ok(progress.total())
    }
}

/// 当前应用可用的模型清理策略。Provider 注册编译声明，Artisan 按名称或源目录筛选。
#[derive(Default)]
pub struct ModelPruners(RwLock<BTreeMap<&'static str, RegisteredPruner>>);

impl ModelPruners {
    pub fn register(
        &self,
        name: &'static str,
        source: &'static str,
        pruner: &'static dyn ModelPruner,
    ) -> Result<(), ConsoleError> {
        self.insert(name, source, Strategy::Batch(pruner))
    }

    /// 注册实现逐实例清理的模型，沿用同一 Artisan 发现机制。
    pub fn register_prunable<M: Prunable>(
        &self,
        name: &'static str,
        source: &'static str,
    ) -> Result<(), ConsoleError> {
        self.insert(
            name,
            source,
            Strategy::PerModel {
                count: prunable::count::<M>,
                run: prunable::run::<M>,
            },
        )
    }

    fn insert(
        &self,
        name: &'static str,
        source: &'static str,
        strategy: Strategy,
    ) -> Result<(), ConsoleError> {
        if name.is_empty() || source.is_empty() {
            return Err(ConsoleError::new("模型清理策略须声明名称和源文件"));
        }
        let mut entries = self.0.write().expect("模型清理策略锁中毒");
        match entries.entry(name) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(RegisteredPruner {
                    name,
                    source,
                    strategy,
                });
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(ConsoleError::new(format!("模型清理策略已注册：{name}")))
            }
        }
    }

    pub(crate) fn entries(&self) -> Vec<RegisteredPruner> {
        self.0
            .read()
            .expect("模型清理策略锁中毒")
            .values()
            .copied()
            .collect()
    }
}

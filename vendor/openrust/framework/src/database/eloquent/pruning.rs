use std::{collections::BTreeMap, sync::RwLock};

use crate::{
    console::{ConsoleError, ConsoleFuture},
    foundation::Application,
};

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
    pub pruner: &'static dyn ModelPruner,
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
        if name.is_empty() || source.is_empty() {
            return Err(ConsoleError::new("模型清理策略须声明名称和源文件"));
        }
        let mut entries = self.0.write().expect("模型清理策略锁中毒");
        match entries.entry(name) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(RegisteredPruner {
                    name,
                    source,
                    pruner,
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

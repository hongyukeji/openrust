//! 同一次模块清单的管理句柄；持久状态与本 App 已加载的 Provider 分开。

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use openrust::container::Resolver;
use openrust::core::{Error, Result};
use openrust::events::Dispatcher;

use super::{ModuleActivationSnapshot, ModuleMetadata, ModuleRepositorySeed, metadata_index};

struct RepositoryState {
    seed: Arc<ModuleRepositorySeed>,
    resolver: Resolver,
}

/// 已编译模块的同容器管理仓储；不扫描或执行未编译源码。
#[derive(Clone)]
pub struct ModuleRepository {
    state: Arc<RepositoryState>,
}

impl ModuleRepository {
    pub(crate) fn from_seed(seed: Arc<ModuleRepositorySeed>, resolver: Resolver) -> Self {
        Self {
            state: Arc::new(RepositoryState { seed, resolver }),
        }
    }

    /// 从同一配置仓储读取当前主模块路径；改变路径不重扫已加载清单或卸载资源。
    pub fn get_path(&self) -> Result<PathBuf> {
        let config: crate::config::ModulesConfig = self.state.resolver.section("modules")?;
        Ok(if config.paths.modules.is_absolute() {
            config.paths.modules
        } else {
            self.state.seed.application_root.join(config.paths.modules)
        })
    }

    /// 返回全部已编译模块，包含本次未加载的模块，保持声明顺序。
    pub fn all(&self) -> Vec<Module> {
        (0..self.state.seed.metadata.len())
            .map(|index| Module {
                state: Arc::clone(&self.state),
                index,
            })
            .collect()
    }

    /// 先按不区分大小写的精确名称查找，再按 Studly 名回退。
    pub fn find(&self, name: &str) -> Option<Module> {
        let entries = &self.state.seed.metadata;
        let index = metadata_index(entries, name)?;
        Some(Module {
            state: Arc::clone(&self.state),
            index,
        })
    }

    /// 查找模块；缺少已编译身份时明确失败，不返回空模块。
    pub fn find_or_fail(&self, name: &str) -> Result<Module> {
        self.find(name).ok_or_else(|| {
            Error::custom(
                "module_not_found",
                if super::valid_identity(name) && name.len() <= 128 {
                    format!("找不到已编译模块：{name}")
                } else {
                    "找不到指定的已编译模块".to_owned()
                },
            )
        })
    }

    /// 读取一次持久状态；未知键和值仅用于兼容，不成为可执行模块。
    pub fn snapshot(&self) -> Result<ModuleActivationSnapshot> {
        self.state.seed.activator.snapshot()
    }

    /// 在选择模块或触发事件前确认同一状态驱动支持持久修改。
    pub fn ensure_writable(&self) -> Result<()> {
        self.state.seed.activator.ensure_writable()
    }

    /// 按 activator 的精确布尔比较筛选；非布尔状态不同于 disabled 比较结果。
    pub fn get_by_status(&self, status: bool) -> Result<Vec<Module>> {
        let snapshot = self.snapshot()?;
        Ok(self
            .all()
            .into_iter()
            .filter(|module| snapshot.has_status(module.name(), status))
            .collect())
    }

    /// 对启用模块按优先级排序；仅 `desc` 降序，其他值沿上游使用升序。
    pub fn get_ordered(&self, direction: &str) -> Result<Vec<Module>> {
        let mut modules = self.get_by_status(true)?;
        super::priority::sort_repository_priority(
            &mut modules,
            Module::priority,
            direction == "desc",
        )?;
        Ok(modules)
    }

    /// 文件驱动的实际选定配置路径；自定义非文件驱动可以没有路径。
    pub fn statuses_file_path(&self) -> Option<&Path> {
        self.state.seed.activator.path()
    }

    /// 通过模块操作触发前后事件并启用；只影响后续 App 的加载。
    pub async fn enable(&self, name: &str) -> Result<()> {
        self.find_or_fail(name)?.enable().await
    }

    /// 通过模块操作触发前后事件并禁用；不卸载当前 App。
    pub async fn disable(&self, name: &str) -> Result<()> {
        self.find_or_fail(name)?.disable().await
    }
}

impl fmt::Debug for ModuleRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModuleRepository")
            .field("compiled_count", &self.state.seed.metadata.len())
            .finish_non_exhaustive()
    }
}

/// 共享仓储中的模块句柄；不会保存第二份 Provider 或启动另一应用。
#[derive(Clone)]
pub struct Module {
    state: Arc<RepositoryState>,
    index: usize,
}

impl Module {
    fn metadata(&self) -> &ModuleMetadata {
        &self.state.seed.metadata[self.index]
    }

    /// 编译清单中的 canonical 身份，持久状态始终使用此名称。
    pub fn name(&self) -> &str {
        &self.metadata().name
    }

    /// 当前仓储模块的清单所在目录；不使用 Manifest 文件包含目录的覆盖值。
    pub fn path(&self) -> &Path {
        &self.metadata().root
    }

    /// module.json 的原始优先级；缺失为 null，Manifest 排序另按 null→0 规则读取。
    pub fn priority(&self) -> &serde_json::Value {
        &self.metadata().priority
    }

    /// 本 App 是否已加载该模块；不会随着持久状态写入热更新。
    pub fn is_loaded(&self) -> bool {
        self.metadata().loaded
    }

    /// 读取当前持久状态是否严格为 true。
    pub fn is_enabled(&self) -> Result<bool> {
        Ok(self
            .state
            .seed
            .activator
            .snapshot()?
            .is_enabled(self.name()))
    }

    /// 对应模块方法的否定判断；与 `has_status(false)` 的非布尔边界不同。
    pub fn is_disabled(&self) -> Result<bool> {
        self.is_enabled().map(|enabled| !enabled)
    }

    /// 使用激活器的精确状态比较，不将非布尔值视为 false。
    pub fn has_status(&self, status: bool) -> Result<bool> {
        Ok(self
            .state
            .seed
            .activator
            .snapshot()?
            .has_status(self.name(), status))
    }

    /// 直接持久化状态，不触发 enable/disable 的前后事件。
    pub fn set_active(&self, active: bool) -> Result<()> {
        self.state.seed.activator.ensure_writable()?;
        self.state
            .seed
            .activator
            .set_active_by_name(self.name(), active)
    }

    /// 派发 enabling，持久化后派发 enabled；任一步异常会向调用方传播。
    pub async fn enable(&self) -> Result<()> {
        self.change(true).await
    }

    /// 派发 disabling，持久化后派发 disabled；不删除代码、资源或数据库。
    pub async fn disable(&self) -> Result<()> {
        self.change(false).await
    }

    async fn change(&self, active: bool) -> Result<()> {
        self.state.seed.activator.ensure_writable()?;
        let (before, after) = if active {
            (
                ModuleLifecyclePhase::Enabling,
                ModuleLifecyclePhase::Enabled,
            )
        } else {
            (
                ModuleLifecyclePhase::Disabling,
                ModuleLifecyclePhase::Disabled,
            )
        };
        self.emit(before).await?;
        // 前回调可能关闭应用；跨 await 后重新检查既有根生命周期，再进入同步提交点。
        let _ = self.state.resolver.is_bound::<Dispatcher>()?;
        // 文件写入没有 await；锁仅在驱动内持有，任何事件回调都在锁外运行。
        self.set_active(active)?;
        // 此处错误代表后事件失败，持久提交已经发生，不能伪称回滚。
        self.emit(after).await
    }

    async fn emit(&self, phase: ModuleLifecyclePhase) -> Result<()> {
        if !self.state.resolver.is_bound::<Dispatcher>()? {
            return Err(Error::custom(
                "module_events_unavailable",
                "模块管理所需的事件服务不可用",
            ));
        }
        // 既有 Resolver 以 Weak 持有同一容器；每次事件都遵循显式重绑定后的当前服务。
        let events = self.state.resolver.resolve_root::<Dispatcher>().await?;
        let event = ModuleLifecycleEvent {
            module: self.clone(),
            phase,
        };
        events.dispatch_named(&event.event_name(), &event).await?;
        Ok(())
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Module")
            .field("name", &self.name())
            .field("loaded", &self.is_loaded())
            .finish_non_exhaustive()
    }
}

/// 持久启停操作的四个事件阶段；不复活废弃的第二套 module boot 生命周期。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleLifecyclePhase {
    /// 启用前；失败时不写状态。
    Enabling,
    /// 启用状态已持久提交。
    Enabled,
    /// 禁用前；失败时不写状态。
    Disabling,
    /// 禁用状态已持久提交。
    Disabled,
}

impl ModuleLifecyclePhase {
    /// 固定模块包使用的阶段名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabling => "enabling",
            Self::Enabled => "enabled",
            Self::Disabling => "disabling",
            Self::Disabled => "disabled",
        }
    }
}

/// 通过同一 [`Dispatcher`] 服务发送的模块操作事件，携带可共享的真实模块管理句柄。
#[derive(Clone, Debug)]
pub struct ModuleLifecycleEvent {
    /// 操作对应的模块；写操作仍使用同一激活器。
    pub module: Module,
    /// 前事件或已经提交后的事件阶段。
    pub phase: ModuleLifecyclePhase,
}

impl ModuleLifecycleEvent {
    /// 对应 `modules.<lower-name>.<phase>` 的完整事件名称。
    pub fn event_name(&self) -> String {
        format!(
            "modules.{}.{}",
            self.module.name().to_ascii_lowercase(),
            self.phase.as_str()
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/module_repository.rs"]
mod tests;

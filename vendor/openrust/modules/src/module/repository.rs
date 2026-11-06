//! 同一次模块清单的管理句柄；持久状态与本 App 已加载的 Provider 分开。

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use openrust::container::Resolver;
use openrust::core::{Error, Result};
use openrust::events::Events;

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
        let _ = self.state.resolver.is_bound::<Events>()?;
        // 文件写入没有 await；锁仅在驱动内持有，任何事件回调都在锁外运行。
        self.set_active(active)?;
        // 此处错误代表后事件失败，持久提交已经发生，不能伪称回滚。
        self.emit(after).await
    }

    async fn emit(&self, phase: ModuleLifecyclePhase) -> Result<()> {
        if !self.state.resolver.is_bound::<Events>()? {
            return Err(Error::custom(
                "module_events_unavailable",
                "模块管理所需的事件服务不可用",
            ));
        }
        // 既有 Resolver 以 Weak 持有同一容器；每次事件都遵循显式重绑定后的当前服务。
        let events = self.state.resolver.resolve_root::<Events>().await?;
        let event = ModuleLifecycleEvent {
            module: self.clone(),
            phase,
        };
        events.dispatch_named(&event.event_name(), &event).await
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

/// 通过同一 [`Events`] 服务发送的模块操作事件，携带可共享的真实模块管理句柄。
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
mod tests {
    use std::sync::{Arc, Mutex};

    use figment::{Figment, providers::Serialized};

    use super::*;
    use crate::{ModuleDescriptor, ModuleManifest, ModuleProviderBuilderExt};
    use openrust::events::{EventPropagation, EventsServiceProvider};
    use openrust::foundation::BootContext;
    use openrust::foundation::ProviderBuilder;
    use openrust::support::BoxFuture;
    use openrust::support::ServiceProvider;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    struct Commands;
    impl ServiceProvider for Commands {
        fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                super::super::commands::register_commands(context);
                Ok(())
            })
        }
    }

    fn manifest(root: &Path) -> TestResult<ModuleManifest> {
        for name in ["blog", "taxonomy"] {
            std::fs::create_dir_all(root.join(name))?;
        }
        Ok(ModuleManifest::from_file(root.join("statuses.json"))
            .module(ModuleDescriptor::new("Blog", root.join("blog"), 10))
            .module(ModuleDescriptor::new("Taxonomy", root.join("taxonomy"), -1)))
    }

    async fn build(
        manifest: ModuleManifest,
        events: Option<EventsServiceProvider>,
    ) -> TestResult<openrust::Application> {
        let mut builder = ProviderBuilder::new();
        builder.module_manifest(manifest).provider(Commands);
        if let Some(events) = events {
            builder.provider(events);
        }
        Ok(openrust::Application::from_provider_runtime(
            builder
                .start(Figment::from(Serialized::defaults(
                    openrust::config::FrameworkConfig::default(),
                )))
                .await?,
        )?)
    }

    fn names(modules: Vec<Module>) -> Vec<String> {
        modules
            .into_iter()
            .map(|module| module.name().to_owned())
            .collect()
    }

    async fn close(app: &openrust::Application) -> Result<()> {
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await
    }

    // 区分持久键的精确比较、Module的否定语义、canonical写入和已加载快照。
    #[tokio::test]
    async fn persistent_keys_are_exact_and_non_boolean_is_not_false_status() -> TestResult {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("statuses.json"),
            r#"{"blog":true,"Old":true,"Taxonomy":"true"}"#,
        )?;
        let app = build(manifest(root.path())?, None).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let again = app.state().resolve::<ModuleRepository>().await?;
        assert!(Arc::ptr_eq(&repository, &again));
        assert_eq!(names(repository.get_by_status(false)?), ["Blog"]);
        let taxonomy = repository.find_or_fail("taxonomy")?;
        assert!(taxonomy.is_disabled()?);
        assert!(!taxonomy.has_status(false)?);
        let blog = repository.find_or_fail("blog")?;
        assert!(!blog.is_loaded());
        blog.enable().await?;
        assert!(blog.is_enabled()?);
        assert!(!blog.is_loaded());
        let snapshot = repository.snapshot()?;
        assert!(snapshot.is_enabled("blog"));
        assert!(snapshot.is_enabled("Blog"));
        assert!(snapshot.is_enabled("Old"));
        close(&app).await?;
        Ok(())
    }

    // 防止空默认Events Provider吞掉应用监听，或直接setActive误发完整启停事件。
    #[tokio::test]
    async fn configured_events_are_preserved_and_direct_set_has_no_lifecycle_events() -> TestResult
    {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("statuses.json"), r#"{"Blog":false}"#)?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, context| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                assert_eq!(context.event_name(), Some(event.event_name().as_str()));
                observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((event.event_name(), event.module.is_enabled()?));
                Ok(EventPropagation::Continue)
            })
        });
        let app = build(manifest(root.path())?, Some(events)).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let blog = repository.find_or_fail("BLOG")?;
        blog.enable().await?;
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                ("modules.blog.enabling".to_owned(), false),
                ("modules.blog.enabled".to_owned(), true),
            ]
        );
        blog.set_active(false)?;
        assert_eq!(
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            2
        );
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn before_error_does_not_write_and_after_error_does_not_undo_commit() -> TestResult {
        for after in [false, true] {
            let root = tempfile::tempdir()?;
            let original = r#"{"Blog":false,"keep":null}"#;
            std::fs::write(root.path().join("statuses.json"), original)?;
            let mut events = EventsServiceProvider::new();
            events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.*", move |event, _| {
                Box::pin(async move {
                    if event.phase
                        == if after {
                            ModuleLifecyclePhase::Enabled
                        } else {
                            ModuleLifecyclePhase::Enabling
                        }
                    {
                        return Err(Error::custom("listener_rejected", "测试监听拒绝"));
                    }
                    Ok(EventPropagation::Continue)
                })
            });
            let app = build(manifest(root.path())?, Some(events)).await?;
            let repository = app.state().resolve::<ModuleRepository>().await?;
            let error = repository
                .enable("Blog")
                .await
                .err()
                .ok_or("listener did not reject")?;
            assert_eq!(error.code(), "listener_rejected");
            assert_eq!(repository.find_or_fail("Blog")?.is_enabled()?, after);
            if !after {
                assert_eq!(
                    std::fs::read_to_string(root.path().join("statuses.json"))?,
                    original
                );
            }
            close(&app).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn stopped_event_propagation_does_not_cancel_activation() -> TestResult {
        let root = tempfile::tempdir()?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::clone(&calls);
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.*", move |event, _| {
            let first = Arc::clone(&first);
            Box::pin(async move {
                first
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.phase);
                Ok(EventPropagation::Stop)
            })
        });
        events.listen_named::<ModuleLifecycleEvent, _>("*", |_, _| {
            Box::pin(async { Err(Error::custom("unexpected_listener", "停止后不应调用")) })
        });
        let app = build(manifest(root.path())?, Some(events)).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        repository.enable("Blog").await?;
        assert!(repository.find_or_fail("Blog")?.is_enabled()?);
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                ModuleLifecyclePhase::Enabling,
                ModuleLifecyclePhase::Enabled
            ]
        );
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn command_prevalidates_all_names_and_keeps_exception_partial_progress() -> TestResult {
        let root = tempfile::tempdir()?;
        let original = r#"{"Blog":false,"Taxonomy":false}"#;
        std::fs::write(root.path().join("statuses.json"), original)?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.event_name());
                Ok(EventPropagation::Continue)
            })
        });
        events.listen_named::<ModuleLifecycleEvent, _>("modules.taxonomy.enabling", |_, _| {
            Box::pin(async { Err(Error::custom("taxonomy_rejected", "测试拒绝第二项")) })
        });
        let app = build(manifest(root.path())?, Some(events)).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let unknown = app
            .run_command("module:enable", vec!["Blog".into(), "Missing".into()])
            .await;
        assert!(unknown.is_err());
        assert!(
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("statuses.json"))?,
            original
        );
        let missing = app
            .run_command("module:enable", vec!["--no-interaction".into()])
            .await;
        assert!(missing.is_err());
        let error = app
            .run_command(
                "module:enable",
                vec!["Blog".into(), "Taxonomy".into(), "Blog".into()],
            )
            .await
            .err()
            .ok_or("second action did not fail")?;
        assert_eq!(error.code(), "taxonomy_rejected");
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                "modules.blog.enabling",
                "modules.blog.enabled",
                "modules.taxonomy.enabling",
            ]
        );
        assert!(repository.find_or_fail("Blog")?.is_enabled()?);
        assert!(!repository.find_or_fail("Taxonomy")?.is_enabled()?);
        app.run_command(
            "module:disable",
            vec!["--all".into(), "Missing".into(), "-n".into()],
        )
        .await?;
        assert!(repository.get_by_status(true)?.is_empty());
        close(&app).await?;
        Ok(())
    }

    // only未用排序时可选direction的null不应提前失败；priority才触发上游类型边界。
    #[tokio::test]
    async fn list_optional_direction_is_only_required_for_priority_sorting() -> TestResult {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("statuses.json"),
            r#"{"Blog":true,"Taxonomy":true}"#,
        )?;
        let app = build(manifest(root.path())?, None).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        assert_eq!(names(repository.get_ordered("asc")?), ["Taxonomy", "Blog"]);
        assert_eq!(names(repository.get_ordered("desc")?), ["Blog", "Taxonomy"]);
        assert_eq!(
            names(repository.get_ordered("other")?),
            ["Taxonomy", "Blog"]
        );
        for only in ["enabled", "disabled", "unknown"] {
            app.run_command(
                "module:list",
                vec![format!("--only={only}"), "--direction".into()],
            )
            .await?;
        }
        app.run_command("module:list", vec!["--direction".into()])
            .await?;
        assert!(
            app.run_command(
                "module:list",
                vec!["--only=priority".into(), "--direction".into()]
            )
            .await
            .is_err()
        );
        close(&app).await?;
        Ok(())
    }

    // 相同名称的每次动作都必须触发事件；空目录的 --all 是成功空操作。
    #[tokio::test]
    async fn duplicate_names_repeat_actions_and_empty_all_succeeds() -> TestResult {
        let root = tempfile::tempdir()?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.event_name());
                Ok(EventPropagation::Continue)
            })
        });
        let app = build(manifest(root.path())?, Some(events)).await?;
        app.run_command("module:enable", vec!["blog".into(), "Blog".into()])
            .await?;
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                "modules.blog.enabling",
                "modules.blog.enabled",
                "modules.blog.enabling",
                "modules.blog.enabled"
            ]
        );
        close(&app).await?;
        let empty = build(
            ModuleManifest::from_file(root.path().join("empty.json")),
            None,
        )
        .await?;
        empty
            .run_command("module:enable", vec!["--all".into(), "Unknown".into()])
            .await?;
        assert!(!root.path().join("empty.json").exists());
        close(&empty).await?;
        Ok(())
    }

    // 只读清单即使没有模块，也必须在 --all 的空循环前拒绝写命令。
    #[tokio::test]
    async fn registered_empty_all_rejects_status_commands() -> TestResult {
        let app = build(ModuleManifest::new(), None).await?;
        for command in ["module:enable", "module:disable"] {
            let error = app
                .run_command(command, vec!["--all".into()])
                .await
                .err()
                .ok_or("readonly empty catalog accepted mutation")?;
            assert_eq!(error.code(), "module_activator_read_only");
        }
        close(&app).await?;
        Ok(())
    }

    // 只读拒绝必须发生在真实事件派发前，避免监听器已产生不可回滚副作用。
    #[tokio::test]
    async fn registered_status_changes_never_dispatch_before_events() -> TestResult {
        let root = tempfile::tempdir()?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.phase);
                Ok(EventPropagation::Continue)
            })
        });
        let app = build(
            ModuleManifest::new()
                .module(ModuleDescriptor::new("Blog", root.path(), 0))
                .enable("Blog"),
            Some(events),
        )
        .await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let blog = repository.find_or_fail("Blog")?;
        for result in [
            blog.enable().await,
            blog.disable().await,
            blog.set_active(false),
            repository.enable("Blog").await,
            repository.disable("Blog").await,
            app.run_command("module:enable", vec!["Blog".into()]).await,
            app.run_command("module:disable", vec!["--all".into()])
                .await,
        ] {
            assert_eq!(
                result
                    .err()
                    .ok_or("readonly catalog accepted mutation")?
                    .code(),
                "module_activator_read_only"
            );
        }
        assert!(
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert!(blog.is_enabled()?);
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn programmatic_alias_is_canonical_but_temporary_disable_is_only_loaded_mask()
    -> TestResult {
        let root = tempfile::tempdir()?;
        let mut manifest = ModuleManifest::new()
            .module(ModuleDescriptor::new("BlogPost", root.path(), 0))
            .enable("blog_post");
        manifest.disable("BLOGPOST");
        let app = build(manifest, None).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let module = repository.find_or_fail("blog-post")?;
        assert!(module.is_enabled()?);
        assert!(!module.is_loaded());
        assert_eq!(
            module
                .set_active(false)
                .err()
                .ok_or("readonly driver wrote")?
                .code(),
            "module_activator_read_only"
        );
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn module_uses_current_events_after_forced_provider_rebinding() -> TestResult {
        let root = tempfile::tempdir()?;
        let mut app = build(manifest(root.path())?, None).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let blog = repository.find_or_fail("Blog")?;
        blog.enable().await?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let mut replacement = EventsServiceProvider::new();
        replacement.listen_named::<ModuleLifecycleEvent, _>("modules.blog.*", move |event, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.phase);
                Ok(EventPropagation::Continue)
            })
        });
        app.force_register_provider(replacement).await?;
        blog.disable().await?;
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                ModuleLifecyclePhase::Disabling,
                ModuleLifecyclePhase::Disabled
            ]
        );
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn dropping_pending_before_event_never_reaches_file_commit() -> TestResult {
        let root = tempfile::tempdir()?;
        let original = r#"{"Blog":false}"#;
        std::fs::write(root.path().join("statuses.json"), original)?;
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.enabling", |_, _| {
            Box::pin(std::future::pending())
        });
        let app = build(manifest(root.path())?, Some(events)).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let blog = repository.find_or_fail("Blog")?;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), blog.enable())
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("statuses.json"))?,
            original
        );
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn closing_app_in_before_event_prevents_late_file_commit() -> TestResult {
        let root = tempfile::tempdir()?;
        let original = r#"{"Blog":false}"#;
        std::fs::write(root.path().join("statuses.json"), original)?;
        let owner = Arc::new(std::sync::OnceLock::<std::sync::Weak<openrust::Application>>::new());
        let observed = Arc::clone(&owner);
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.enabling", move |_, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                let app = observed
                    .get()
                    .and_then(std::sync::Weak::upgrade)
                    .ok_or_else(|| Error::custom("owner_missing", "测试应用不存在"))?;
                close(&app).await?;
                Ok(EventPropagation::Continue)
            })
        });
        let app = Arc::new(build(manifest(root.path())?, Some(events)).await?);
        owner
            .set(Arc::downgrade(&app))
            .map_err(|_| "duplicate owner")?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        assert!(repository.enable("Blog").await.is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("statuses.json"))?,
            original
        );
        close(&app).await?;
        Ok(())
    }

    #[tokio::test]
    async fn singleton_module_handle_survives_root_scoped_reset() -> TestResult {
        let root = tempfile::tempdir()?;
        let app = build(manifest(root.path())?, None).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let blog = repository.find_or_fail("Blog")?;
        blog.enable().await?;
        app.state()
            .container()
            .forget_scoped_instances(
                tokio::time::Instant::now() + std::time::Duration::from_secs(2),
            )
            .await?;
        blog.disable().await?;
        assert!(blog.is_disabled()?);
        assert!(Arc::ptr_eq(
            &repository,
            &app.state().resolve::<ModuleRepository>().await?
        ));
        close(&app).await?;
        Ok(())
    }
}

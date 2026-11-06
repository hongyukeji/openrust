//! 运行声明到已编译 Rust 扩展的绑定；真正注册仍使用唯一 Provider 内核。

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::config::ModulesConfig;
use openrust::core::Error;
use openrust::core::Result;
use openrust::foundation::ProviderId;
use openrust::foundation::RegisterContext;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;

use super::discovery::{DiscoveredModule, is_truthy, providers, scan};
use super::{
    ModuleExecution, ModuleManifest, ModuleMetadata, ModuleRepository, ModuleRepositorySeed,
};

#[cfg(test)]
#[path = "loading_tests.rs"]
mod tests;

/// 一次已发现声明的构造上下文；未编译的 Rust 源码不会执行。
#[derive(Clone)]
pub struct ModuleLoadContext {
    name: String,
    directory: PathBuf,
    application_root: PathBuf,
}

impl ModuleLoadContext {
    /// 元数据中的原始模块名。
    pub fn name(&self) -> &str {
        &self.name
    }
    /// 当前模块在同一仓储中的目录；同名后来源胜出，不受文件包含目录覆盖影响。
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    /// 本次应用根，不与模块扫描目录或状态文件混用。
    pub fn application_root(&self) -> &Path {
        &self.application_root
    }
}

/// 显式编译的文件初始化；其副作用没有模块级事务回滚。
pub trait ModuleFile: Send + Sync + 'static {
    /// 在模块 eager Provider 注册后执行，共用当前 Container 与配置。
    fn initialize<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>>;
}

type ProviderFactory = dyn Fn(&ModuleLoadContext) -> Result<Box<dyn ServiceProvider>> + Send + Sync;

struct Binding {
    name: String,
    identity: ProviderId,
    factory: Box<ProviderFactory>,
}

struct FileBinding {
    module: Option<String>,
    path: PathBuf,
    initializer: Arc<dyn ModuleFile>,
}

#[derive(Default)]
pub(super) struct DiscoveryBindings {
    providers: Vec<Binding>,
    files: Vec<FileBinding>,
    possible: Vec<ProviderId>,
    locations: Vec<PathBuf>,
    execution: Arc<ModuleExecution>,
}

impl ModuleManifest {
    pub(crate) fn has_discovery(&self) -> bool {
        self.discovery.is_some()
    }

    fn selected_activator(
        &self,
        config: &ModulesConfig,
    ) -> Result<Arc<dyn super::ModuleActivator>> {
        if self.config_managed {
            if matches!(config.activator.as_str(), "file" | "file-live") {
                let path = &config.activators.file.statuses_file;
                let driver = super::FileModuleActivator::new(
                    if path.as_os_str().is_empty() || path.is_absolute() {
                        path.clone()
                    } else {
                        self.application_root.join(path)
                    },
                )?;
                return Ok(Arc::new(if config.activator == "file-live" {
                    driver.with_live_reads()
                } else {
                    driver
                }));
            }
            return self
                .activators
                .get(&config.activator)
                .ok_or_else(|| Error::custom("module_activator_missing", "模块启用源未登记"))?
                .build();
        }
        if self.explicit_selected {
            Ok(Arc::new(super::ExplicitActivator {
                enabled: self.explicit.enabled.clone(),
            }))
        } else {
            self.activators
                .get(&self.selected)
                .ok_or_else(|| Error::custom("module_activator_missing", "模块启用源未登记"))?
                .build()
        }
    }

    /// 按同一配置建立运行发现清单；只执行后续显式登记的已编译绑定。
    pub fn discover_from_config(
        application_root: impl AsRef<Path>,
        config: &ModulesConfig,
    ) -> Self {
        let mut manifest = Self::from_config(application_root, config);
        manifest.discovery = Some(DiscoveryBindings::default());
        manifest
    }

    /// 为 module.json 的 Provider 名称登记类型工厂；名称重复在 package register 时拒绝。
    #[must_use]
    pub fn compiled_provider<P, F>(mut self, name: impl Into<String>, factory: F) -> Self
    where
        P: ServiceProvider,
        F: Fn(&ModuleLoadContext) -> Result<P> + Send + Sync + 'static,
    {
        let bindings = self
            .discovery
            .get_or_insert_with(DiscoveryBindings::default);
        bindings.providers.push(Binding {
            name: name.into(),
            identity: ProviderId::of::<P>(),
            factory: Box::new(move |context| {
                factory(context).map(|provider| Box::new(provider) as Box<dyn ServiceProvider>)
            }),
        });
        self
    }

    /// 补充编译工厂内部可能注册的类型，供外部 typed 依赖在实际执行前再核实。
    #[must_use]
    pub fn possible_providers(mut self, identities: impl IntoIterator<Item = ProviderId>) -> Self {
        self.discovery
            .get_or_insert_with(DiscoveryBindings::default)
            .possible
            .extend(identities);
        self
    }

    /// 将实际文件路径绑定到已编译初始化；相对路径以 `application_root` 解析。
    #[must_use]
    pub fn compiled_file(mut self, path: impl Into<PathBuf>, initializer: impl ModuleFile) -> Self {
        self.discovery
            .get_or_insert_with(DiscoveryBindings::default)
            .files
            .push(FileBinding {
                module: None,
                path: path.into(),
                initializer: Arc::new(initializer),
            });
        self
    }

    /// 为指定模块目录中的文件登记编译初始化，跟随元数据的 `module_directory`。
    #[must_use]
    pub fn compiled_module_file(
        mut self,
        module: impl Into<String>,
        path: impl Into<PathBuf>,
        initializer: impl ModuleFile,
    ) -> Self {
        self.discovery
            .get_or_insert_with(DiscoveryBindings::default)
            .files
            .push(FileBinding {
                module: Some(module.into()),
                path: path.into(),
                initializer: Arc::new(initializer),
            });
        self
    }

    /// 在主路径之前追加扫描位置，保留次序及重复项；每个位置匹配直接子模块。
    #[must_use]
    pub fn add_location(mut self, path: impl Into<PathBuf>) -> Self {
        self.discovery
            .get_or_insert_with(DiscoveryBindings::default)
            .locations
            .push(path.into());
        self
    }

    /// 共享显式执行会话的 `include_once` 集合，不共享 Container 或 Provider 实例。
    #[must_use]
    pub fn execution(mut self, execution: Arc<ModuleExecution>) -> Self {
        self.discovery
            .get_or_insert_with(DiscoveryBindings::default)
            .execution = execution;
        self
    }

    pub(crate) fn into_discovery_provider(mut self) -> Option<ModulesServiceProvider> {
        let bindings = self.discovery.take()?;
        Some(ModulesServiceProvider {
            manifest: self,
            bindings,
        })
    }
}

/// 模块包在 register 阶段装载运行声明，不建立第二套生命周期。
pub(crate) struct ModulesServiceProvider {
    manifest: ModuleManifest,
    bindings: DiscoveryBindings,
}

impl ServiceProvider for ModulesServiceProvider {
    fn possible_nested_providers(&self) -> Vec<ProviderId> {
        self.bindings
            .providers
            .iter()
            .map(|binding| binding.identity)
            .chain(self.bindings.possible.iter().copied())
            .collect()
    }

    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let manifest = &self.manifest;
            manifest.validate_application_root()?;
            if !manifest.modules.is_empty() {
                return Err(Error::custom(
                    "module_manifest_mixed_sources",
                    "运行发现清单不能混入旧式编译模块描述；将扩展登记为命名编译绑定",
                ));
            }
            let config: ModulesConfig = app.config_repository().section("modules")?;
            let activator = manifest.selected_activator(&config)?;
            let execution_manifest = self.snapshot(manifest, &config, activator.as_ref())?;
            let selected: Vec<_> = execution_manifest.selected.iter().collect();
            let metadata = repository_metadata(&execution_manifest.catalog, &selected);
            let seed = Arc::new(ModuleRepositorySeed {
                metadata,
                activator,
                application_root: manifest.application_root.clone(),
            });
            let factory_seed = Arc::clone(&seed);
            app.singleton::<ModuleRepository, _>(move |resolver| {
                let seed = Arc::clone(&seed);
                Box::pin(async move { Ok(Arc::new(ModuleRepository::from_seed(seed, resolver))) })
            })
            .await?;
            let bindings = provider_bindings(&self.bindings.providers)?;
            // PHP 类 alias 不是 Container alias；尚未完成的语义明确失败，不能静默丢弃。
            if selected.iter().any(|module| {
                module.attributes.get("aliases").is_some_and(|aliases| {
                    matches!(aliases, Value::Array(_) | Value::Object(_)) && is_truthy(aliases)
                })
            }) {
                return Err(Error::custom(
                    "module_class_alias_unsupported",
                    "模块 PHP 类别名尚无等价 Rust 编译接线，不能作为容器别名加载",
                ));
            }
            for module in &selected {
                let context = ModuleLoadContext {
                    name: module.name.clone(),
                    directory: factory_seed.metadata[super::metadata_index(
                        &factory_seed.metadata,
                        &module.name,
                    )
                    .ok_or_else(|| {
                        Error::custom("module_not_found", "模块编译工厂缺少对应仓储目录")
                    })?]
                    .root
                    .clone(),
                    application_root: manifest.application_root.clone(),
                };
                for value in providers(std::slice::from_ref(*module)) {
                    let name = value.as_str().ok_or_else(|| {
                        Error::custom(
                            "module_provider_name_invalid",
                            "有效 Provider 声明必须能映射到已编译名称",
                        )
                    })?;
                    let binding = bindings.get(name).ok_or_else(|| {
                        Error::custom(
                            "module_provider_not_compiled",
                            "启用模块的 Provider 尚无编译绑定",
                        )
                    })?;
                    let provider = (binding.factory)(&context)?;
                    if provider.id() != binding.identity {
                        return Err(Error::custom(
                            "module_provider_identity_invalid",
                            "编译工厂返回的 Provider 身份与声明不符",
                        ));
                    }
                    app.boxed_register_provider(provider, false).await?;
                }
            }
            for module in &selected {
                self.initialize_files(module, &manifest.application_root, app)
                    .await?;
            }
            Ok(())
        })
    }
}

pub(super) struct ExecutionManifest {
    selected: Vec<DiscoveredModule>,
    catalog: Vec<DiscoveredModule>,
}

impl ModulesServiceProvider {
    fn snapshot(
        &self,
        manifest: &ModuleManifest,
        config: &ModulesConfig,
        activator: &dyn super::ModuleActivator,
    ) -> Result<Arc<ExecutionManifest>> {
        let cached = self
            .bindings
            .execution
            .manifest
            .lock()
            .map_err(|_| Error::custom("module_execution_poisoned", "模块发现执行状态不可用"))?
            .clone();
        if let Some(cached) = &cached
            && !cached.catalog.is_empty()
        {
            return Ok(Arc::clone(cached));
        }
        // Filesystem 与自定义 activator 均在缓存锁之外执行。
        let paths = scan_paths(&manifest.application_root, config, &self.bindings.locations);
        let discovered = scan(&paths)?;
        let selected = if let Some(cached) = &cached {
            cached.selected.clone()
        } else {
            let snapshot = activator.snapshot()?;
            let mut selected: Vec<_> = discovered
                .iter()
                .filter(|module| {
                    snapshot.is_enabled(&module.name)
                        && !manifest
                            .disabled
                            .iter()
                            .any(|name| name.eq_ignore_ascii_case(&module.name))
                })
                .cloned()
                .collect();
            super::priority::sort_by_priority(
                &mut selected,
                |module| module.attributes.get("priority").unwrap_or(&Value::Null),
                false,
            )?;
            selected
        };
        let snapshot = Arc::new(ExecutionManifest {
            selected,
            catalog: discovered,
        });
        let mut stored =
            self.bindings.execution.manifest.lock().map_err(|_| {
                Error::custom("module_execution_poisoned", "模块发现执行状态不可用")
            })?;
        match &*stored {
            Some(current) if cached.is_none() || !current.catalog.is_empty() => {
                Ok(Arc::clone(current))
            }
            _ => {
                *stored = Some(Arc::clone(&snapshot));
                Ok(snapshot)
            }
        }
    }
}

impl ModulesServiceProvider {
    async fn initialize_files(
        &self,
        module: &DiscoveredModule,
        application_root: &Path,
        app: &mut RegisterContext<'_>,
    ) -> Result<()> {
        let Some(files) = module
            .attributes
            .get("files")
            .filter(|files| is_truthy(files))
        else {
            return Ok(());
        };
        let entries: Vec<_> = match files {
            Value::Array(values) => values.iter().collect(),
            Value::Object(values) => values.values().collect(),
            _ => {
                return Err(Error::custom(
                    "module_files_invalid",
                    "模块 files 必须为可遍历声明",
                ));
            }
        };
        for file in entries {
            let file = file_scalar_string(file, "module_file_name_invalid")?;
            // PHP 拼接字符串，即使 file 以分隔符开头也不丢弃 module_directory。
            let mut joined = file_directory(module)?;
            joined.push(std::path::MAIN_SEPARATOR_STR);
            joined.push(&file);
            let path = PathBuf::from(joined).canonicalize().map_err(|_| {
                Error::custom("module_file_missing", "模块声明文件不存在或无法解析")
            })?;
            if !path.is_file() {
                return Err(Error::custom(
                    "module_file_not_regular",
                    "模块声明的包含路径不是普通文件",
                ));
            }
            if self.bindings.execution.wait_if_started(&path).await? {
                continue;
            }
            let mut matched = None;
            for binding in &self.bindings.files {
                let compiled = if let Some(name) = &binding.module {
                    if name != &module.name {
                        continue;
                    }
                    let mut path = file_directory(module)?;
                    path.push(std::path::MAIN_SEPARATOR_STR);
                    path.push(&binding.path);
                    PathBuf::from(path)
                } else if binding.path.is_absolute() {
                    binding.path.clone()
                } else {
                    application_root.join(&binding.path)
                };
                if compiled.canonicalize().ok().as_ref() == Some(&path) {
                    if matched.is_some() {
                        return Err(Error::custom(
                            "module_file_binding_conflict",
                            "同一模块文件存在多个编译绑定",
                        ));
                    }
                    matched = Some(&binding.initializer);
                }
            }
            let Some(initializer) = matched else {
                // 另一 App 可能在绑定查找期间成为 owner；共享执行已经开始时等待其结果。
                if self.bindings.execution.wait_if_started(&path).await? {
                    continue;
                }
                return Err(Error::custom(
                    "module_file_not_compiled",
                    "模块声明文件尚无编译初始化绑定",
                ));
            };
            self.bindings
                .execution
                .initialize(path, initializer.as_ref(), app)
                .await?;
        }
        Ok(())
    }
}

fn file_scalar_string(value: &Value, code: &'static str) -> Result<String> {
    if value.is_null() {
        return Ok(String::new());
    }
    super::priority::priority_label(value)
        .map_err(|_| Error::custom(code, "模块文件路径值无法按 PHP 标量规则转换为字符串"))
}

fn file_directory(module: &DiscoveredModule) -> Result<OsString> {
    match module.attributes.get("module_directory") {
        None => Ok(module.root.as_os_str().to_os_string()),
        Some(value) => {
            file_scalar_string(value, "module_file_directory_invalid").map(OsString::from)
        }
    }
}

fn provider_bindings(bindings: &[Binding]) -> Result<HashMap<&str, &Binding>> {
    let mut table = HashMap::new();
    for binding in bindings {
        if binding.name.is_empty() || table.insert(binding.name.as_str(), binding).is_some() {
            return Err(Error::custom(
                "module_provider_binding_conflict",
                "模块编译 Provider 名称为空或重复",
            ));
        }
    }
    Ok(table)
}

fn scan_paths(root: &Path, config: &ModulesConfig, locations: &[PathBuf]) -> Vec<PathBuf> {
    locations
        .iter()
        .chain(std::iter::once(&config.paths.modules))
        .chain(config.scan.paths.iter().filter(|_| config.scan.enabled))
        .map(|path| {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            };
            if path.file_name().is_some_and(|name| name == "*") {
                path
            } else {
                path.join("*")
            }
        })
        .collect()
}

fn repository_metadata(
    all: &[DiscoveredModule],
    selected: &[&DiscoveredModule],
) -> Vec<ModuleMetadata> {
    let mut metadata: Vec<ModuleMetadata> = Vec::new();
    for module in all {
        let entry = ModuleMetadata {
            name: module.name.clone(),
            root: module.root.clone(),
            priority: module
                .attributes
                .get("priority")
                .cloned()
                .unwrap_or(Value::Null),
            loaded: selected.iter().any(|loaded| {
                loaded.manifest_path == module.manifest_path && loaded.name == module.name
            }),
        };
        if let Some(previous) = metadata
            .iter_mut()
            .find(|old| old.name.eq_ignore_ascii_case(&entry.name))
        {
            *previous = entry;
        } else {
            metadata.push(entry);
        }
    }
    metadata
}

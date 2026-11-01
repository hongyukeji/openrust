//! 编译期模块描述与每次应用启动读取的启用快照。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{BootContext, Error, Result, ServiceProvider};

/// 已编译模块的静态描述；只包含应用启动可选择的 Rust 代码。
pub struct ModuleDescriptor {
    pub(crate) name: &'static str,
    pub(crate) root: PathBuf,
    pub(crate) priority: i32,
    pub(crate) requirements: Vec<String>,
    pub(crate) providers: Vec<Box<dyn ServiceProvider>>,
}

impl ModuleDescriptor {
    /// 声明模块身份、运行时根路径及升序启动优先级；相对路径基于启动时工作目录。
    pub fn new(name: &'static str, root: impl Into<PathBuf>, priority: i32) -> Self {
        Self {
            name,
            root: root.into(),
            priority,
            requirements: Vec::new(),
            providers: Vec::new(),
        }
    }

    /// 声明另一个已编译且必须启用的模块；依赖模块先于当前模块启动。
    #[must_use]
    pub fn requires(mut self, module: impl Into<String>) -> Self {
        self.requirements.push(module.into());
        self
    }

    /// 添加本模块的 Provider，保留声明顺序。
    #[must_use]
    pub fn provider(mut self, provider: impl ServiceProvider) -> Self {
        self.providers.push(Box::new(provider));
        self
    }

    /// 添加本模块的已盒装 Provider。
    #[must_use]
    pub fn boxed_provider(mut self, provider: Box<dyn ServiceProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// 已编译模块名称。
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// 已规范化的模块资源根路径。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 模块优先级，较小值先启动。
    pub fn priority(&self) -> i32 {
        self.priority
    }
}

mod activator;
mod commands;
mod discovery;
mod execution;
mod loading;
mod priority;
mod repository;

pub use activator::{FileModuleActivator, ModuleActivationSnapshot, ModuleActivator};
pub(crate) use commands::register_commands;
pub use execution::ModuleExecution;
pub use loading::{ModuleFile, ModuleLoadContext};
pub use repository::{Module, ModuleLifecycleEvent, ModuleLifecyclePhase, ModuleRepository};

#[derive(Default)]
struct ExplicitActivator {
    enabled: HashSet<String>,
}

impl ModuleActivator for ExplicitActivator {
    fn enabled(&self) -> Result<HashSet<String>> {
        Ok(self.enabled.clone())
    }
}

#[derive(Clone)]
pub(crate) struct ModuleMetadata {
    name: String,
    root: PathBuf,
    priority: serde_json::Value,
    loaded: bool,
}

pub(crate) struct ModuleRepositorySeed {
    metadata: Vec<ModuleMetadata>,
    activator: Arc<dyn ModuleActivator>,
    application_root: PathBuf,
}

pub(crate) struct PreparedModules {
    pub modules: Vec<ModuleDescriptor>,
    pub seed: ModuleRepositorySeed,
}

/// 已编译模块目录和独立的运行启用源。
pub struct ModuleManifest {
    discovery: Option<loading::DiscoveryBindings>,
    config_managed: bool,
    application_root: PathBuf,
    application_root_error: bool,
    modules: Vec<ModuleDescriptor>,
    explicit: ExplicitActivator,
    activators: HashMap<String, Arc<dyn ModuleActivator>>,
    selected: String,
    explicit_selected: bool,
    disabled: HashSet<String>,
}

impl Default for ModuleManifest {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleManifest {
    /// 建立空清单；未显式启用的模块不参与应用启动。
    pub fn new() -> Self {
        let root = std::env::current_dir();
        let application_root_error = root.is_err();
        Self {
            discovery: None,
            config_managed: false,
            application_root: root.unwrap_or_default(),
            application_root_error,
            modules: Vec::new(),
            explicit: ExplicitActivator::default(),
            activators: HashMap::new(),
            selected: "explicit".into(),
            explicit_selected: true,
            disabled: HashSet::new(),
        }
    }

    /// 使用文件激活器建立清单；状态按每次 App 启动读取，缺文件表示全禁用。
    pub fn from_file(path: impl Into<PathBuf>) -> Self {
        let mut manifest = Self::new();
        manifest
            .activators
            .insert("file".to_owned(), Arc::new(FileModuleActivator::new(path)));
        "file".clone_into(&mut manifest.selected);
        manifest.explicit_selected = false;
        manifest
    }

    /// 从同次应用配置选择激活器；状态路径相对应用根，未登记驱动在准备时明确失败。
    pub fn from_config(
        application_root: impl AsRef<Path>,
        config: &crate::config::ModulesConfig,
    ) -> Self {
        let selected = &config.activators.file.statuses_file;
        let path = if selected.is_absolute() {
            selected.clone()
        } else {
            application_root.as_ref().join(selected)
        };
        let mut manifest = Self::from_file(path);
        match std::path::absolute(application_root.as_ref()) {
            Ok(root) => {
                manifest.application_root = root;
                manifest.application_root_error = false;
            }
            Err(_) => manifest.application_root_error = true,
        }
        manifest.config_managed = true;
        manifest.selected.clone_from(&config.activator);
        manifest.explicit_selected = false;
        manifest
    }

    /// 添加已编译的模块代码描述。
    #[must_use]
    pub fn module(mut self, module: ModuleDescriptor) -> Self {
        self.modules.push(module);
        self
    }

    /// 仅对显式启用源增加名称；名称仍须匹配已编译模块。
    #[must_use]
    pub fn enable(mut self, name: impl Into<String>) -> Self {
        self.explicit.enabled.insert(name.into());
        self
    }

    /// 对本次应用构建禁用已编译模块；不修改激活器持久状态。
    pub fn disable(&mut self, name: impl Into<String>) -> &mut Self {
        self.disabled.insert(name.into());
        self
    }

    /// 注册应用自定义启用源；驱动可显式支持写入，同名或保留名称拒绝。
    pub fn register_activator(
        &mut self,
        name: impl Into<String>,
        activator: impl ModuleActivator,
    ) -> Result<&mut Self> {
        let name = name.into();
        if name == "explicit" || self.activators.contains_key(&name) {
            return Err(Error::custom(
                "module_activator_conflict",
                "模块启用源名称重复",
            ));
        }
        self.activators.insert(name, Arc::new(activator));
        Ok(self)
    }

    /// 选择本次启动的启用源；未注册名称在准备阶段失败，不回退。
    pub fn use_activator(&mut self, name: impl Into<String>) -> &mut Self {
        self.config_managed = false;
        self.selected = name.into();
        self.explicit_selected = self.selected == "explicit";
        self
    }

    /// 按固定模块包的顺序查找：先不区分大小写的精确身份，再尝试 Studly 名称。
    pub fn find(&self, name: &str) -> Option<&ModuleDescriptor> {
        find_module(&self.modules, name)
    }

    #[cfg(test)]
    pub(crate) fn prepare(self) -> Result<Vec<ModuleDescriptor>> {
        self.prepare_runtime().map(|prepared| prepared.modules)
    }

    pub(crate) fn prepare_runtime(self) -> Result<PreparedModules> {
        self.validate_application_root()?;
        let activator: Arc<dyn ModuleActivator> = if self.explicit_selected {
            Arc::new(self.explicit)
        } else {
            Arc::clone(
                self.activators
                    .get(&self.selected)
                    .ok_or_else(|| Error::custom("module_activator_missing", "模块启用源未登记"))?,
            )
        };
        let snapshot = activator.snapshot()?;
        let mut modules = self.modules;
        validate_module_identities(&modules)?;
        let mut selected_names: HashSet<&'static str> = if self.explicit_selected {
            snapshot
                .enabled()
                .iter()
                .map(|name| {
                    find_module(&modules, name)
                        .map(|module| module.name)
                        .ok_or_else(|| Error::custom("module_not_compiled", "启用模块没有编译入口"))
                })
                .collect::<Result<_>>()?
        } else {
            // 文件键与模块仓储查找是不同合同；只查 canonical 名，不把旧孤立键当编译项。
            modules
                .iter()
                .filter(|module| snapshot.is_enabled(module.name))
                .map(|module| module.name)
                .collect()
        };
        let activator: Arc<dyn ModuleActivator> = if self.explicit_selected {
            Arc::new(ExplicitActivator {
                enabled: selected_names
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect(),
            })
        } else {
            activator
        };
        selected_names.retain(|name| {
            !self
                .disabled
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(name))
        });
        let selected_module_indexes: Vec<_> = modules
            .iter()
            .enumerate()
            .filter_map(|(index, module)| selected_names.contains(module.name).then_some(index))
            .collect();
        let mut roots = HashMap::new();
        for index in &selected_module_indexes {
            let module = &mut modules[*index];
            module.root = module
                .root
                .canonicalize()
                .map_err(|_| Error::custom("module_root_invalid", "模块根路径不存在或不可读取"))?;
            if !module.root.is_dir() {
                return Err(Error::custom("module_root_invalid", "模块根路径必须是目录"));
            }
            if let Some(previous) = roots.insert(module.root.clone(), module.name) {
                return Err(Error::custom(
                    "module_root_conflict",
                    format!("模块资源根重复：{previous} 与 {}", module.name),
                ));
            }
        }

        let metadata = modules
            .iter()
            .map(|module| {
                Ok(ModuleMetadata {
                    name: module.name.to_owned(),
                    root: std::path::absolute(&module.root)
                        .map_err(|_| Error::custom("module_root_invalid", "无法锚定模块资源根"))?,
                    priority: module.priority.into(),
                    loaded: selected_names.contains(module.name),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedModules {
            modules: order_selected_modules(modules, &selected_module_indexes)?,
            seed: ModuleRepositorySeed {
                metadata,
                activator,
                application_root: self.application_root,
            },
        })
    }
}

impl ModuleManifest {
    fn validate_application_root(&self) -> Result<()> {
        if self.application_root_error {
            return Err(Error::custom(
                "module_application_root_invalid",
                "无法锚定模块应用根路径",
            ));
        }
        Ok(())
    }
}

fn validate_module_identities(modules: &[ModuleDescriptor]) -> Result<()> {
    let mut identities: HashMap<String, (usize, String)> = HashMap::new();
    for (index, module) in modules.iter().enumerate() {
        if !valid_identity(module.name) {
            return Err(Error::custom(
                "module_name_invalid",
                "模块身份只允许 ASCII 字母、数字、连字符与下划线",
            ));
        }
        let normalized = module.name.to_ascii_lowercase();
        if let Some((previous_index, previous_source)) =
            identities.insert(normalized, (index, safe_source(&module.root)))
        {
            return Err(Error::custom(
                "module_identity_conflict",
                format!(
                    "模块身份 {} 冲突：编译项 {} ({}) 与 {} ({})",
                    module.name,
                    previous_index,
                    previous_source,
                    index,
                    safe_source(&module.root)
                ),
            ));
        }
    }
    Ok(())
}

fn order_selected_modules(
    modules: Vec<ModuleDescriptor>,
    selected_module_indexes: &[usize],
) -> Result<Vec<ModuleDescriptor>> {
    let selected_indexes: HashMap<_, _> = selected_module_indexes
        .iter()
        .enumerate()
        .map(|(index, module_index)| (modules[*module_index].name.to_ascii_lowercase(), index))
        .collect();
    let mut dependencies = vec![Vec::new(); selected_module_indexes.len()];
    for (index, module_index) in selected_module_indexes.iter().enumerate() {
        let module = &modules[*module_index];
        let mut seen = HashSet::new();
        for required in &module.requirements {
            if !valid_dependency_reference(required) {
                return Err(Error::custom(
                    "module_dependency_invalid",
                    format!("模块 {} 声明了无效依赖名称", module.name),
                ));
            }
            let dependency = find_module(&modules, required).ok_or_else(|| {
                Error::custom(
                    "module_dependency_not_compiled",
                    format!("模块 {} 依赖未编译模块 {required}", module.name),
                )
            })?;
            if dependency.name.eq_ignore_ascii_case(module.name) {
                return Err(Error::custom(
                    "module_dependency_self",
                    format!("模块 {} 不能依赖自身", module.name),
                ));
            }
            let normalized = dependency.name.to_ascii_lowercase();
            if !seen.insert(normalized.clone()) {
                return Err(Error::custom(
                    "module_dependency_duplicate",
                    format!("模块 {} 重复声明依赖 {}", module.name, dependency.name),
                ));
            }
            let Some(dependency_index) = selected_indexes.get(&normalized).copied() else {
                return Err(Error::custom(
                    "module_dependency_not_enabled",
                    format!("模块 {} 依赖未启用模块 {}", module.name, dependency.name),
                ));
            };
            dependencies[index].push(dependency_index);
        }
    }

    let mut ordered_indexes = Vec::with_capacity(selected_module_indexes.len());
    let mut emitted = HashSet::new();
    while ordered_indexes.len() < selected_module_indexes.len() {
        let next = (0..selected_module_indexes.len())
            .filter(|index| !emitted.contains(index))
            .filter(|index| dependencies[*index].iter().all(|dep| emitted.contains(dep)))
            .min_by_key(|index| (modules[selected_module_indexes[*index]].priority, *index));
        let Some(next) = next else {
            let cycle = (0..selected_module_indexes.len())
                .filter(|index| !emitted.contains(index))
                .map(|index| modules[selected_module_indexes[index]].name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::custom(
                "module_dependency_cycle",
                format!("模块依赖存在循环或被循环阻塞：{cycle}"),
            ));
        };
        emitted.insert(next);
        ordered_indexes.push(next);
    }

    let mut modules: Vec<_> = modules.into_iter().map(Some).collect();
    ordered_indexes
        .into_iter()
        .map(|index| {
            modules[selected_module_indexes[index]]
                .take()
                .ok_or_else(|| Error::custom("module_order_invalid", "模块启动顺序无效"))
        })
        .collect()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('_')
        && !name.ends_with('_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_identity(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn valid_dependency_reference(name: &str) -> bool {
    let first = name.trim_start_matches(['-', '_', ' ']).as_bytes().first();
    first.is_some_and(u8::is_ascii_alphabetic)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b' '))
}

fn studly(name: &str) -> String {
    let mut next_upper = true;
    let mut result = String::new();
    for ch in name.chars() {
        if ch == '-' || ch == '_' || ch == ' ' {
            next_upper = true;
        } else if next_upper {
            result.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn metadata_index(entries: &[ModuleMetadata], name: &str) -> Option<usize> {
    entries
        .iter()
        .position(|entry| entry.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            let candidate = studly(name);
            entries
                .iter()
                .position(|entry| entry.name.eq_ignore_ascii_case(&candidate))
        })
}

fn find_module<'a>(modules: &'a [ModuleDescriptor], name: &str) -> Option<&'a ModuleDescriptor> {
    modules
        .iter()
        .find(|module| module.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            let candidate = studly(name);
            modules
                .iter()
                .find(|module| module.name.eq_ignore_ascii_case(&candidate))
        })
}

fn safe_source(path: &Path) -> String {
    path.file_name().map_or_else(
        || "unknown".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// 从模块自有 `config/` 目录按固定顺序写入共享配置仓储。
///
/// `config.toml` 先覆盖模块根区段，其他 TOML 文件按相对路径自然排序；
/// 模块 Provider 可在自己 `boot` 的准确时点调用 [`Self::overlay`][Self::overlay]。
pub struct ModuleConfigFiles {
    name: String,
    root: PathBuf,
}

impl ModuleConfigFiles {
    /// 绑定模块资源根；相对路径基于调用时工作目录并立即规范化。
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        if !valid_name(&name) {
            return Err(Error::custom(
                "module_name_invalid",
                "模块名称必须为 snake_case",
            ));
        }
        let root = root.into();
        let root = root
            .canonicalize()
            .map_err(|_| Error::custom("module_root_invalid", "模块根路径不存在或不可读取"))?;
        if !root.is_dir() {
            return Err(Error::custom("module_root_invalid", "模块根路径必须是目录"));
        }
        Ok(Self { name, root })
    }

    /// 解析模块内资源路径；符号链接与越界路径均拒绝。
    pub fn resource(&self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(Error::custom(
                "module_resource_outside",
                "模块资源必须使用相对路径且不能越出模块根目录",
            ));
        }
        let mut requested = self.root.clone();
        for component in path.components() {
            requested.push(component);
            let metadata = requested.symlink_metadata().map_err(|_| {
                Error::custom("module_resource_missing", "模块资源不存在或不可读取")
            })?;
            if metadata.file_type().is_symlink() {
                return Err(Error::custom(
                    "module_resource_outside",
                    "模块资源不能使用符号链接",
                ));
            }
        }
        let resolved = requested
            .canonicalize()
            .map_err(|_| Error::custom("module_resource_missing", "模块资源不存在或不可读取"))?;
        if !resolved.starts_with(&self.root) {
            return Err(Error::custom(
                "module_resource_outside",
                "模块资源越出模块根目录",
            ));
        }
        Ok(resolved)
    }

    /// 在调用此方法的 Provider boot 时点逐文件覆盖配置。
    pub fn overlay(&self, app: &mut BootContext<'_>) -> Result<()> {
        if app.config_repository().is_cached() {
            return Ok(());
        }
        let config_dir = self.root.join("config");
        let config_metadata = match config_dir.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => {
                return Err(Error::custom(
                    "module_config_unreadable",
                    "模块配置目录无法读取",
                ));
            }
        };
        if config_metadata.file_type().is_symlink() {
            return Err(Error::custom(
                "module_config_outside",
                "模块配置目录不允许符号链接",
            ));
        }
        if !config_metadata.is_dir() {
            return Err(Error::custom(
                "module_config_invalid",
                "模块配置路径不是目录",
            ));
        }
        let mut files = Vec::new();
        Self::collect_files(&config_dir, &mut files)?;
        files.sort_by(|left, right| {
            let left_key = left.strip_prefix(&config_dir).unwrap_or(left);
            let right_key = right.strip_prefix(&config_dir).unwrap_or(right);
            crate::config::natural_name_cmp(
                &left_key.to_string_lossy(),
                &right_key.to_string_lossy(),
            )
        });
        if let Some(index) = files
            .iter()
            .position(|path| path == &config_dir.join("config.toml"))
        {
            let first = files.remove(index);
            files.insert(0, first);
        }
        for file in files {
            let section = self.section_for(&config_dir, &file)?;
            let content = std::fs::read_to_string(&file)
                .map_err(|_| Error::custom("module_config_unreadable", "模块配置文件无法读取"))?;
            let parsed: toml::Value = toml::from_str(&content)
                .map_err(|_| Error::custom("module_config_invalid", "模块配置 TOML 格式错误"))?;
            let value = serde_json::to_value(parsed)
                .map_err(|_| Error::custom("module_config_invalid", "模块配置无法转换为区段值"))?;
            app.overlay(&section, value)?;
        }
        Ok(())
    }

    fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let entries = std::fs::read_dir(directory)
            .map_err(|_| Error::custom("module_config_unreadable", "模块配置目录无法读取"))?;
        for entry in entries {
            let entry = entry
                .map_err(|_| Error::custom("module_config_unreadable", "模块配置目录项无法读取"))?;
            let path = entry.path();
            let metadata = path
                .symlink_metadata()
                .map_err(|_| Error::custom("module_config_unreadable", "模块配置目录项无法读取"))?;
            if metadata.file_type().is_symlink() {
                return Err(Error::custom(
                    "module_config_outside",
                    "模块配置目录不允许符号链接",
                ));
            }
            if metadata.is_dir() {
                Self::collect_files(&path, files)?;
            } else if metadata.is_file() && path.extension().is_some_and(|ext| ext == "toml") {
                files.push(path);
            }
        }
        Ok(())
    }

    fn section_for(&self, config_dir: &Path, file: &Path) -> Result<String> {
        let relative = file
            .strip_prefix(config_dir)
            .map_err(|_| Error::custom("module_config_outside", "模块配置文件越出配置目录"))?;
        let mut parts = relative
            .with_extension("")
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if parts.len() == 1 && parts[0] == "config" {
            return Ok(self.name.clone());
        }
        while parts.first().is_some_and(|part| part == &self.name) {
            parts.remove(0);
        }
        if parts.is_empty() || parts.iter().any(|part| !valid_name(part)) {
            return Err(Error::custom(
                "module_config_name_invalid",
                "模块配置文件名称必须为 snake_case",
            ));
        }
        Ok(format!("{}.{}", self.name, parts.join(".")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cached_module_configuration_skips_invalid_toml_before_discovery() -> Result<()> {
        struct ConfigModuleProvider(ModuleConfigFiles);
        impl ServiceProvider for ConfigModuleProvider {
            fn boot<'a>(
                &'a self,
                app: &'a mut BootContext<'_>,
            ) -> super::super::BoxFuture<'a, Result<()>> {
                Box::pin(async move { self.0.overlay(app) })
            }
        }

        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let root = directory(temporary.path(), "example")?;
        let config = directory(&root, "config")?;
        std::fs::write(config.join("greeting.toml"), "invalid [").map_err(Error::internal)?;
        let files = ModuleConfigFiles::new("example", &root)?;
        let mut value = serde_json::to_value(crate::config::FrameworkConfig::default())
            .map_err(Error::internal)?;
        value["example"] = serde_json::json!({ "greeting": { "text": "cached" } });
        let cached = crate::config::ConfigSources::from_cached(value)?;
        let mut builder = super::super::ProviderBuilder::new();
        builder.provider(ConfigModuleProvider(files));
        let runtime = builder.start(cached).await?;
        assert_eq!(
            runtime.config.get("example.greeting.text")?,
            Some(serde_json::json!("cached"))
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        Ok(())
    }

    fn directory(root: &Path, name: &str) -> Result<PathBuf> {
        let path = root.join(name);
        std::fs::create_dir(&path).map_err(Error::internal)?;
        Ok(path)
    }

    #[test]
    fn exact_identity_wins_before_studly_fallback() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let studly_root = directory(temporary.path(), "blog_post")?;
        let literal_root = directory(temporary.path(), "blog_post_literal")?;
        let manifest = ModuleManifest::new()
            .module(ModuleDescriptor::new("BlogPost", studly_root, 0))
            .module(ModuleDescriptor::new("blog-post", literal_root, 0));
        assert_eq!(
            manifest.find("BLOG-POST").map(ModuleDescriptor::name),
            Some("blog-post")
        );
        assert_eq!(
            manifest.find("blog_post").map(ModuleDescriptor::name),
            Some("BlogPost")
        );
        let selected = manifest.enable("blog_post").prepare()?;
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name(), "BlogPost");
        Ok(())
    }

    #[test]
    fn disabled_compiled_module_does_not_read_missing_resource_root() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let missing = temporary.path().join("not-created");
        assert!(
            ModuleManifest::new()
                .module(ModuleDescriptor::new("Disabled", &missing, 0))
                .prepare()?
                .is_empty()
        );
        assert_eq!(
            ModuleManifest::new()
                .module(ModuleDescriptor::new("Disabled", missing, 0))
                .enable("Disabled")
                .prepare()
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("module_root_invalid")
        );
        Ok(())
    }

    #[test]
    fn activation_file_only_accepts_boolean_true_and_invalid_data_is_not_rewritten() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let root = directory(temporary.path(), "blog")?;
        let state = temporary.path().join("modules_statuses.json");
        let make_manifest = || -> Result<ModuleManifest> {
            let mut manifest =
                ModuleManifest::new().module(ModuleDescriptor::new("Blog", root.clone(), 0));
            manifest.register_activator("file", FileModuleActivator::new(&state))?;
            manifest.use_activator("file");
            Ok(manifest)
        };
        assert!(make_manifest()?.prepare()?.is_empty());
        for value in ["", "[]", "{\"Blog\":\"true\"}", "{\"Blog\":false}"] {
            std::fs::write(&state, value).map_err(Error::internal)?;
            assert!(make_manifest()?.prepare()?.is_empty());
        }
        std::fs::write(&state, "{\"Blog\":true}").map_err(Error::internal)?;
        assert_eq!(make_manifest()?.prepare()?.len(), 1);
        for value in ["[true]", "null", "{broken"] {
            std::fs::write(&state, value).map_err(Error::internal)?;
            assert_eq!(
                make_manifest()?
                    .prepare()
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("module_state_invalid")
            );
            assert_eq!(
                std::fs::read_to_string(&state).map_err(Error::internal)?,
                value
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn activation_file_is_snapshotted_per_application_start() -> Result<()> {
        struct MarkerProvider;
        impl ServiceProvider for MarkerProvider {
            fn register<'a>(
                &'a self,
                app: &'a mut super::super::RegisterContext<'_>,
            ) -> crate::core::BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.instance(Arc::new(42_u32)).await?;
                    Ok(())
                })
            }
        }

        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let root = directory(temporary.path(), "blog")?;
        let state = temporary.path().join("modules_statuses.json");
        std::fs::write(&state, r#"{"Blog":true}"#).map_err(Error::internal)?;
        let start =
            || {
                let mut builder = super::super::ProviderBuilder::new();
                builder.module_manifest(ModuleManifest::from_file(&state).module(
                    ModuleDescriptor::new("Blog", root.clone(), 0).provider(MarkerProvider),
                ));
                builder
            };
        let first = start().start(figment::Figment::new()).await?;
        assert_eq!(*first.container().resolve::<u32>().await?, 42);
        std::fs::write(&state, r#"{"Blog":false}"#).map_err(Error::internal)?;
        let second = start().start(figment::Figment::new()).await?;
        assert!(!second.container().contains::<u32>());
        assert_eq!(*first.container().resolve::<u32>().await?, 42);
        first
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await?;
        second
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await
    }

    #[test]
    fn source_identity_conflicts_report_both_safe_origins() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let first = directory(temporary.path(), "first")?;
        let second = directory(temporary.path(), "second")?;
        let error = ModuleManifest::new()
            .module(ModuleDescriptor::new("Blog", first, 0))
            .module(ModuleDescriptor::new("blog", second, 0))
            .prepare()
            .err()
            .ok_or_else(|| Error::custom("expected_conflict", "重复模块身份未被拒绝"))?;
        assert_eq!(error.code(), "module_identity_conflict");
        let detail = error.to_string();
        assert!(detail.contains("first") && detail.contains("second"));
        assert!(!detail.contains(&temporary.path().display().to_string()));
        Ok(())
    }

    #[test]
    fn dependencies_override_priority_and_keep_ready_modules_stable() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let consumer = directory(temporary.path(), "consumer")?;
        let producer = directory(temporary.path(), "producer")?;
        let alpha = directory(temporary.path(), "alpha")?;
        let beta = directory(temporary.path(), "beta")?;
        let modules = ModuleManifest::new()
            .module(ModuleDescriptor::new("consumer", consumer, -100).requires("producer"))
            .module(ModuleDescriptor::new("producer", producer, 100))
            .module(ModuleDescriptor::new("alpha", alpha, 0))
            .module(ModuleDescriptor::new("beta", beta, 0))
            .enable("consumer")
            .enable("producer")
            .enable("alpha")
            .enable("beta")
            .prepare()?;
        let names = modules
            .iter()
            .map(ModuleDescriptor::name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["alpha", "beta", "producer", "consumer"]);
        Ok(())
    }

    #[test]
    fn dependency_names_follow_existing_studly_lookup() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let consumer = directory(temporary.path(), "consumer")?;
        let producer = directory(temporary.path(), "producer")?;
        let modules = ModuleManifest::new()
            .module(ModuleDescriptor::new("consumer", consumer, 0).requires("foo bar"))
            .module(ModuleDescriptor::new("FooBar", producer, 0))
            .enable("consumer")
            .enable("FooBar")
            .prepare()?;
        assert_eq!(
            modules
                .iter()
                .map(ModuleDescriptor::name)
                .collect::<Vec<_>>(),
            ["FooBar", "consumer"]
        );
        Ok(())
    }

    #[test]
    fn dependencies_reject_invalid_missing_disabled_duplicate_self_and_cycles() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let root = |name| directory(temporary.path(), name);
        let error_code = |result: Result<Vec<ModuleDescriptor>>| {
            result.err().map(|error| error.code().to_owned())
        };

        let invalid = ModuleManifest::new()
            .module(ModuleDescriptor::new("invalid", root("invalid")?, 0).requires("bad/name"))
            .enable("invalid");
        assert_eq!(
            error_code(invalid.prepare()).as_deref(),
            Some("module_dependency_invalid")
        );

        let missing = ModuleManifest::new()
            .module(
                ModuleDescriptor::new("missing-user", root("missing-user")?, 0).requires("absent"),
            )
            .enable("missing-user");
        assert_eq!(
            error_code(missing.prepare()).as_deref(),
            Some("module_dependency_not_compiled")
        );

        let disabled = ModuleManifest::new()
            .module(
                ModuleDescriptor::new("disabled-user", root("disabled-user")?, 0)
                    .requires("disabled"),
            )
            .module(ModuleDescriptor::new("disabled", root("disabled")?, 0))
            .enable("disabled-user");
        assert_eq!(
            error_code(disabled.prepare()).as_deref(),
            Some("module_dependency_not_enabled")
        );

        let duplicate = ModuleManifest::new()
            .module(
                ModuleDescriptor::new("duplicate-user", root("duplicate-user")?, 0)
                    .requires("base")
                    .requires("BASE"),
            )
            .module(ModuleDescriptor::new("base", root("base")?, 0))
            .enable("duplicate-user")
            .enable("base");
        assert_eq!(
            error_code(duplicate.prepare()).as_deref(),
            Some("module_dependency_duplicate")
        );

        let self_dependency = ModuleManifest::new()
            .module(ModuleDescriptor::new("self", root("self")?, 0).requires("SELF"))
            .enable("self");
        assert_eq!(
            error_code(self_dependency.prepare()).as_deref(),
            Some("module_dependency_self")
        );

        let cycle = ModuleManifest::new()
            .module(ModuleDescriptor::new("cycle-a", root("cycle-a")?, 0).requires("cycle-b"))
            .module(ModuleDescriptor::new("cycle-b", root("cycle-b")?, 0).requires("cycle-a"))
            .enable("cycle-a")
            .enable("cycle-b");
        assert_eq!(
            error_code(cycle.prepare()).as_deref(),
            Some("module_dependency_cycle")
        );
        Ok(())
    }

    #[test]
    fn nested_repeated_namespace_is_removed_and_resource_ancestors_reject_symlinks() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let root = directory(temporary.path(), "blog")?;
        let config_dir = directory(&root, "config")?;
        let nested = directory(&config_dir, "blog")?;
        let repeated = directory(&nested, "blog")?;
        let file = repeated.join("settings.toml");
        std::fs::write(&file, "value = 1").map_err(Error::internal)?;
        let files = ModuleConfigFiles::new("blog", &root)?;
        assert_eq!(files.section_for(&config_dir, &file)?, "blog.settings");

        #[cfg(unix)]
        {
            let assets = directory(&root, "assets")?;
            std::fs::write(assets.join("index.txt"), "ok").map_err(Error::internal)?;
            std::os::unix::fs::symlink(&assets, root.join("alias")).map_err(Error::internal)?;
            assert_eq!(
                files
                    .resource("alias/index.txt")
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("module_resource_outside")
            );
        }
        Ok(())
    }
}

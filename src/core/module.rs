//! 编译期模块描述与每次应用启动读取的启用快照。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use super::{BootContext, Error, Result, ServiceProvider};

/// 已编译模块的静态描述；只包含应用启动可选择的 Rust 代码。
pub struct ModuleDescriptor {
    pub(crate) name: &'static str,
    pub(crate) root: PathBuf,
    pub(crate) priority: i32,
    pub(crate) providers: Vec<Box<dyn ServiceProvider>>,
}

impl ModuleDescriptor {
    /// 声明模块身份、运行时根路径及升序启动优先级；相对路径基于启动时工作目录。
    pub fn new(name: &'static str, root: impl Into<PathBuf>, priority: i32) -> Self {
        Self {
            name,
            root: root.into(),
            priority,
            providers: Vec::new(),
        }
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

/// 每次应用启动读取一次的模块启用来源。
pub trait ModuleActivator: Send + Sync + 'static {
    /// 返回启用名称；实现必须在错误时保留原始状态，不自行改写。
    fn enabled(&self) -> Result<HashSet<String>>;
}

#[derive(Default)]
struct ExplicitActivator {
    enabled: HashSet<String>,
}

impl ModuleActivator for ExplicitActivator {
    fn enabled(&self) -> Result<HashSet<String>> {
        Ok(self.enabled.clone())
    }
}

/// 只读兼容固定模块包的 JSON 激活状态；状态写入与管理命令属于 R13。
pub struct FileModuleActivator {
    path: PathBuf,
}

impl FileModuleActivator {
    /// 建立文件状态源；每次 App 构建重新读取，不缓存跨 App 状态。
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl ModuleActivator for FileModuleActivator {
    fn enabled(&self) -> Result<HashSet<String>> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
            Err(_) => {
                return Err(Error::custom(
                    "module_state_unreadable",
                    "模块启用状态文件无法读取",
                ));
            }
        };
        if contents.trim().is_empty() {
            return Ok(HashSet::new());
        }
        let state: Value = serde_json::from_str(&contents)
            .map_err(|_| Error::custom("module_state_invalid", "模块启用状态不是有效 JSON"))?;
        let entries = match state {
            Value::Object(entries) => entries,
            Value::Array(entries) if entries.is_empty() => return Ok(HashSet::new()),
            _ => {
                return Err(Error::custom(
                    "module_state_invalid",
                    "模块启用状态必须是对象或空数组",
                ));
            }
        };
        Ok(entries
            .into_iter()
            .filter_map(|(name, state)| (state == Value::Bool(true)).then_some(name))
            .collect())
    }
}

/// 已编译模块目录和独立的运行启用源。
pub struct ModuleManifest {
    modules: Vec<ModuleDescriptor>,
    explicit: ExplicitActivator,
    activators: HashMap<String, Arc<dyn ModuleActivator>>,
    selected: String,
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
        Self {
            modules: Vec::new(),
            explicit: ExplicitActivator::default(),
            activators: HashMap::new(),
            selected: "explicit".into(),
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

    /// 注册应用自定义的只读启用源；同名或保留名称拒绝。
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
        self.selected = name.into();
        self
    }

    /// 按固定模块包的顺序查找：先不区分大小写的精确身份，再尝试 Studly 名称。
    pub fn find(&self, name: &str) -> Option<&ModuleDescriptor> {
        find_module(&self.modules, name)
    }

    pub(crate) fn prepare(self) -> Result<Vec<ModuleDescriptor>> {
        let mut enabled = if self.selected == "explicit" {
            self.explicit.enabled()?
        } else {
            self.activators
                .get(&self.selected)
                .ok_or_else(|| Error::custom("module_activator_missing", "模块启用源未登记"))?
                .enabled()?
        };
        enabled.retain(|name| {
            !self
                .disabled
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(name))
        });
        let mut identities: HashMap<String, (usize, String)> = HashMap::new();
        let modules = self.modules;
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
        let selected_names = enabled
            .iter()
            .map(|name| {
                find_module(&modules, name)
                    .map(|module| module.name)
                    .ok_or_else(|| Error::custom("module_not_compiled", "启用模块没有编译入口"))
            })
            .collect::<Result<HashSet<_>>>()?;
        let mut selected: Vec<_> = modules
            .into_iter()
            .filter(|module| selected_names.contains(module.name))
            .collect();
        let mut roots = HashMap::new();
        for module in &mut selected {
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
        selected.sort_by_key(|module| module.priority);
        Ok(selected)
    }
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
/// `config.toml` 先覆盖模块根区段，其他 TOML 文件按相对路径排序；
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
            left_key.cmp(right_key)
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
            fn register(&self, app: &mut super::super::RegisterContext<'_>) -> Result<()> {
                app.instance(Arc::new(42_u32));
                Ok(())
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

//! 文件翻译Loader；JSON全局文件与分组JSON目录共用Catalog结构，不执行PHP。

use super::{
    Locale,
    loader::{
        MaterializedSession, TranslationLines, TranslationLoadSession, TranslationLoader,
        TranslationResource, invalid_resource, parse_file_lines, validate_lines,
    },
};
use crate::{Error, Result, support::BoxFuture};

mod merge;
use merge::merge_layer;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// 默认文件来源；普通模式不要求数据库、缓存或后台任务。
///
/// 来源为应用维护的受信目录；Unix允许指向普通文件的链接，资源地址取原路径。
/// 子目录链接须在已登记目录内，循环链接会中止捕获。不提供所有祖先组件的原子路径约束。
#[derive(Clone)]
pub struct FileLoader {
    paths: Vec<PathBuf>,
    json_paths: Vec<PathBuf>,
    hints: Vec<(String, PathBuf)>,
    first_path_required: bool,
    max_files: usize,
    max_file_bytes: u64,
}
impl FileLoader {
    /// 读取 `<locale>.json` 全局资源及 `<locale>/<group>.json` 分组资源。
    ///
    /// 全局文件的根必须是对象。分组文件也可使用数组根，其索引以十进制键进入资源映射。
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            paths: vec![path.into()],
            json_paths: Vec::new(),
            hints: Vec::new(),
            first_path_required: true,
            max_files: 4096,
            max_file_bytes: 1024 * 1024,
        }
    }
    /// 选取应用的语言目录。`resources/lang` 为目录时优先使用，否则使用根 `lang`。
    ///
    /// 在构造时选取一次。即使 `resources/lang` 为空也优先使用该目录。
    /// 已选目录尚未创建时，文件来源为空。目录不可读取或资源无效时返回错误。
    pub fn for_application(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        let resources = root.join("resources/lang");
        let path = if resources.is_dir() {
            resources
        } else {
            root.join("lang")
        };
        Self::new(path).with_optional_primary_path()
    }

    /// 允许首个普通语言目录缺失。目录或资源存在但无效时仍返回错误。
    #[must_use]
    pub fn with_optional_primary_path(mut self) -> Self {
        self.first_path_required = false;
        self
    }

    /// 追加普通语言目录。目录缺失时跳过，后登记的目录可覆盖早先的值。
    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.paths.push(path.into());
        self
    }

    /// 追加仅提供全局 JSON 消息的目录。普通语言目录在 JSON 专用路径之后覆盖。
    #[must_use]
    pub fn with_json_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.json_paths.push(path.into());
        self
    }

    /// 追加必需的命名空间提示目录。多个提示目录不得提供同一个资源。
    ///
    /// 普通语言目录中的 `vendor/<namespace>/<locale>/<group>.json`
    /// 会按目录登记顺序覆盖该命名空间分组的部分值。
    #[must_use]
    pub fn with_namespace(
        mut self,
        namespace: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        self.hints.push((namespace.into(), path.into()));
        self
    }
    /// 设置非零文件数量与单文件字节上限。
    pub fn with_limits(mut self, max_files: usize, max_file_bytes: u64) -> Result<Self> {
        if max_files == 0 || max_file_bytes == 0 || max_file_bytes == u64::MAX {
            return Err(invalid_resource());
        }
        self.max_files = max_files;
        self.max_file_bytes = max_file_bytes;
        Ok(self)
    }
    fn materialize(&self) -> Result<MaterializedSession> {
        let mut capture = Capture {
            loader: self,
            files: 0,
        };
        let mut all = Resources::new();
        for path in &self.json_paths {
            if let Some(root) = directory(path, None, true)? {
                merge_layer(&mut all, capture.read_root(&root, None, LayerKind::Global)?);
            }
        }
        let mut roots = Vec::new();
        for (index, path) in self.paths.iter().enumerate() {
            if let Some(root) = directory(path, None, index != 0 || !self.first_path_required)? {
                merge_layer(&mut all, capture.read_root(&root, None, LayerKind::Both)?);
                roots.push(root);
            }
        }

        let mut namespaces = BTreeSet::new();
        for (namespace, path) in &self.hints {
            validate_component(namespace)?;
            let root = required_directory(path, None)?;
            namespaces.insert(namespace.as_str());
            for (resource, lines) in capture.read_root(&root, Some(namespace), LayerKind::Both)? {
                insert_resource(&mut all, resource, lines)?;
            }
        }
        capture.read_overrides(&mut all, &roots, &namespaces)?;
        MaterializedSession::new(all)
    }
}

type Resources = BTreeMap<TranslationResource, TranslationLines>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayerKind {
    Global,
    Group,
    Both,
}

struct Capture<'a> {
    loader: &'a FileLoader,
    files: usize,
}

impl Capture<'_> {
    fn read_root(
        &mut self,
        root: &Path,
        namespace: Option<&str>,
        kind: LayerKind,
    ) -> Result<Resources> {
        let mut resources = Resources::new();
        for (path, is_directory) in entries(root, |name| {
            name != OsStr::new("vendor")
                && match kind {
                    LayerKind::Global => is_json(Path::new(name)),
                    LayerKind::Group => !is_json(Path::new(name)),
                    LayerKind::Both => true,
                }
        })? {
            if is_directory {
                if kind == LayerKind::Global {
                    return Err(source_error());
                }
                let locale = locale_stem(&path, false)?;
                let locale_root = required_directory(&path, Some(root))?;
                let mut groups = Groups {
                    locale: &locale,
                    namespace,
                    root: &locale_root,
                    ancestors: Vec::new(),
                    resources: &mut resources,
                };
                self.read_groups(&mut groups, &locale_root, "")?;
            } else if kind != LayerKind::Group && is_json(&path) {
                self.read_file(
                    &mut resources,
                    TranslationResource {
                        locale: locale_stem(&path, true)?,
                        group: "*".into(),
                        namespace: namespace.map(str::to_owned),
                    },
                    &path,
                )?;
            }
        }
        Ok(resources)
    }

    fn read_overrides(
        &mut self,
        all: &mut Resources,
        roots: &[PathBuf],
        namespaces: &BTreeSet<&str>,
    ) -> Result<()> {
        if namespaces.is_empty() {
            return Ok(());
        }
        for root in roots {
            let Some(vendor) = directory(&root.join("vendor"), Some(root), true)? else {
                continue;
            };
            for &namespace in namespaces {
                if let Some(path) = directory(&vendor.join(namespace), Some(&vendor), true)? {
                    merge_layer(
                        all,
                        self.read_root(&path, Some(namespace), LayerKind::Group)?,
                    );
                }
            }
        }
        Ok(())
    }

    fn read_groups(&mut self, groups: &mut Groups<'_>, path: &Path, prefix: &str) -> Result<()> {
        if groups.ancestors.iter().any(|ancestor| ancestor == path) {
            return Err(source_error());
        }
        groups.ancestors.push(path.to_path_buf());
        for (entry, is_directory) in entries(path, |_| true)? {
            if is_directory {
                let next = group_name(prefix, entry.file_name())?;
                let child = required_directory(&entry, Some(groups.root))?;
                self.read_groups(groups, &child, &next)?;
            } else if is_json(&entry) {
                self.read_file(
                    groups.resources,
                    TranslationResource {
                        locale: groups.locale.clone(),
                        group: group_name(prefix, entry.file_stem())?,
                        namespace: groups.namespace.map(str::to_owned),
                    },
                    &entry,
                )?;
            }
        }
        groups.ancestors.pop();
        Ok(())
    }

    fn read_file(
        &mut self,
        all: &mut Resources,
        resource: TranslationResource,
        path: &Path,
    ) -> Result<()> {
        self.files = self.files.checked_add(1).ok_or_else(limit_error)?;
        if self.files > self.loader.max_files {
            return Err(limit_error());
        }
        let mut bytes = Vec::new();
        open_regular(path)?
            .take(self.loader.max_file_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| source_error())?;
        if u64::try_from(bytes.len()).map_err(|_| limit_error())? > self.loader.max_file_bytes {
            return Err(limit_error());
        }
        let lines = parse_file_lines(&bytes, &resource.group)?;
        // 后续覆盖不能把同一来源原本无效的消息藏起来。
        validate_lines(&resource, &lines)?;
        insert_resource(all, resource, lines)
    }
}

struct Groups<'a> {
    locale: &'a Locale,
    namespace: Option<&'a str>,
    root: &'a Path,
    ancestors: Vec<PathBuf>,
    resources: &'a mut Resources,
}

fn insert_resource(
    target: &mut Resources,
    resource: TranslationResource,
    lines: TranslationLines,
) -> Result<()> {
    if target.insert(resource, lines).is_some() {
        return Err(invalid_resource());
    }
    Ok(())
}

fn group_name(prefix: &str, name: Option<&OsStr>) -> Result<String> {
    let name = name.and_then(OsStr::to_str).ok_or_else(invalid_resource)?;
    validate_component(name)?;
    let group = if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    };
    if group == "*" || group.len() > 255 {
        return Err(invalid_resource());
    }
    Ok(group)
}

fn validate_component(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(part)) if part == name)
        || components.next().is_some()
        || name.len() > 255
        || name.contains(['/', '\\'])
        || name.contains("..")
        || name.chars().any(char::is_control)
    {
        return Err(invalid_resource());
    }
    Ok(())
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

fn required_directory(path: &Path, boundary: Option<&Path>) -> Result<PathBuf> {
    directory(path, boundary, false)?.ok_or_else(source_error)
}

fn directory(path: &Path, boundary: Option<&Path>, optional: bool) -> Result<Option<PathBuf>> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() || metadata.file_type().is_symlink() => {}
        Err(error) if optional && error.kind() == ErrorKind::NotFound => return Ok(None),
        _ => return Err(source_error()),
    }
    let actual = path.canonicalize().map_err(|_| source_error())?;
    if boundary.is_some_and(|root| actual == root || !actual.starts_with(root)) || !actual.is_dir()
    {
        return Err(source_error());
    }
    Ok(Some(actual))
}

impl TranslationLoader for FileLoader {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        let owned = self.clone();
        Box::pin(async move {
            let session = tokio::task::spawn_blocking(move || owned.materialize())
                .await
                .map_err(|_| source_error())??;
            Ok(Arc::new(session) as Arc<dyn TranslationLoadSession>)
        })
    }
}
#[cfg(unix)]
fn open_regular(path: &Path) -> Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags, open};
    // 受信文件链接保留原资源地址，打开已解析的实际目标。
    let resolved = path.canonicalize().map_err(|_| source_error())?;
    // 枚举/解析后的目标若被换为FIFO，NONBLOCK不等待；若被换为链接，NOFOLLOW明确拒绝。
    let descriptor = open(
        &resolved,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| source_error())?;
    let file = std::fs::File::from(descriptor);
    if !file.metadata().map_err(|_| source_error())?.is_file() {
        return Err(source_error());
    }
    Ok(file)
}
#[cfg(not(unix))]
fn open_regular(path: &Path) -> Result<std::fs::File> {
    // 保留既有平台普通文件路径；此平台尚缺非阻塞/拒reparse的原子open适配，不能宣称TOCTOU已闭合。
    let before = std::fs::symlink_metadata(path).map_err(|_| source_error())?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(source_error());
    }
    let file = std::fs::File::open(path).map_err(|_| source_error())?;
    if !file.metadata().map_err(|_| source_error())?.is_file() {
        return Err(source_error());
    }
    Ok(file)
}

fn entries(path: &Path, include: impl Fn(&OsStr) -> bool) -> Result<Vec<(PathBuf, bool)>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|_| source_error())? {
        let entry = entry.map_err(|_| source_error())?;
        if !include(&entry.file_name()) {
            continue;
        }
        let kind = entry.file_type().map_err(|_| source_error())?;
        let kind = if kind.is_symlink() {
            std::fs::metadata(entry.path())
                .map_err(|_| source_error())?
                .file_type()
        } else {
            kind
        };
        if !kind.is_file() && !kind.is_dir() {
            return Err(source_error());
        }
        paths.push((entry.path(), kind.is_dir()));
    }
    paths.sort();
    Ok(paths)
}

fn locale_stem(path: &Path, file: bool) -> Result<Locale> {
    let name = if file {
        path.file_stem()
    } else {
        path.file_name()
    }
    .and_then(|name| name.to_str())
    .ok_or_else(invalid_resource)?;
    Locale::new(name).map_err(|_| invalid_resource())
}
fn source_error() -> Error {
    Error::custom("i18n_file_source_unavailable", "文件翻译来源不可用")
}
fn limit_error() -> Error {
    Error::custom("i18n_file_source_limit", "文件翻译来源超过配置上限")
}

#[cfg(test)]
#[path = "file_loader_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/translation/file_loader_paths.rs"]
mod path_tests;

//! 文件翻译Loader；JSON全局文件与分组JSON目录共用Catalog结构，不执行PHP。

use super::{
    Locale,
    loader::{
        MaterializedSession, TranslationLines, TranslationLoadSession, TranslationLoader,
        TranslationResource, invalid_resource, parse_lines,
    },
};
use crate::{Error, Result, support::BoxFuture};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

/// 默认文件来源；普通模式不要求数据库、缓存或后台任务。
///
/// 来源为应用维护的受信目录；Unix允许指向普通文件的链接，资源地址取原路径。
/// 不提供目录权限隔离或所有祖先组件的原子路径约束；目标变化可明确拒绝本次捕获。
#[derive(Clone)]
pub struct FileTranslationLoader {
    paths: Vec<(Option<String>, PathBuf)>,
    max_files: usize,
    max_file_bytes: u64,
}
impl FileTranslationLoader {
    /// 读取`<locale>.json`全局资源及`<locale>/<group>.json`分组资源。
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            paths: vec![(None, path.into())],
            max_files: 4096,
            max_file_bytes: 1024 * 1024,
        }
    }
    /// 追加显式模块命名空间目录；同命名空间重复资源明确拒绝。
    #[must_use]
    pub fn with_namespace(
        mut self,
        namespace: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        self.paths.push((Some(namespace.into()), path.into()));
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
        let mut all = BTreeMap::new();
        let mut count = 0;
        for (namespace, directory) in &self.paths {
            if namespace.as_ref().is_some_and(|name| {
                name.is_empty() || name.len() > 255 || name.chars().any(char::is_control)
            }) {
                return Err(invalid_resource());
            }
            for path in entries(directory)? {
                if path.is_dir() {
                    let locale = locale_stem(&path, false)?;
                    for file in entries(&path)? {
                        if file
                            .extension()
                            .is_some_and(|extension| extension == "json")
                        {
                            let group = file
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .ok_or_else(invalid_resource)?;
                            self.insert_file(
                                &mut all,
                                &mut count,
                                TranslationResource {
                                    locale: locale.clone(),
                                    group: group.into(),
                                    namespace: namespace.clone(),
                                },
                                &file,
                            )?;
                        }
                    }
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    self.insert_file(
                        &mut all,
                        &mut count,
                        TranslationResource {
                            locale: locale_stem(&path, true)?,
                            group: "*".into(),
                            namespace: namespace.clone(),
                        },
                        &path,
                    )?;
                }
            }
        }
        MaterializedSession::new(all)
    }
    fn insert_file(
        &self,
        all: &mut BTreeMap<TranslationResource, TranslationLines>,
        count: &mut usize,
        resource: TranslationResource,
        path: &Path,
    ) -> Result<()> {
        *count += 1;
        if *count > self.max_files {
            return Err(limit_error());
        }
        let mut bytes = Vec::new();
        open_regular(path)?
            .take(self.max_file_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| source_error())?;
        if u64::try_from(bytes.len()).map_err(|_| limit_error())? > self.max_file_bytes {
            return Err(limit_error());
        }
        let lines = parse_lines(&bytes)?;
        if all.insert(resource, lines).is_some() {
            return Err(invalid_resource());
        }
        Ok(())
    }
}
impl TranslationLoader for FileTranslationLoader {
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
    // Laravel允许受信资源链接；资源地址仍取原路径，解析后的实际目标才用于open。
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

fn entries(path: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = std::fs::read_dir(path)
        .map_err(|_| source_error())?
        .map(|entry| {
            let entry = entry.map_err(|_| source_error())?;
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
            Ok(entry.path())
        })
        .collect::<Result<Vec<_>>>()?;
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

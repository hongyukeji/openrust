#![allow(non_snake_case, reason = "Matches Laravel's translation loader API")]

use std::{
    collections::{BTreeSet, HashMap},
    io,
    path::{Path, PathBuf},
    sync::RwLock,
};

use cap_std::{ambient_authority, fs::Dir};

use crate::contracts::translation::Loader;

pub(super) mod merge;

use self::merge::merge;
use super::{Lines, TranslationError, TranslationFuture, TranslationResult};

/// Loads application and package translations from configured directories.
pub struct FileLoader {
    paths: RwLock<Vec<PathBuf>>,
    json_paths: RwLock<Vec<PathBuf>>,
    namespaces: RwLock<HashMap<String, PathBuf>>,
}

impl Clone for FileLoader {
    fn clone(&self) -> Self {
        Self {
            paths: RwLock::new(self.paths.read().unwrap().clone()),
            json_paths: RwLock::new(self.json_paths.read().unwrap().clone()),
            namespaces: RwLock::new(self.namespaces.read().unwrap().clone()),
        }
    }
}

impl FileLoader {
    pub fn new(path: PathBuf) -> Self {
        Self {
            paths: RwLock::new(vec![path]),
            json_paths: RwLock::default(),
            namespaces: RwLock::default(),
        }
    }

    pub fn addPath(&self, path: PathBuf) {
        self.paths.write().unwrap().push(path);
    }

    /// Add an anonymous JSON translation directory before the application roots.
    pub fn addJsonPath(&self, path: PathBuf) {
        self.json_paths.write().unwrap().push(path);
    }

    pub fn addNamespace(&mut self, namespace: impl Into<String>, path: PathBuf) {
        self.add_namespace(namespace.into(), path);
    }

    /// 发现应用资源中的语言，按规范标签排序并去重；不枚举包命名空间。
    ///
    /// 识别 `<语言>.json` 和包含分组 JSON 文件的语言目录；名称须与规范
    /// BCP 47 标签完全一致。忽略空目录、隐藏项及符号链接，不预加载翻译内容。
    /// 缺失的根目录视为空来源，其他读取错误保留；结果由调用方决定何时刷新。
    pub fn availableLocales(&self) -> TranslationResult<Vec<String>> {
        let paths = self.paths.read().unwrap().clone();
        let json_paths = self.json_paths.read().unwrap().clone();
        let mut locales = BTreeSet::new();
        for (root, grouped) in paths
            .iter()
            .map(|path| (path, true))
            .chain(json_paths.iter().map(|path| (path, false)))
        {
            discover_locales(root, grouped, &mut locales)
                .map_err(|source| TranslationError::Source(Box::new(source)))?;
        }
        Ok(locales.into_iter().collect())
    }

    pub(super) fn add_namespace(&self, namespace: String, path: PathBuf) {
        self.namespaces.write().unwrap().insert(namespace, path);
    }

    pub(super) fn load_lines(
        &self,
        locale: &str,
        group: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<Lines> {
        if !valid_segment(locale) {
            return Err(TranslationError::InvalidLocale);
        }
        let paths = self.paths.read().unwrap();
        let mut lines = Lines::new();
        if group == "*" && namespace == Some("*") {
            let json_paths = self.json_paths.read().unwrap().clone();
            for root in json_paths.iter().chain(paths.iter()) {
                lines.extend(read(root, Path::new(&format!("{locale}.json")))?);
            }
            return Ok(lines);
        }
        let namespace = namespace.filter(|namespace| *namespace != "*");
        if !valid_group(group) || namespace.is_some_and(|namespace| !valid_segment(namespace)) {
            return Err(TranslationError::InvalidKey);
        }
        let relative = Path::new(locale).join(format!("{group}.json"));
        if let Some(namespace) = namespace {
            let Some(package) = self.namespaces.read().unwrap().get(namespace).cloned() else {
                return Ok(lines);
            };
            lines = read(&package, &relative)?;
            for root in paths.iter() {
                merge(
                    &mut lines,
                    read(root, &Path::new("vendor").join(namespace).join(&relative))?,
                );
            }
        } else {
            for root in paths.iter() {
                merge(&mut lines, read(root, &relative)?);
            }
        }
        Ok(lines)
    }
}

fn discover_locales(root: &Path, grouped: bool, locales: &mut BTreeSet<String>) -> io::Result<()> {
    let dir = match Dir::open_ambient_dir(root, ambient_authority()) {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in dir.entries()? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name();
        let path = Path::new(&name);
        let candidate = if kind.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            path.file_stem().and_then(|name| name.to_str())
        } else if grouped && kind.is_dir() && name != "vendor" {
            name.to_str()
        } else {
            None
        };
        let Some(locale) = candidate.filter(|candidate| {
            super::canonical_locale(candidate).is_ok_and(|locale| locale == *candidate)
        }) else {
            continue;
        };
        if kind.is_file() || has_groups(dir.open_dir(&name)?)? {
            locales.insert(locale.to_owned());
        }
    }
    Ok(())
}

fn has_groups(dir: Dir) -> io::Result<bool> {
    let mut pending = vec![dir];
    while let Some(dir) = pending.pop() {
        for entry in dir.entries()? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name();
            let path = Path::new(&name);
            if kind.is_file()
                && path.extension().is_some_and(|ext| ext == "json")
                && path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(valid_segment)
            {
                return Ok(true);
            }
            if kind.is_dir() && name.to_str().is_some_and(valid_segment) {
                pending.push(dir.open_dir(&name)?);
            }
        }
    }
    Ok(false)
}

impl Loader for FileLoader {
    fn load<'a>(
        &'a self,
        locale: &'a str,
        group: &'a str,
        namespace: Option<&'a str>,
    ) -> TranslationFuture<'a, TranslationResult<Lines>> {
        Box::pin(async move {
            let loader = self.clone();
            let locale = super::locale::canonical_locale(locale);
            let group = group.to_owned();
            let namespace = namespace.map(str::to_owned);
            tokio::task::spawn_blocking(move || {
                loader.load_lines(&locale?, &group, namespace.as_deref())
            })
            .await
            .map_err(|source| TranslationError::Source(Box::new(source)))?
        })
    }
}

pub(super) fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn valid_group(value: &str) -> bool {
    value.split('/').all(valid_segment)
}

fn read(root: &Path, relative: &Path) -> TranslationResult<Lines> {
    let bytes = match Dir::open_ambient_dir(root, ambient_authority())
        .and_then(|root| root.read(relative))
    {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Lines::new()),
        Err(error) => return Err(TranslationError::Source(Box::new(error))),
    };
    serde_json::from_slice(&bytes).map_err(|source| TranslationError::Source(Box::new(source)))
}

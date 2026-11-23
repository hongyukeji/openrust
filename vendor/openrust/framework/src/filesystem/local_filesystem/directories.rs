use super::{FileError, FileResult, Filesystem, io_error};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Finder 深度条件；多个条件同时成立，空条件匹配全部层级。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryDepth(Vec<(DepthOperator, usize)>);
#[derive(Clone, Debug, PartialEq, Eq)]
enum DepthOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
impl DirectoryDepth {
    pub fn exact(depth: usize) -> Self {
        Self(vec![(DepthOperator::Equal, depth)])
    }
    pub fn all() -> Self {
        Self::default()
    }
    pub fn parse(conditions: impl IntoIterator<Item = impl AsRef<str>>) -> FileResult<Self> {
        let mut result = Vec::new();
        for condition in conditions {
            let text = condition.as_ref().trim();
            let (operator, number) = if let Some(number) = text.strip_prefix(">=") {
                (DepthOperator::GreaterEqual, number)
            } else if let Some(number) = text.strip_prefix("<=") {
                (DepthOperator::LessEqual, number)
            } else if let Some(number) = text.strip_prefix("!=") {
                (DepthOperator::NotEqual, number)
            } else if let Some(number) = text.strip_prefix("==") {
                (DepthOperator::Equal, number)
            } else if let Some(number) = text.strip_prefix('>') {
                (DepthOperator::Greater, number)
            } else if let Some(number) = text.strip_prefix('<') {
                (DepthOperator::Less, number)
            } else if let Some(number) = text.strip_prefix('=') {
                (DepthOperator::Equal, number)
            } else {
                (DepthOperator::Equal, text)
            };
            let depth = number
                .trim()
                .parse()
                .map_err(|_| FileError::InvalidDepth(text.into()))?;
            result.push((operator, depth));
        }
        Ok(Self(result))
    }
    fn matches(&self, depth: usize) -> bool {
        self.0.iter().all(|(operator, value)| match operator {
            DepthOperator::Equal => depth == *value,
            DepthOperator::NotEqual => depth != *value,
            DepthOperator::Less => depth < *value,
            DepthOperator::LessEqual => depth <= *value,
            DepthOperator::Greater => depth > *value,
            DepthOperator::GreaterEqual => depth >= *value,
        })
    }
    fn descends(&self, depth: usize) -> bool {
        self.0.iter().all(|(operator, value)| match operator {
            DepthOperator::Equal | DepthOperator::LessEqual => depth < *value,
            DepthOperator::Less => depth.saturating_add(1) < *value,
            _ => true,
        })
    }
}

/// 本地目录枚举结果，保留路径、相对路径与文件元数据读取入口。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    path: PathBuf,
    relative: PathBuf,
}
impl FileInfo {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn relative_path(&self) -> &Path {
        self.relative.parent().unwrap_or(Path::new(""))
    }
    pub fn relative_pathname(&self) -> &Path {
        &self.relative
    }
    pub fn filename(&self) -> &std::ffi::OsStr {
        self.path.file_name().unwrap_or_default()
    }
    pub fn contents(&self) -> FileResult<Vec<u8>> {
        Filesystem::new().get(&self.path)
    }
    pub fn metadata(&self) -> FileResult<fs::Metadata> {
        fs::metadata(&self.path).map_err(|error| io_error(&self.path, error))
    }
}
impl AsRef<Path> for FileInfo {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Filesystem {
    pub fn files(&self, directory: impl AsRef<Path>) -> FileResult<Vec<FileInfo>> {
        self.files_with_options(directory, false, DirectoryDepth::exact(0))
    }
    pub fn files_with_options(
        &self,
        directory: impl AsRef<Path>,
        hidden: bool,
        depth: DirectoryDepth,
    ) -> FileResult<Vec<FileInfo>> {
        self.entries(directory.as_ref(), hidden, depth, false)
    }
    pub fn all_files(&self, directory: impl AsRef<Path>) -> FileResult<Vec<FileInfo>> {
        self.all_files_with_hidden(directory, false)
    }
    pub fn all_files_with_hidden(
        &self,
        directory: impl AsRef<Path>,
        hidden: bool,
    ) -> FileResult<Vec<FileInfo>> {
        self.files_with_options(directory, hidden, DirectoryDepth::all())
    }
    pub fn directories(&self, directory: impl AsRef<Path>) -> FileResult<Vec<PathBuf>> {
        self.directories_with_depth(directory, DirectoryDepth::exact(0))
    }
    pub fn directories_with_depth(
        &self,
        directory: impl AsRef<Path>,
        depth: DirectoryDepth,
    ) -> FileResult<Vec<PathBuf>> {
        Ok(self
            .entries(directory.as_ref(), false, depth, true)?
            .into_iter()
            .map(|value| value.path)
            .collect())
    }
    pub fn all_directories(&self, directory: impl AsRef<Path>) -> FileResult<Vec<PathBuf>> {
        self.directories_with_depth(directory, DirectoryDepth::all())
    }
    pub fn is_empty_directory(&self, directory: impl AsRef<Path>) -> FileResult<bool> {
        self.is_empty_directory_ignoring_dot_files(directory, false)
    }
    pub fn is_empty_directory_ignoring_dot_files(
        &self,
        directory: impl AsRef<Path>,
        ignore_dot_files: bool,
    ) -> FileResult<bool> {
        let path = directory.as_ref();
        for item in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let item = item.map_err(|error| io_error(path, error))?;
            if !excluded(&item.file_name(), !ignore_dot_files) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn entries(
        &self,
        root: &Path,
        hidden: bool,
        depth: DirectoryDepth,
        directories: bool,
    ) -> FileResult<Vec<FileInfo>> {
        let mut output = Vec::new();
        let mut stack = vec![(root.to_owned(), 0)];
        while let Some((path, level)) = stack.pop() {
            for item in fs::read_dir(&path).map_err(|error| io_error(&path, error))? {
                let item = item.map_err(|error| io_error(&path, error))?;
                if excluded(&item.file_name(), hidden) {
                    continue;
                }
                let path = item.path();
                let metadata = fs::metadata(&path);
                let is_directory = metadata.as_ref().is_ok_and(fs::Metadata::is_dir);
                let is_file = metadata.as_ref().is_ok_and(fs::Metadata::is_file);
                if depth.matches(level)
                    && ((directories && is_directory) || (!directories && is_file))
                {
                    output.push(FileInfo {
                        relative: path.strip_prefix(root).unwrap_or(&path).to_owned(),
                        path: path.clone(),
                    });
                }
                if is_directory
                    && !item
                        .file_type()
                        .map_err(|error| io_error(&path, error))?
                        .is_symlink()
                    && depth.descends(level)
                {
                    stack.push((path, level + 1));
                }
            }
        }
        // Finder 按相对路径字符串排序；Path 的逐组件排序对连字符等名称结果不同。
        output.sort_by(|left, right| left.relative.as_os_str().cmp(right.relative.as_os_str()));
        Ok(output)
    }
    pub fn ensure_directory_exists(&self, path: impl AsRef<Path>) -> FileResult<()> {
        self.ensure_directory_exists_with_options(path, 0o755, true)
    }
    pub fn ensure_directory_exists_with_options(
        &self,
        path: impl AsRef<Path>,
        mode: u32,
        recursive: bool,
    ) -> FileResult<()> {
        let path = path.as_ref();
        if !self.is_directory(path) {
            self.make_directory_with_options(path, mode, recursive, false)?;
        }
        Ok(())
    }
    pub fn make_directory(&self, path: impl AsRef<Path>) -> FileResult<bool> {
        self.make_directory_with_options(path, 0o755, false, false)
    }
    pub fn make_directory_with_options(
        &self,
        path: impl AsRef<Path>,
        mode: u32,
        recursive: bool,
        force: bool,
    ) -> FileResult<bool> {
        let path = path.as_ref();
        let mut builder = fs::DirBuilder::new();
        builder.recursive(recursive);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        match builder.create(path) {
            Ok(()) => Ok(true),
            Err(_) if force => Ok(false),
            Err(error) => Err(io_error(path, error)),
        }
    }
    pub fn move_directory(
        &self,
        source: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> FileResult<bool> {
        self.move_directory_with_overwrite(source, target, false)
    }
    pub fn move_directory_with_overwrite(
        &self,
        source: impl AsRef<Path>,
        target: impl AsRef<Path>,
        overwrite: bool,
    ) -> FileResult<bool> {
        let target = target.as_ref();
        if overwrite && target.is_dir() && !self.delete_directory(target)? {
            return Ok(false);
        }
        Ok(fs::rename(source, target).is_ok())
    }
    pub fn copy_directory(
        &self,
        source: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> FileResult<bool> {
        let source = source.as_ref();
        let target = target.as_ref();
        if !source.is_dir() {
            return Ok(false);
        }
        let source_absolute = fs::canonicalize(source).map_err(|error| io_error(source, error))?;
        let mut existing = target.to_owned();
        while !existing.exists() {
            if !existing.pop() {
                existing = PathBuf::from(".");
                break;
            }
        }
        if fs::canonicalize(&existing).is_ok_and(|path| path.starts_with(&source_absolute)) {
            return Err(io_error(
                target,
                std::io::Error::other("目标目录不能位于源目录内"),
            ));
        }
        self.ensure_directory_exists_with_options(target, 0o777, true)?;
        let mut stack = vec![(
            source.to_owned(),
            target.to_owned(),
            vec![fs::canonicalize(source).map_err(|error| io_error(source, error))?],
        )];
        while let Some((source, target, ancestors)) = stack.pop() {
            for item in fs::read_dir(&source).map_err(|error| io_error(&source, error))? {
                let item = item.map_err(|error| io_error(&source, error))?;
                let path = item.path();
                let destination = target.join(item.file_name());
                if path.is_dir() {
                    let canonical =
                        fs::canonicalize(&path).map_err(|error| io_error(&path, error))?;
                    if ancestors.contains(&canonical) {
                        return Err(io_error(
                            &path,
                            std::io::Error::other("目录复制遇到符号链接循环"),
                        ));
                    }
                    self.ensure_directory_exists_with_options(&destination, 0o777, true)?;
                    let mut next = ancestors.clone();
                    next.push(canonical);
                    stack.push((path, destination, next));
                } else {
                    self.copy(path, destination)?;
                }
            }
        }
        Ok(true)
    }
    pub fn delete_directory(&self, directory: impl AsRef<Path>) -> FileResult<bool> {
        self.delete_directory_preserving_root(directory, false)
    }
    pub fn delete_directory_preserving_root(
        &self,
        directory: impl AsRef<Path>,
        preserve: bool,
    ) -> FileResult<bool> {
        let directory = directory.as_ref();
        if !directory.is_dir() {
            return Ok(false);
        }
        // 不跟随子目录链接；清理目录不能越过链接删除外部数据。
        for item in fs::read_dir(directory).map_err(|error| io_error(directory, error))? {
            let item = item.map_err(|error| io_error(directory, error))?;
            let path = item.path();
            if item
                .file_type()
                .map_err(|error| io_error(&path, error))?
                .is_dir()
            {
                self.delete_directory(&path)?;
            } else {
                self.delete(path);
            }
        }
        if !preserve {
            let _ = fs::remove_dir(directory);
        }
        Ok(true)
    }
    pub fn delete_directories(&self, directory: impl AsRef<Path>) -> FileResult<bool> {
        let paths = self.directories(directory)?;
        let found = !paths.is_empty();
        for path in paths {
            self.delete_directory(path)?;
        }
        Ok(found)
    }
    pub fn clean_directory(&self, directory: impl AsRef<Path>) -> FileResult<bool> {
        self.delete_directory_preserving_root(directory, true)
    }
}
fn excluded(name: &std::ffi::OsStr, hidden: bool) -> bool {
    let text = name.to_string_lossy();
    (!hidden && text.starts_with('.'))
        || matches!(
            text.as_ref(),
            ".git" | ".hg" | ".svn" | ".bzr" | "_darcs" | "CVS" | "_svn"
        )
}

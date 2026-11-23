//! 不受磁盘适配器根目录限制的本地文件操作。
mod directories;
mod links;
mod macros;
mod paths;
mod reading;
pub use directories::{DirectoryDepth, FileInfo};

use crate::contracts::container::BindingResolutionError;
pub use macros::FileMacro;
use std::{
    collections::HashMap,
    fmt,
    fs::{self, File as StdFile, OpenOptions},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::RwLock,
    time::UNIX_EPOCH,
};

pub type FileResult<T> = Result<T, FileError>;

/// chmod 的混合返回值：读取八进制模式，或成功修改模式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileMode {
    Current(String),
    Changed,
}

#[derive(Debug)]
pub enum FileError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Unavailable(BindingResolutionError),
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    InvalidUtf8 {
        path: PathBuf,
    },
    UnknownAlgorithm(String),
    InvalidTime,
    Pattern(String),
    InvalidDepth(String),
    UnknownMacro(String),
    MacroType {
        name: String,
        expected: &'static str,
        actual: &'static str,
    },
}
impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "文件操作失败 [{}]：{source}", path.display()),
            Self::Unavailable(source) => write!(f, "本地文件服务不可用：{source}"),
            Self::Json { path, source } => {
                write!(f, "文件 JSON 解码失败 [{}]：{source}", path.display())
            }
            Self::InvalidUtf8 { path } => {
                write!(f, "文件不是有效的 UTF-8 文本 [{}]", path.display())
            }
            Self::UnknownAlgorithm(name) => write!(f, "未支持的文件摘要算法：{name}"),
            Self::InvalidDepth(value) => write!(f, "无效的目录深度条件：{value}"),
            Self::Pattern(value) => write!(f, "无效的文件匹配模式：{value}"),
            Self::InvalidTime => f.write_str("文件时间超出可表示的范围"),
            Self::UnknownMacro(name) => write!(f, "未注册文件宏：{name}"),
            Self::MacroType {
                name,
                expected,
                actual,
            } => write!(
                f,
                "文件宏 {name} 的类型不匹配：需要 {expected}，实际 {actual}"
            ),
        }
    }
}
impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Unavailable(source) => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}
impl From<BindingResolutionError> for FileError {
    fn from(value: BindingResolutionError) -> Self {
        Self::Unavailable(value)
    }
}
fn io_error(path: &Path, source: io::Error) -> FileError {
    FileError::Io {
        path: path.to_owned(),
        source,
    }
}

/// 应用级本地文件服务；相对路径遵循进程当前目录，不套用 Storage 磁盘根目录。
#[derive(Default)]
pub struct Filesystem {
    macros: RwLock<HashMap<String, FileMacro>>,
}
impl Filesystem {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().exists()
    }
    pub fn missing(&self, path: impl AsRef<Path>) -> bool {
        !self.exists(path)
    }
    pub fn is_file(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().is_file()
    }
    pub fn is_directory(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().is_dir()
    }

    pub fn get(&self, path: impl AsRef<Path>) -> FileResult<Vec<u8>> {
        self.get_with_lock(path, false)
    }
    pub fn shared_get(&self, path: impl AsRef<Path>) -> FileResult<Vec<u8>> {
        self.get_with_lock(path, true)
    }
    pub fn get_with_lock(&self, path: impl AsRef<Path>, lock: bool) -> FileResult<Vec<u8>> {
        let path = path.as_ref();
        let mut file = StdFile::open(path).map_err(|source| io_error(path, source))?;
        if !file
            .metadata()
            .map_err(|source| io_error(path, source))?
            .is_file()
        {
            return Err(io_error(path, io::Error::other("路径不是普通文件")));
        }
        if lock {
            file.lock_shared()
                .map_err(|source| io_error(path, source))?;
        }
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|source| io_error(path, source))?;
        Ok(content)
    }
    pub fn get_text(&self, path: impl AsRef<Path>) -> FileResult<String> {
        let path = path.as_ref();
        String::from_utf8(self.get(path)?).map_err(|_| FileError::InvalidUtf8 {
            path: path.to_owned(),
        })
    }
    pub fn put(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> FileResult<usize> {
        self.put_with_lock(path, content, false)
    }
    pub fn put_with_lock(
        &self,
        path: impl AsRef<Path>,
        content: impl AsRef<[u8]>,
        lock: bool,
    ) -> FileResult<usize> {
        let path = path.as_ref();
        let content = content.as_ref();
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(!lock)
            .open(path)
            .map_err(|source| io_error(path, source))?;
        if lock {
            file.lock().map_err(|source| io_error(path, source))?;
            file.set_len(0).map_err(|source| io_error(path, source))?;
        }
        file.write_all(content)
            .map_err(|source| io_error(path, source))?;
        Ok(content.len())
    }
    pub fn append(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> FileResult<usize> {
        self.append_with_lock(path, content, false)
    }
    pub fn append_with_lock(
        &self,
        path: impl AsRef<Path>,
        content: impl AsRef<[u8]>,
        lock: bool,
    ) -> FileResult<usize> {
        let path = path.as_ref();
        let content = content.as_ref();
        let mut options = OpenOptions::new();
        options.create(true);
        if lock {
            options.write(true).truncate(false);
        } else {
            options.append(true);
        }
        let mut file = options
            .open(path)
            .map_err(|source| io_error(path, source))?;
        if lock {
            file.lock().map_err(|source| io_error(path, source))?;
            file.seek(io::SeekFrom::End(0))
                .map_err(|source| io_error(path, source))?;
        }
        file.write_all(content)
            .map_err(|source| io_error(path, source))?;
        Ok(content.len())
    }
    pub fn prepend(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> FileResult<usize> {
        let path = path.as_ref();
        let mut merged = content.as_ref().to_vec();
        if self.exists(path) {
            merged.extend(self.get(path)?);
        }
        self.put(path, merged)
    }
    pub fn replace(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> FileResult<()> {
        self.replace_with_mode(path, content, None)
    }
    pub fn replace_with_mode(
        &self,
        path: impl AsRef<Path>,
        content: impl AsRef<[u8]>,
        mode: Option<u32>,
    ) -> FileResult<()> {
        let original = path.as_ref();
        let path = fs::canonicalize(original).unwrap_or_else(|_| original.to_owned());
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut builder = tempfile::Builder::new();
        builder.prefix(".openrust-file-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            builder.permissions(fs::Permissions::from_mode(mode.unwrap_or(0o777)));
        }
        let mut temporary = builder
            .tempfile_in(parent)
            .map_err(|source| io_error(&path, source))?;
        #[cfg(not(unix))]
        if let Some(mode) = mode {
            self.set_mode(temporary.path(), mode)?;
        }
        if let Some(mode) = mode {
            self.set_mode(temporary.path(), mode)?;
        }
        temporary
            .write_all(content.as_ref())
            .map_err(|source| io_error(&path, source))?;
        temporary
            .persist(&path)
            .map_err(|error| io_error(&path, error.error))?;
        Ok(())
    }
    pub fn replace_in_file(
        &self,
        search: impl AsRef<[u8]>,
        replacement: impl AsRef<[u8]>,
        path: impl AsRef<Path>,
    ) -> FileResult<()> {
        self.replace_many_in_file(&[(search.as_ref(), replacement.as_ref())], path)
    }
    pub fn replace_many_in_file(
        &self,
        replacements: &[(&[u8], &[u8])],
        path: impl AsRef<Path>,
    ) -> FileResult<()> {
        let path = path.as_ref();
        let mut content = self.get(path)?;
        for &(search, replacement) in replacements {
            if search.is_empty() {
                continue;
            }
            let mut next = Vec::new();
            let mut offset = 0;
            while let Some(index) = content[offset..]
                .windows(search.len())
                .position(|value| value == search)
            {
                let start = offset + index;
                next.extend_from_slice(&content[offset..start]);
                next.extend_from_slice(replacement);
                offset = start + search.len();
            }
            next.extend_from_slice(&content[offset..]);
            content = next;
        }
        self.put(path, content)?;
        Ok(())
    }
    pub fn size(&self, path: impl AsRef<Path>) -> FileResult<u64> {
        let path = path.as_ref();
        fs::metadata(path)
            .map(|value| value.len())
            .map_err(|source| io_error(path, source))
    }
    pub fn last_modified(&self, path: impl AsRef<Path>) -> FileResult<i64> {
        let path = path.as_ref();
        let time = fs::metadata(path)
            .and_then(|value| value.modified())
            .map_err(|source| io_error(path, source))?;
        match time.duration_since(UNIX_EPOCH) {
            Ok(value) => i64::try_from(value.as_secs()).map_err(|_| FileError::InvalidTime),
            Err(error) => {
                let duration = error.duration();
                let magnitude = duration
                    .as_secs()
                    .checked_add(u64::from(duration.subsec_nanos() != 0))
                    .ok_or(FileError::InvalidTime)?;
                i64::try_from(magnitude)
                    .ok()
                    .and_then(i64::checked_neg)
                    .ok_or(FileError::InvalidTime)
            }
        }
    }
    pub fn chmod(&self, path: impl AsRef<Path>, mode: Option<u32>) -> FileResult<FileMode> {
        match mode {
            None | Some(0) => Ok(FileMode::Current(format!("{:04o}", self.mode(path)?))),
            Some(mode) => {
                self.set_mode(path, mode)?;
                Ok(FileMode::Changed)
            }
        }
    }
    pub fn set_mode(&self, path: impl AsRef<Path>, mode: u32) -> FileResult<()> {
        let path = path.as_ref();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
                .map_err(|source| io_error(path, source))
        }
        #[cfg(not(unix))]
        {
            let mut permissions = fs::metadata(path)
                .map_err(|source| io_error(path, source))?
                .permissions();
            permissions.set_readonly(mode & 0o200 == 0);
            fs::set_permissions(path, permissions).map_err(|source| io_error(path, source))
        }
    }
    pub fn mode(&self, path: impl AsRef<Path>) -> FileResult<u32> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|source| io_error(path, source))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            Ok(metadata.permissions().mode() & 0o7777)
        }
        #[cfg(not(unix))]
        {
            Ok(if metadata.is_dir() {
                0o777
            } else if metadata.permissions().readonly() {
                0o444
            } else {
                0o666
            })
        }
    }
    pub fn delete(&self, path: impl AsRef<Path>) -> bool {
        fs::remove_file(path).is_ok()
    }
    pub fn delete_many<P: AsRef<Path>>(&self, paths: impl IntoIterator<Item = P>) -> bool {
        paths
            .into_iter()
            .fold(true, |success, path| self.delete(path) && success)
    }
    pub fn r#move(&self, source: impl AsRef<Path>, target: impl AsRef<Path>) -> FileResult<()> {
        let source = source.as_ref();
        fs::rename(source, target).map_err(|error| io_error(source, error))
    }
    pub fn copy(&self, source: impl AsRef<Path>, target: impl AsRef<Path>) -> FileResult<()> {
        let source = source.as_ref();
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| io_error(source, error))
    }
    pub fn hash(&self, path: impl AsRef<Path>) -> FileResult<String> {
        self.hash_with_algorithm(path, "md5")
    }
    pub fn hash_with_algorithm(
        &self,
        path: impl AsRef<Path>,
        algorithm: &str,
    ) -> FileResult<String> {
        match algorithm.to_ascii_lowercase().as_str() {
            "md5" => hash_reader::<md5::Md5>(path.as_ref()),
            "sha1" => aws_hash(path.as_ref(), &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY),
            "sha256" => aws_hash(path.as_ref(), &aws_lc_rs::digest::SHA256),
            "sha384" => aws_hash(path.as_ref(), &aws_lc_rs::digest::SHA384),
            "sha512" => aws_hash(path.as_ref(), &aws_lc_rs::digest::SHA512),
            "xxh128" => xxh128(path.as_ref()),
            _ => Err(FileError::UnknownAlgorithm(algorithm.to_owned())),
        }
    }
    pub fn has_same_hash(&self, first: impl AsRef<Path>, second: impl AsRef<Path>) -> bool {
        use subtle::ConstantTimeEq as _;
        let (Ok(first), Ok(second)) = (
            self.hash_with_algorithm(first, "xxh128"),
            self.hash_with_algorithm(second, "xxh128"),
        ) else {
            return false;
        };
        bool::from(first.as_bytes().ct_eq(second.as_bytes()))
    }
}
fn chunks(path: &Path, mut update: impl FnMut(&[u8])) -> FileResult<()> {
    let mut file = StdFile::open(path).map_err(|source| io_error(path, source))?;
    let mut buffer = [0; 16 * 1024];
    loop {
        let size = match file.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result.map_err(|source| io_error(path, source))?,
        };
        if size == 0 {
            break;
        }
        update(&buffer[..size]);
    }
    Ok(())
}
fn hexadecimal(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").expect("写入字符串失败");
    }
    text
}
fn hash_reader<D: md5::Digest>(path: &Path) -> FileResult<String> {
    let mut digest = D::new();
    chunks(path, |value| digest.update(value))?;
    Ok(hexadecimal(&digest.finalize()))
}
fn aws_hash(path: &Path, algorithm: &'static aws_lc_rs::digest::Algorithm) -> FileResult<String> {
    let mut digest = aws_lc_rs::digest::Context::new(algorithm);
    chunks(path, |value| digest.update(value))?;
    Ok(hexadecimal(digest.finish().as_ref()))
}
fn xxh128(path: &Path) -> FileResult<String> {
    let mut digest = xxhash_rust::xxh3::Xxh3::new();
    chunks(path, |value| digest.update(value))?;
    Ok(format!("{:032x}", digest.digest128()))
}

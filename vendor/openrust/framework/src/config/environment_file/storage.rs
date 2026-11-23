use super::{EnvironmentFileError as Error, io_error};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub(super) fn normalize(path: &Path) -> Result<PathBuf, Error> {
    let path = std::path::absolute(path).map_err(|e| io_error("路径解析", e))?;
    let name = path.file_name().ok_or(Error::InvalidPath)?;
    let parent = path
        .parent()
        .ok_or(Error::InvalidPath)?
        .canonicalize()
        .map_err(|e| io_error("目录读取", e))?;
    Ok(parent.join(name))
}
fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}
fn regular(path: &Path) -> Result<fs::Metadata, Error> {
    let meta = fs::symlink_metadata(path).map_err(|e| io_error("读取", e))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::InvalidPath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::InvalidPath);
        }
    }
    Ok(meta)
}
pub(super) fn read(path: &Path) -> Result<Zeroizing<String>, Error> {
    let meta = regular(path)?;
    if meta.len() > 8 * 1024 * 1024 {
        return Err(Error::TooLarge);
    }
    let file = options()
        .read(true)
        .open(path)
        .map_err(|e| io_error("打开", e))?;
    if !file.metadata().map_err(|e| io_error("检查", e))?.is_file() {
        return Err(Error::InvalidPath);
    }
    regular(path)?;
    let mut text = Zeroizing::new(String::new());
    file.take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| io_error("读取", e))?;
    if text.len() > 8 * 1024 * 1024 {
        return Err(Error::TooLarge);
    }
    Ok(text)
}
pub(super) struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        // 显式解锁：Unix 子进程在 fork 到 exec 的短暂窗口可能继承文件描述符。
        // 仅关闭父进程描述符可能使已完成的操作仍暂时占锁。
        let _ = self.0.unlock();
    }
}

pub(super) fn lock(path: &Path) -> Result<Lock, Error> {
    let name = path
        .file_name()
        .ok_or(Error::InvalidPath)?
        .to_string_lossy();
    let path = path.with_file_name(format!("{name}.lock"));
    let file = options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| io_error("加锁", e))?;
    regular(&path)?;
    // 锁文件独立于被替换目标，释放时不删除，以免并发写入者锁到不同 inode。
    file.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(e) => io_error("加锁", e),
    })?;
    Ok(Lock(file))
}
pub(super) fn create(path: &Path, text: &str) -> Result<(), Error> {
    // 临时文件先完整落盘，再通过原子不覆盖发布；不会暴露半份新配置。
    let temp = temporary(path, text, None)?;
    temp.persist_noclobber(path)
        .map_err(|e| io_error("创建", e.error))?;
    sync_parent(path)
}
fn temporary(
    path: &Path,
    text: &str,
    permissions: Option<fs::Permissions>,
) -> Result<tempfile::NamedTempFile, Error> {
    let parent = path.parent().ok_or(Error::InvalidPath)?;
    let mut file = tempfile::Builder::new()
        .prefix(".env.edit-")
        .tempfile_in(parent)
        .map_err(|e| io_error("临时文件创建", e))?;
    if let Some(permissions) = permissions {
        file.as_file()
            .set_permissions(permissions)
            .map_err(|e| io_error("权限保留", e))?;
    }
    file.write_all(text.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| io_error("写入", e))?;
    Ok(file)
}
pub(super) fn replace(
    path: &Path,
    original: &str,
    text: &str,
    restrictive: bool,
) -> Result<(), Error> {
    let meta = regular(path)?;
    let permissions = meta.permissions();
    #[cfg(unix)]
    let mut permissions = permissions;
    #[cfg(unix)]
    if restrictive {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = restrictive;
    if meta.permissions().readonly() {
        return Err(Error::Io {
            operation: "写入",
            kind: std::io::ErrorKind::PermissionDenied,
        });
    }
    let file = temporary(path, text, Some(permissions))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let temp_meta = file
            .as_file()
            .metadata()
            .map_err(|e| io_error("所有者检查", e))?;
        if temp_meta.uid() != meta.uid() || temp_meta.gid() != meta.gid() {
            rustix::fs::fchown(
                file.as_file(),
                Some(rustix::fs::Uid::from_raw(meta.uid())),
                Some(rustix::fs::Gid::from_raw(meta.gid())),
            )
            .map_err(|e| io_error("所有者保留", e.into()))?;
        }
    }
    if read(path)?.as_str() != original {
        return Err(Error::Conflict);
    }
    regular(path)?;
    #[cfg(not(windows))]
    file.persist(path).map_err(|e| io_error("替换", e.error))?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
        // ReplaceFileW 独占打开替换源；先关闭文件句柄，保留 TempPath 的失败清理职责。
        let mut temporary = file.into_temp_path();
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        // 两个缓冲区以 NUL 结尾，TempPath 在调用期间保持源路径有效。
        let result = unsafe {
            ReplaceFileW(
                target.as_ptr(),
                source.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(1176 | 1177)) {
                // Win32 这两种错误可能已移动目标；保留完整新文件供人工恢复，不能当作未写入重试。
                temporary.disable_cleanup(true);
                return Err(Error::ReplacementUncertain {
                    recovery: temporary.to_path_buf(),
                });
            }
            return Err(io_error("替换", error));
        }
    }
    sync_parent(path)
}
fn sync_parent(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    File::open(path.parent().ok_or(Error::InvalidPath)?)
        .and_then(|file| file.sync_all())
        .map_err(|_| Error::DurabilityUncertain)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

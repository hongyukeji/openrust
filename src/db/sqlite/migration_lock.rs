//! SQLite 迁移互斥；不占用迁移需要的数据库写事务。

use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::{Error, Result};

const RETRY_INTERVAL: Duration = Duration::from_millis(25);
type MemoryRegistry = Mutex<HashMap<MemoryIdentity, Weak<AsyncMutex<()>>>>;
static MEMORY_LOCKS: OnceLock<MemoryRegistry> = OnceLock::new();

#[cfg(test)]
mod process_tests;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum MemoryIdentity {
    Named(Vec<u8>),
    Pool(usize),
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
    volume: u64,
    index: u64,
}

struct FileLease {
    mutex: File,
    // 只保留 stat 身份快照；不能额外开闭主文件 FD，以免清掉 SQLite 的 POSIX 锁。
    identity: FileIdentity,
    database_path: PathBuf,
    mutex_path: PathBuf,
}

enum Lease {
    File(FileLease),
    Memory {
        _guard: OwnedMutexGuard<()>,
        // Pool identity 使用 options Arc 地址；保留 Arc 防止地址复用。
        _options: Arc<sqlx::sqlite::SqliteConnectOptions>,
    },
}

/// 文件库使用跨进程文件锁；内存库按 SQLite 共享身份使用进程内锁。
pub(in crate::db) struct SqliteMigrationLock {
    lease: Option<Lease>,
    pool: sqlx::SqlitePool,
}

impl SqliteMigrationLock {
    /// 在绝对截止时间前取得互斥，不额外连接 SQLite。
    pub(in crate::db) async fn acquire(
        db: &crate::db::Db,
        url: &str,
        wait: Duration,
    ) -> Result<Self> {
        let deadline = tokio::time::Instant::now() + wait;
        let connection = db.primary_connection()?;
        if connection.get_database_backend() != DbBackend::Sqlite {
            return Err(failure("migration_lock_backend_mismatch"));
        }
        let pool = connection.get_sqlite_connection_pool().clone();
        let rows = tokio::time::timeout_at(
            deadline,
            connection.query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA database_list",
            )),
        )
        .await
        .map_err(|_| failure("migration_lock_timeout"))?
        .map_err(|_| failure("migration_lock_identity_unavailable"))?;
        let mut filename = None;
        for row in rows {
            let name: String = row
                .try_get("", "name")
                .map_err(|_| failure("migration_lock_identity_unavailable"))?;
            if name == "main" {
                filename = Some(
                    row.try_get::<String>("", "file")
                        .map_err(|_| failure("migration_lock_identity_unavailable"))?,
                );
                break;
            }
        }
        let filename = filename.ok_or_else(|| failure("migration_lock_identity_unavailable"))?;
        let lease = if filename.is_empty() {
            let options = pool.connect_options();
            let identity = memory_identity(url, Arc::as_ptr(&options) as usize)?;
            let mutex = memory_mutex(identity)?;
            let guard = tokio::time::timeout_at(deadline, mutex.lock_owned())
                .await
                .map_err(|_| failure("migration_lock_timeout"))?;
            Lease::Memory {
                _guard: guard,
                _options: options,
            }
        } else {
            reject_unsupported_uri(url)?;
            let file = FileLease::open(Path::new(&filename))?;
            loop {
                if tokio::time::Instant::now() >= deadline {
                    return Err(failure("migration_lock_timeout"));
                }
                file.verify()?;
                match file.mutex.try_lock() {
                    Ok(()) => {
                        file.verify()?;
                        break Lease::File(file);
                    }
                    Err(std::fs::TryLockError::WouldBlock) => {
                        tokio::time::sleep_until(
                            deadline.min(tokio::time::Instant::now() + RETRY_INTERVAL),
                        )
                        .await;
                    }
                    Err(std::fs::TryLockError::Error(_)) => {
                        return Err(failure("migration_lock_unavailable"));
                    }
                }
            }
        };
        Ok(Self {
            lease: Some(lease),
            pool,
        })
    }

    /// 正常完成迁移后释放互斥；失败时保留租约，避免虚报已经释放。
    pub(in crate::db) fn release(mut self) -> Result<()> {
        if let Some(Lease::File(file)) = self.lease.as_ref() {
            file.verify()?;
            file.mutex
                .unlock()
                .map_err(|_| failure("migration_lock_release_failed"))?;
        }
        self.lease.take();
        Ok(())
    }
}

impl Drop for SqliteMigrationLock {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        // 异常析构不能把锁生命周期绑定到正在退出的调用方 runtime。
        // 独立线程先关闭主池，待所有活动 SQLite 操作退还后才允许锁析构。
        let holder = Arc::new(Mutex::new(Some(lease)));
        let worker_holder = holder.clone();
        let pool = self.pool.clone();
        let worker = std::thread::Builder::new()
            .name("openrust-sqlite-migration-cleanup".to_owned())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| ())?;
                    runtime.block_on(pool.close());
                    Ok::<_, ()>(())
                }));
                if !matches!(outcome, Ok(Ok(()))) {
                    preserve_lease(worker_holder);
                }
                // 成功路径：最后一份 holder 析构才释放租约。
            });
        if worker.is_err() {
            preserve_lease(holder);
        }
    }
}

fn preserve_lease(holder: Arc<Mutex<Option<Lease>>>) {
    // 无法证明池已经静止时宁可保持互斥至进程退出，不向竞争者开放执行窗口。
    std::mem::forget(holder);
    tracing::error!("SQLite 迁移异常清理失败；互斥租约保留至进程退出");
}

fn failure(code: &'static str) -> Error {
    // 不把 URI、文件名或凭据带入公开错误与异常日志。
    Error::custom(code, "SQLite 迁移互斥无法安全执行")
}

fn memory_mutex(identity: MemoryIdentity) -> Result<Arc<AsyncMutex<()>>> {
    let mut registry = MEMORY_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| failure("migration_lock_registry_unavailable"))?;
    registry.retain(|_, mutex| mutex.strong_count() > 0);
    if let Some(mutex) = registry.get(&identity).and_then(Weak::upgrade) {
        return Ok(mutex);
    }
    let mutex = Arc::new(AsyncMutex::new(()));
    registry.insert(identity, Arc::downgrade(&mutex));
    Ok(mutex)
}

fn reject_unsupported_uri(url: &str) -> Result<()> {
    let (_, query) = sqlite_parts(url)?;
    if url.contains('#')
        || url::form_urlencoded::parse(query.as_bytes())
            .any(|(name, _)| name == "vfs" || name == "immutable" || name == "nolock")
    {
        return Err(failure("migration_lock_unsupported_sqlite_uri"));
    }
    Ok(())
}

fn sqlite_parts(url: &str) -> Result<(&str, &str)> {
    let value = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .ok_or_else(|| failure("migration_lock_identity_unavailable"))?;
    Ok(value.split_once('?').unwrap_or((value, "")))
}

fn memory_identity(url: &str, pool: usize) -> Result<MemoryIdentity> {
    reject_unsupported_uri(url)?;
    let (path, query) = sqlite_parts(url)?;
    let mut shared = false;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if name == "cache" {
            shared = match value.as_ref() {
                "shared" => true,
                "private" => false,
                _ => return Err(failure("migration_lock_unsupported_sqlite_uri")),
            };
        }
    }
    if shared && path != ":memory:" {
        // SQLx 的普通命名内存与原生 file: URI 都以去前缀后的名字交 SQLite。
        // 普通 :memory: 则由 SQLx 自动生成每池唯一名，必须保留 pool 身份。
        let name = decode_name(path.strip_prefix("file:").unwrap_or(path))?;
        if name.starts_with(b"//") {
            return Err(failure("migration_lock_unsupported_sqlite_uri"));
        }
        // file::memory:?cache=shared 是原生 SQLite 的共享匿名内存身份。
        if !name.is_empty() {
            return Ok(MemoryIdentity::Named(name));
        }
    }
    Ok(MemoryIdentity::Pool(pool))
}

fn decode_name(name: &str) -> Result<Vec<u8>> {
    let mut decoded = Vec::with_capacity(name.len());
    let mut bytes = name.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(hex);
            let low = bytes.next().and_then(hex);
            let (Some(high), Some(low)) = (high, low) else {
                return Err(failure("migration_lock_unsupported_sqlite_uri"));
            };
            let value = high * 16 + low;
            if value == 0 {
                return Err(failure("migration_lock_unsupported_sqlite_uri"));
            }
            decoded.push(value);
        } else {
            decoded.push(byte);
        }
    }
    Ok(decoded)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl FileLease {
    fn open(path: &Path) -> Result<Self> {
        let database_path = path
            .canonicalize()
            .map_err(|_| failure("migration_lock_identity_unavailable"))?;
        let identity = database_identity(&database_path)?;
        let physical = identity;
        let mut digest = Sha256::new();
        digest.update(b"OpenRust SQLite migration mutex v1\0");
        digest.update(physical.volume.to_be_bytes());
        digest.update(physical.index.to_be_bytes());
        let filename = format!(".openrust-migration-{:x}.lock", digest.finalize());
        let mutex_path = database_path
            .parent()
            .ok_or_else(|| failure("migration_lock_identity_unavailable"))?
            .join(filename);
        let mutex = open_mutex(&mutex_path)?;
        let lease = Self {
            mutex,
            identity,
            database_path,
            mutex_path,
        };
        lease.verify()?;
        Ok(lease)
    }

    fn verify(&self) -> Result<()> {
        // 此检查不能证明路径仍对应 SQLite 已打开的旧文件；外部替换路径仍为限制。
        if database_identity(&self.database_path)? != self.identity {
            return Err(failure("migration_lock_identity_changed"));
        }
        verify_path(&self.mutex_path, &self.mutex)?;
        reject_hardlinks(&self.mutex)
    }
}

// SQLite 官方 howtocorrupt §2.2：同进程关闭任何数据库 FD 可释放其全部 POSIX 锁。
// §2.6：不同硬链接名字会产生不同 journal/WAL，不能仅以迁移互斥掩盖此上游风险。
// https://www.sqlite.org/howtocorrupt.html
#[cfg(unix)]
fn database_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = path
        .symlink_metadata()
        .map_err(|_| failure("migration_lock_identity_unavailable"))?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(failure("migration_lock_identity_changed"));
    }
    if metadata.nlink() != 1 {
        return Err(failure("migration_lock_hardlinks_unsupported"));
    }
    Ok(FileIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

#[cfg(windows)]
fn database_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::windows::fs::MetadataExt;
    let metadata = path
        .symlink_metadata()
        .map_err(|_| failure("migration_lock_identity_unavailable"))?;
    if !metadata.is_file() || metadata.file_attributes() & 0x0000_0400 != 0 {
        return Err(failure("migration_lock_identity_changed"));
    }
    // Windows 的 LockFileEx 锁属于取得锁的句柄；关闭这个未加锁的独立查询句柄
    // 不会像 Unix POSIX 锁那样撤销同进程其他 SQLite 句柄的锁。
    // https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex
    let file = open_identity_path(path)?;
    reject_hardlinks(&file)?;
    file_identity(&file)
}

#[cfg(not(any(unix, windows)))]
fn database_identity(_: &Path) -> Result<FileIdentity> {
    Err(failure("migration_lock_unsupported_platform"))
}

#[cfg(unix)]
fn open_mutex(path: &Path) -> Result<File> {
    use rustix::fs::{Mode, OFlags};
    rustix::fs::open(
        path,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(|_| failure("migration_lock_sidecar_unavailable"))
}

#[cfg(windows)]
fn open_mutex(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(0x0020_0000) // FILE_FLAG_OPEN_REPARSE_POINT
        .open(path)
        .map_err(|_| failure("migration_lock_sidecar_unavailable"))
}

#[cfg(not(any(unix, windows)))]
fn open_mutex(_: &Path) -> Result<File> {
    Err(failure("migration_lock_unsupported_platform"))
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file
        .metadata()
        .map_err(|_| failure("migration_lock_identity_unavailable"))?;
    Ok(FileIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity> {
    let information = winapi_util::file::information(file)
        .map_err(|_| failure("migration_lock_identity_unavailable"))?;
    Ok(FileIdentity {
        volume: information.volume_serial_number(),
        index: information.file_index(),
    })
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_: &File) -> Result<FileIdentity> {
    Err(failure("migration_lock_unsupported_platform"))
}

fn verify_path(path: &Path, file: &File) -> Result<()> {
    let metadata = path
        .symlink_metadata()
        .map_err(|_| failure("migration_lock_identity_changed"))?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(failure("migration_lock_identity_changed"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x0000_0400 != 0 {
            return Err(failure("migration_lock_identity_changed"));
        }
    }
    #[cfg(unix)]
    let current_identity = {
        use std::os::unix::fs::MetadataExt;
        FileIdentity {
            volume: metadata.dev(),
            index: metadata.ino(),
        }
    };
    #[cfg(not(unix))]
    let current_identity = file_identity(&open_identity_path(path)?)?;
    if current_identity != file_identity(file)? {
        return Err(failure("migration_lock_identity_changed"));
    }
    Ok(())
}

#[cfg(windows)]
fn open_identity_path(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(|_| failure("migration_lock_identity_changed"))
}

#[cfg(not(any(unix, windows)))]
fn open_identity_path(_: &Path) -> Result<File> {
    Err(failure("migration_lock_unsupported_platform"))
}

#[cfg(any(unix, windows))]
fn reject_hardlinks(file: &File) -> Result<()> {
    #[cfg(unix)]
    let links = {
        use std::os::unix::fs::MetadataExt;
        file.metadata()
            .map_err(|_| failure("migration_lock_identity_unavailable"))?
            .nlink()
    };
    #[cfg(windows)]
    let links = winapi_util::file::information(file)
        .map_err(|_| failure("migration_lock_identity_unavailable"))?
        .number_of_links();
    if links != 1 {
        return Err(failure("migration_lock_hardlinks_unsupported"));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn reject_hardlinks(_: &File) -> Result<()> {
    Err(failure("migration_lock_unsupported_platform"))
}

// 清理阶段正常取得的 sidecar 租约；身份不明的保留路径仍禁止析构。
struct CleanupLock(File);

impl Drop for CleanupLock {
    fn drop(&mut self) {
        // dup 或 fork 到 exec 之间仍可能持有同一打开文件描述；只 close 未必释放 flock。
        // 身份不明路径会主动 forget 整个租约，不能在这里释放其保留的互斥。
        let _ = self.0.unlock();
    }
}

/// 仅清理 guard 自有目录；调用方必须先关闭所有借用其 URL 的 App、池和 worker，
/// 且清理期间不得重新创建该数据库或启动新的借用者。此前置条件不等于框架能追踪任意外部连接或阻止不遵守协议的文件系统写入者。
pub(in crate::db) fn cleanup_owned_directory(directory: &Path) -> Result<()> {
    let directory_metadata = directory
        .symlink_metadata()
        .map_err(|_| failure("sqlite_cleanup_directory_unavailable"))?;
    if !directory_metadata.is_dir() || directory_metadata.is_symlink() {
        return Err(failure("sqlite_cleanup_directory_identity_changed"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if directory_metadata.file_attributes() & 0x0000_0400 != 0 {
            return Err(failure("sqlite_cleanup_directory_identity_changed"));
        }
    }
    let snapshot = cleanup_snapshot(directory)?;
    let main_path = directory.join("database.sqlite");
    let main = snapshot
        .iter()
        .find(|(path, _)| path == &main_path)
        .map(|(_, identity)| *identity);
    let mut locks = Vec::new();
    for (path, expected) in &snapshot {
        if !cleanup_sidecar(path) {
            continue;
        }
        let file = open_cleanup_mutex(path)?;
        let verified = verify_cleanup_mutex(path, &file, *expected, main);
        if let Err(error) = verified {
            // 可能是主库别名 FD，Unix close 会撤销同进程 SQLite POSIX 锁。
            // 无法证明安全时只允许保留至进程退出，不让错误路径自动 Drop。
            std::mem::forget(file);
            return Err(error);
        }
        let acquired = file.try_lock();
        if let Err(error) = verify_cleanup_mutex(path, &file, *expected, main) {
            std::mem::forget(file);
            std::mem::forget(locks);
            return Err(error);
        }
        match acquired {
            Ok(()) => locks.push(CleanupLock(file)),
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(failure("sqlite_cleanup_active_migration"));
            }
            Err(std::fs::TryLockError::Error(_)) => {
                return Err(failure("sqlite_cleanup_lock_unavailable"));
            }
        }
    }
    // 所有协议锁都取得后再次比对整个目录；未知或变化条目不得部分删除。
    let current = cleanup_snapshot(directory);
    if !matches!(current, Ok(ref current) if current == &snapshot) {
        std::mem::forget(locks);
        return Err(failure("sqlite_cleanup_identity_changed"));
    }
    // 主文件先消失，FileLease::verify 随即拒绝新迁移；绝不先 unlink 互斥域。
    for (path, _) in snapshot.iter().filter(|(path, _)| !cleanup_sidecar(path)) {
        std::fs::remove_file(path).map_err(|_| failure("sqlite_cleanup_remove_failed"))?;
    }
    for (path, _) in snapshot.iter().filter(|(path, _)| cleanup_sidecar(path)) {
        std::fs::remove_file(path).map_err(|_| failure("sqlite_cleanup_remove_failed"))?;
    }
    // Windows DeleteFile 可能等待最后句柄关闭才完成删除；主库已删除且所有
    // sidecar 删除请求均成功后才关闭句柄，新标准迁移必须先验证主库存在。
    #[cfg(windows)]
    drop(locks);
    let removed =
        std::fs::remove_dir(directory).map_err(|_| failure("sqlite_cleanup_remove_failed"));
    #[cfg(not(windows))]
    drop(locks);
    removed
}

fn cleanup_sidecar(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(digest) = name
        .strip_prefix(".openrust-migration-")
        .and_then(|name| name.strip_suffix(".lock"))
    else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn cleanup_snapshot(directory: &Path) -> Result<Vec<(PathBuf, FileIdentity)>> {
    let mut snapshot = Vec::new();
    for entry in
        std::fs::read_dir(directory).map_err(|_| failure("sqlite_cleanup_directory_unavailable"))?
    {
        let path = entry
            .map_err(|_| failure("sqlite_cleanup_directory_unavailable"))?
            .path();
        if !cleanup_sidecar(&path)
            && !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(
                    "database.sqlite"
                        | "database.sqlite-wal"
                        | "database.sqlite-shm"
                        | "database.sqlite-journal"
                )
            )
        {
            return Err(failure("sqlite_cleanup_unknown_entry"));
        }
        // Unix database_identity 只 stat，绝不打开或关闭主数据库 FD。
        snapshot.push((path.clone(), database_identity(&path)?));
    }
    // 主库排第一，sidecar 锁顺序亦稳定；活动后续锁时前序仍不得删除。
    snapshot.sort_by(|(left, _), (right, _)| {
        (
            left.file_name() != Some(std::ffi::OsStr::new("database.sqlite")),
            left,
        )
            .cmp(&(
                right.file_name() != Some(std::ffi::OsStr::new("database.sqlite")),
                right,
            ))
    });
    Ok(snapshot)
}

fn verify_cleanup_mutex(
    path: &Path,
    file: &File,
    expected: FileIdentity,
    main: Option<FileIdentity>,
) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|_| failure("sqlite_cleanup_identity_changed"))?;
    if !metadata.is_file() {
        return Err(failure("sqlite_cleanup_identity_changed"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x0000_0400 != 0 {
            return Err(failure("sqlite_cleanup_identity_changed"));
        }
    }
    let actual = file_identity(file)?;
    if actual != expected || main == Some(actual) {
        return Err(failure("sqlite_cleanup_identity_changed"));
    }
    reject_hardlinks(file)?;
    verify_path(path, file)?;
    if database_identity(path)? != expected {
        return Err(failure("sqlite_cleanup_identity_changed"));
    }
    Ok(())
}

#[cfg(unix)]
fn open_cleanup_mutex(path: &Path) -> Result<File> {
    use rustix::fs::{Mode, OFlags};
    rustix::fs::open(
        path,
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| failure("sqlite_cleanup_sidecar_unavailable"))
}

#[cfg(windows)]
fn open_cleanup_mutex(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(|_| failure("sqlite_cleanup_sidecar_unavailable"))
}

#[cfg(not(any(unix, windows)))]
fn open_cleanup_mutex(_: &Path) -> Result<File> {
    Err(failure("migration_lock_unsupported_platform"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_memory_decodes_names_but_preserves_plus_and_last_cache_choice() -> Result<()> {
        assert_eq!(
            memory_identity("sqlite:file:na%6de?mode=memory&cache=shared", 1)?,
            memory_identity("sqlite:file:name?cache=shared&mode=memory", 2)?
        );
        assert_eq!(
            memory_identity("sqlite://name?mode=memory&cache=shared", 1)?,
            memory_identity("sqlite:file:name?mode=memory&cache=shared", 2)?
        );
        assert_ne!(
            memory_identity("sqlite:file:a+b?mode=memory&cache=shared", 1)?,
            memory_identity("sqlite:file:a%20b?mode=memory&cache=shared", 2)?
        );
        assert_eq!(
            memory_identity("sqlite:file:name?cache=shared&cache=private", 7)?,
            MemoryIdentity::Pool(7)
        );
        assert_eq!(
            memory_identity("sqlite:file::memory:?cache=shared", 1)?,
            memory_identity("sqlite:file::memory:?cache=shared", 2)?
        );
        assert_ne!(
            memory_identity("sqlite::memory:", 1)?,
            memory_identity("sqlite::memory:", 2)?
        );
        Ok(())
    }

    #[test]
    fn uncertain_vfs_and_nul_names_fail_without_uri_disclosure() {
        for url in [
            "sqlite:file:secret?vfs=private",
            "sqlite:file:secret%00suffix?cache=shared",
        ] {
            let result = memory_identity(url, 1);
            assert!(result.is_err());
            if let Err(error) = result {
                assert!(!format!("{error:?}").contains("secret"));
            }
        }
    }

    #[test]
    fn memory_registry_reuses_live_mutex_without_merging_pools() -> Result<()> {
        let first = memory_mutex(MemoryIdentity::Pool(123))?;
        let same = memory_mutex(MemoryIdentity::Pool(123))?;
        let other = memory_mutex(MemoryIdentity::Pool(124))?;
        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other));
        Ok(())
    }

    fn cleanup_test_sidecar(directory: &Path, digest: char) -> PathBuf {
        directory.join(format!(
            ".openrust-migration-{}.lock",
            digest.to_string().repeat(64)
        ))
    }

    #[test]
    fn owned_cleanup_removes_database_auxiliary_files_and_all_sidecars()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        for filename in [
            "database.sqlite",
            "database.sqlite-wal",
            "database.sqlite-shm",
            "database.sqlite-journal",
        ] {
            // 创建零字节 fixture 后即关句柄；不打开实际 SQLite 主库 FD 参与锁探针。
            std::fs::write(directory.path().join(filename), [])?;
        }
        for digest in ['a', 'b'] {
            std::fs::write(cleanup_test_sidecar(directory.path(), digest), [])?;
        }
        cleanup_owned_directory(directory.path())?;
        assert!(!directory.path().exists());
        Ok(())
    }

    #[test]
    fn active_second_sidecar_keeps_first_sidecar_and_database_intact()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("database.sqlite");
        std::fs::write(&database, b"owned fixture")?;
        let first_path = cleanup_test_sidecar(directory.path(), 'a');
        let second_path = cleanup_test_sidecar(directory.path(), 'b');
        std::fs::write(&first_path, [])?;
        std::fs::write(&second_path, [])?;
        let active = open_cleanup_mutex(&second_path)?;
        active.try_lock()?;
        assert!(cleanup_owned_directory(directory.path()).is_err());
        assert!(database.exists());
        assert!(first_path.exists());
        assert!(second_path.exists());
        assert_eq!(std::fs::metadata(&database)?.len(), 13);
        active.unlock()?;
        drop(active);
        // 前序清理尝试必须释放它正常取得的 sidecar 锁，而没有删除现场。
        cleanup_owned_directory(directory.path())?;
        assert!(!directory.path().exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_lock_releases_before_a_duplicate_descriptor_is_closed()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = cleanup_test_sidecar(directory.path(), 'a');
        std::fs::write(&path, [])?;
        let first = open_cleanup_mutex(&path)?;
        first.try_lock()?;
        // fork 到 exec 的窗口也共享此打开文件描述；CLOEXEC 不使父进程 close 立即解锁。
        let duplicate = first.try_clone()?;
        drop(CleanupLock(first));
        let peer = open_cleanup_mutex(&path)?;
        peer.try_lock()?;
        assert!(duplicate.metadata()?.is_file());
        peer.unlock()?;
        drop(peer);
        drop(duplicate);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_rejects_symlink_and_hardlink_without_opening_main_database()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        for hardlink in [false, true] {
            let directory = tempfile::tempdir()?;
            let database = directory.path().join("database.sqlite");
            std::fs::write(&database, b"owned fixture")?;
            let sidecar = cleanup_test_sidecar(directory.path(), 'a');
            if hardlink {
                std::fs::hard_link(&database, &sidecar)?;
            } else {
                symlink(&database, &sidecar)?;
            }
            assert!(cleanup_owned_directory(directory.path()).is_err());
            // 只 stat，不为了断言读取内容而打开/关闭主库 FD。
            assert!(database.symlink_metadata()?.is_file());
            assert_eq!(database.symlink_metadata()?.len(), 13);
            assert!(sidecar.symlink_metadata().is_ok());
        }
        Ok(())
    }

    #[test]
    fn unknown_entry_or_non_protocol_sidecar_keeps_every_file()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        for unknown in [
            "notes.txt",
            ".openrust-migration-short.lock",
            // 与合法 a... 侧car使用不同摘要，兼容默认大小写不敏感的 macOS 文件系统。
            ".openrust-migration-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB.lock",
        ] {
            let directory = tempfile::tempdir()?;
            let database = directory.path().join("database.sqlite");
            let sidecar = cleanup_test_sidecar(directory.path(), 'a');
            std::fs::write(&database, [])?;
            std::fs::write(&sidecar, [])?;
            std::fs::write(directory.path().join(unknown), [])?;
            assert!(cleanup_owned_directory(directory.path()).is_err());
            assert!(database.exists());
            assert!(sidecar.exists());
            assert!(directory.path().join(unknown).exists());
        }
        Ok(())
    }
}

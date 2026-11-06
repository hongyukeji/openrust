//! 同用户可信本机目录中的测试数据库归属记录；文件锁本身就是存活凭据。

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 16_384;
const RESOURCE_PREFIX: &str = "openrust_resource_";

/// 归属记录对应的服务端类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Backend {
    /// MySQL 服务端。
    Mysql,
    /// PostgreSQL 服务端。
    Postgres,
}

/// 模板和最终目标分别拥有独立记录、锁与随机 token。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResourceKind {
    /// 最终测试数据库。
    Target,
    /// PostgreSQL 迁移模板数据库。
    Template,
}

/// 本地记录的清理阶段；状态本身不证明数据库对象已被创建或可被删除。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OwnershipState {
    /// 尚未确认 CREATE 的意图。
    Intent,
    /// 调用方已确认取得本次对象所有权。
    Owned,
    /// 调用方正在执行删除。
    DropPending,
    /// 远端操作结果不明确，必须另行核验。
    Ambiguous,
    /// 已完成的墓碑，不能转回活动记录。
    Done,
}

/// 可持久化的归属证据；不包含 DSN、密码或完整连接 URL。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnershipRecord {
    pub(super) version: u32,
    pub(super) resource_id: String,
    pub(super) backend: Backend,
    pub(super) server_identity: String,
    pub(super) admin_database: String,
    pub(super) database_name: String,
    pub(super) resource_kind: ResourceKind,
    pub(super) token: String,
    pub(super) state: OwnershipState,
    #[serde(default)]
    pub(super) marker_identity: Option<super::marker::MarkerIdentity>,
    #[serde(default)]
    pub(super) last_error: Option<String>,
}

impl fmt::Debug for OwnershipRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnershipRecord")
            .field("version", &self.version)
            .field("resource_id", &self.resource_id)
            .field("backend", &self.backend)
            .field("server_identity", &self.server_identity)
            .field("admin_database", &self.admin_database)
            .field("database_name", &self.database_name)
            .field("resource_kind", &self.resource_kind)
            .field("token", &"[REDACTED]")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

/// 只管理本地锁与证据；不执行数据库 SQL，也不推断远端所有权。
pub(super) struct OwnershipJournal {
    root: PathBuf,
    identity: Identity,
}

impl fmt::Debug for OwnershipJournal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnershipJournal")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// 独占记录的打开句柄；移动到清理 future 即移交锁，释放 File 时才释放锁。
pub(super) struct LockedOwnership {
    root: PathBuf,
    root_identity: Identity,
    path: PathBuf,
    lock_path: PathBuf,
    file: File,
    record: OwnershipRecord,
    data_identity: Option<Identity>,
}

impl fmt::Debug for LockedOwnership {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LockedOwnership")
            .field("record", &self.record)
            .finish_non_exhaustive()
    }
}

impl OwnershipJournal {
    /// 使用调用方指定的本机目录；新目录私有，既有目录必须已满足私有权限。
    pub(super) fn new(root: &Path) -> Result<Self> {
        if root.as_os_str().is_empty()
            || root
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(failure("test_database_journal_path_invalid"));
        }
        create_private_root(root)?;
        let identity = validate_root(root, None)?;
        Ok(Self {
            root: root.to_owned(),
            identity,
        })
    }

    /// 创建新的 intent 记录并持锁；目标名必须位于完整的 openrust_ 命名空间。
    pub(super) fn create(
        &self,
        backend: Backend,
        server_identity: &str,
        admin_database: &str,
        database_name: &str,
        resource_kind: ResourceKind,
    ) -> Result<LockedOwnership> {
        validate_root(&self.root, Some(self.identity))?;
        validate_server_identity(server_identity)?;
        validate_identifier(admin_database, false)?;
        validate_identifier(database_name, true)?;
        for _ in 0..8 {
            let resource_id = format!("{RESOURCE_PREFIX}{}", random_hex::<16>()?);
            let record = OwnershipRecord {
                version: VERSION,
                resource_id: resource_id.clone(),
                backend,
                server_identity: server_identity.to_owned(),
                admin_database: admin_database.to_owned(),
                database_name: database_name.to_owned(),
                resource_kind,
                token: random_hex::<32>()?,
                state: OwnershipState::Intent,
                marker_identity: None,
                last_error: None,
            };
            let path = self.root.join(format!("{resource_id}.json"));
            if fs::symlink_metadata(&path).is_ok() {
                continue;
            }
            let lock_path = self.root.join(format!("{resource_id}.lock"));
            let mut options = private_options();
            options.create_new(true);
            let file = match options.open(&lock_path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(failure("test_database_journal_create_failed")),
            };
            file.try_lock()
                .map_err(|_| failure("test_database_journal_lock_unavailable"))?;
            let mut locked = LockedOwnership {
                root: self.root.clone(),
                root_identity: self.identity,
                path,
                lock_path,
                file,
                record,
                data_identity: None,
            };
            locked.persist(OwnershipState::Intent)?;
            sync_directory(&self.root)?;
            return Ok(locked);
        }
        Err(failure("test_database_journal_create_failed"))
    }

    /// 只列出合法 JSON 记录；永久锁及未完成临时文件不构成资源归属证据。
    pub(super) fn list_paths(&self) -> Result<Vec<PathBuf>> {
        validate_root(&self.root, Some(self.identity))?;
        let mut paths = Vec::new();
        for entry in
            fs::read_dir(&self.root).map_err(|_| failure("test_database_journal_read_failed"))?
        {
            let path = entry
                .map_err(|_| failure("test_database_journal_read_failed"))?
                .path();
            // 先识别名字；合法临时文件可已被 writer 原子替换，不能据此中断整次扫描。
            let (record, temporary) =
                match path.extension().and_then(|extension| extension.to_str()) {
                    Some("json") => {
                        validate_record_path(&self.root, &path)?;
                        (true, false)
                    }
                    Some("lock")
                        if path
                            .file_stem()
                            .and_then(|name| name.to_str())
                            .is_some_and(valid_resource_id) =>
                    {
                        (false, false)
                    }
                    Some("tmp") if valid_temporary_name(&path) => (false, true),
                    _ => return Err(failure("test_database_journal_path_invalid")),
                };
            if validate_scanned_path(&path, temporary)? && record {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    /// 尝试持有既有永久锁；锁忙返回 None，不能通过重建锁文件接管活动对象。
    pub(super) fn open_try_locked(&self, path: &Path) -> Result<Option<LockedOwnership>> {
        validate_root(&self.root, Some(self.identity))?;
        validate_record_path(&self.root, path)?;
        let lock_path = path.with_extension("lock");
        validate_file_metadata(
            &fs::symlink_metadata(&lock_path)
                .map_err(|_| failure("test_database_journal_open_failed"))?,
        )?;
        let file = private_options()
            .open(&lock_path)
            .map_err(|_| failure("test_database_journal_open_failed"))?;
        validate_open_file(&lock_path, &file)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(_)) => {
                return Err(failure("test_database_journal_lock_unavailable"));
            }
        }
        validate_root(&self.root, Some(self.identity))?;
        validate_open_file(&lock_path, &file)?;
        let (record, data_identity) = read_record_path(path, None)?;
        Ok(Some(LockedOwnership {
            root: self.root.clone(),
            root_identity: self.identity,
            path: path.to_owned(),
            lock_path,
            file,
            record,
            data_identity: Some(data_identity),
        }))
    }
}

impl LockedOwnership {
    /// 返回当前持锁的归属证据；Debug 始终隐藏 token。
    pub(super) fn record(&self) -> &OwnershipRecord {
        &self.record
    }

    fn validate_lock(&self) -> Result<()> {
        validate_root(&self.root, Some(self.root_identity))?;
        validate_open_file(&self.lock_path, &self.file)
    }

    /// 在永久锁下重读 JSON；不接受未由本句柄完成的替换或字段变动。
    pub(super) fn load(&self) -> Result<OwnershipRecord> {
        self.validate_lock()?;
        let (record, _) = read_record_path(&self.path, self.data_identity)?;
        if !same_resource(&record, &self.record)
            || record.state != self.record.state
            || record.last_error != self.record.last_error
        {
            return Err(failure("test_database_journal_identity_changed"));
        }
        Ok(record)
    }

    /// 写完整私有临时文件后原子替换 JSON 并同步目录，永久锁 inode 不变。
    pub(super) fn persist(&mut self, state: OwnershipState) -> Result<()> {
        self.persist_with_writer(state, Write::write_all)
    }

    /// 首次确认本次数据库对象身份；已记录身份不可更换，重复确认不改变清理阶段。
    pub(super) fn confirm_owned(&mut self, identity: super::marker::MarkerIdentity) -> Result<()> {
        self.load()?;
        validate_marker_identity(&self.record, &identity)?;
        if let Some(previous) = &self.record.marker_identity {
            return if previous == &identity {
                Ok(())
            } else {
                Err(failure("test_database_journal_marker_changed"))
            };
        }
        if !matches!(
            self.record.state,
            OwnershipState::Intent | OwnershipState::Ambiguous
        ) {
            return Err(failure("test_database_journal_confirmation_invalid"));
        }
        let mut record = self.record.clone();
        record.marker_identity = Some(identity);
        record.state = OwnershipState::Owned;
        self.write_record(record, Write::write_all)
    }

    /// 只持久化调用方固定错误代码；不接受错误原文、DSN 或连接 URL。
    pub(super) fn record_error(&mut self, state: OwnershipState, code: &'static str) -> Result<()> {
        validate_error_code(code)?;
        let mut record = self.record.clone();
        record.state = state;
        record.last_error = Some(code.to_owned());
        self.write_record(record, Write::write_all)
    }

    fn persist_with_writer(
        &mut self,
        state: OwnershipState,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        let mut record = self.record.clone();
        record.state = state;
        self.write_record(record, write)
    }

    fn write_record(
        &mut self,
        record: OwnershipRecord,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        if self.record.state == OwnershipState::Done && record.state != OwnershipState::Done {
            return Err(failure("test_database_journal_done"));
        }
        self.validate_lock()?;
        self.validate_current_data()?;
        validate_record(&record, &self.path)?;
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| failure("test_database_journal_write_failed"))?;
        let temporary = self.root.join(format!(
            "{}.{}.tmp",
            self.record.resource_id,
            random_hex::<16>()?
        ));
        let mut options = private_options();
        options.create_new(true);
        let mut file = options
            .open(&temporary)
            .map_err(|_| failure("test_database_journal_write_failed"))?;
        validate_open_file(&temporary, &file)?;
        write(&mut file, &bytes).map_err(|_| failure("test_database_journal_write_failed"))?;
        file.sync_all()
            .map_err(|_| failure("test_database_journal_write_failed"))?;
        let next_identity = identity(
            &file
                .metadata()
                .map_err(|_| failure("test_database_journal_write_failed"))?,
        );
        self.validate_lock()?;
        self.validate_current_data()?;
        validate_open_file(&temporary, &file)?;
        fs::rename(&temporary, &self.path)
            .map_err(|_| failure("test_database_journal_write_failed"))?;
        // rename 成功后 JSON 已完整替换；目录同步失败不能误称旧文件仍在。
        self.data_identity = Some(next_identity);
        self.record = record;
        self.validate_lock()?;
        validate_open_file(&self.path, &file)?;
        sync_directory(&self.root)
    }

    fn validate_current_data(&self) -> Result<()> {
        if self.data_identity.is_some() {
            self.load().map(|_| ())
        } else {
            match fs::symlink_metadata(&self.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(failure("test_database_journal_identity_changed")),
            }
        }
    }
}

fn read_record_path(
    path: &Path,
    expected: Option<Identity>,
) -> Result<(OwnershipRecord, Identity)> {
    let before =
        fs::symlink_metadata(path).map_err(|_| failure("test_database_journal_read_failed"))?;
    validate_file_metadata(&before)?;
    if expected.is_some_and(|expected| expected != identity(&before)) {
        return Err(failure("test_database_journal_identity_changed"));
    }
    let mut file = File::open(path).map_err(|_| failure("test_database_journal_read_failed"))?;
    validate_open_file(path, &file)?;
    let opened = identity(
        &file
            .metadata()
            .map_err(|_| failure("test_database_journal_read_failed"))?,
    );
    if opened != identity(&before) {
        return Err(failure("test_database_journal_identity_changed"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure("test_database_journal_read_failed"))?;
    if bytes.is_empty() || bytes.len() > 16_384 {
        return Err(failure("test_database_journal_invalid_record"));
    }
    let record = serde_json::from_slice(&bytes)
        .map_err(|_| failure("test_database_journal_invalid_record"))?;
    validate_record(&record, path)?;
    validate_open_file(path, &file)?;
    Ok((record, opened))
}

fn validate_record(record: &OwnershipRecord, path: &Path) -> Result<()> {
    if record.version != VERSION
        || !valid_resource_id(&record.resource_id)
        || path.file_stem().and_then(|name| name.to_str()) != Some(record.resource_id.as_str())
        || record.token.len() != 64
        || !record
            .token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(failure("test_database_journal_invalid_record"));
    }
    validate_identifier(&record.admin_database, false)?;
    validate_identifier(&record.database_name, true)?;
    validate_server_identity(&record.server_identity)?;
    if let Some(identity) = &record.marker_identity {
        validate_marker_identity(record, identity)?;
    }
    if let Some(code) = &record.last_error {
        validate_error_code(code)?;
    }
    Ok(())
}

fn validate_marker_identity(
    record: &OwnershipRecord,
    identity: &super::marker::MarkerIdentity,
) -> Result<()> {
    if identity.backend != record.backend
        || identity.database_name != record.database_name
        || match identity.backend {
            Backend::Postgres => identity.postgres_oid.is_none_or(|oid| oid == 0),
            Backend::Mysql => identity.postgres_oid.is_some(),
        }
    {
        return Err(failure("test_database_journal_marker_invalid"));
    }
    Ok(())
}

fn validate_error_code(code: &str) -> Result<()> {
    if code.is_empty()
        || code.len() > 96
        || !code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(failure("test_database_journal_error_code_invalid"));
    }
    Ok(())
}

fn same_resource(left: &OwnershipRecord, right: &OwnershipRecord) -> bool {
    left.version == right.version
        && left.resource_id == right.resource_id
        && left.backend == right.backend
        && left.server_identity == right.server_identity
        && left.admin_database == right.admin_database
        && left.database_name == right.database_name
        && left.resource_kind == right.resource_kind
        && left.token == right.token
        && left.marker_identity == right.marker_identity
}

fn validate_identifier(name: &str, owned: bool) -> Result<()> {
    // 管理库名仅原样入回执/摘要/比较，不拼入DDL；不能套用生成目标库的命名规则。
    if !owned {
        return if !name.is_empty() && name.len() <= 256 && !name.contains('\0') {
            Ok(())
        } else {
            Err(failure("test_database_journal_identifier_invalid"))
        };
    }
    if name.is_empty()
        || name.len() > 63
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || (owned && (!name.starts_with("openrust_") || name.len() <= "openrust_".len()))
    {
        Err(failure("test_database_journal_identifier_invalid"))
    } else {
        Ok(())
    }
}

fn validate_server_identity(identity: &str) -> Result<()> {
    if identity.is_empty()
        || identity.len() > 256
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_' | b'.'))
    {
        Err(failure("test_database_journal_server_identity_invalid"))
    } else {
        Ok(())
    }
}

fn valid_resource_id(name: &str) -> bool {
    name.strip_prefix(RESOURCE_PREFIX).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn validate_scanned_path(path: &Path, temporary: bool) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if temporary && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(_) => return Err(failure("test_database_journal_read_failed")),
    };
    validate_file_metadata(&metadata)?;
    Ok(true)
}

fn valid_temporary_name(path: &Path) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .and_then(|stem| stem.rsplit_once('.'))
        .is_some_and(|(resource, suffix)| {
            valid_resource_id(resource)
                && suffix.len() == 32
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn validate_record_path(root: &Path, path: &Path) -> Result<()> {
    if path.parent() != Some(root)
        || path.extension().and_then(|extension| extension.to_str()) != Some("json")
        || !path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(valid_resource_id)
    {
        return Err(failure("test_database_journal_path_invalid"));
    }
    Ok(())
}

fn random_hex<const N: usize>() -> Result<String> {
    use std::fmt::Write as _;
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|_| failure("test_database_journal_random_failed"))?;
    let mut encoded = String::with_capacity(N * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| failure("test_database_journal_random_failed"))?;
    }
    Ok(encoded)
}

#[cfg(unix)]
fn private_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.read(true).write(true).mode(0o600);
    options
}
#[cfg(not(unix))]
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    options
}

#[cfg(unix)]
fn create_private_root(root: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut missing = Vec::new();
    let mut cursor = root;
    loop {
        match fs::symlink_metadata(cursor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.to_owned());
            }
            Err(_) => return Err(failure("test_database_journal_directory_failed")),
        }
        cursor = cursor
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
    }
    for directory in missing.iter().rev() {
        match fs::DirBuilder::new().mode(0o700).create(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(failure("test_database_journal_directory_failed")),
        }
        validate_root(directory, None)?;
        // 每层新目录及其父目录项均同步；耐久性仍取决于本机文件系统和硬件的 fsync 语义。
        sync_directory(directory)?;
        let parent = directory
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        sync_directory(parent)?;
    }
    Ok(())
}
#[cfg(not(unix))]
fn create_private_root(_: &Path) -> Result<()> {
    Err(failure("test_database_journal_private_acl_unsupported"))
}

fn validate_root(root: &Path, expected: Option<Identity>) -> Result<Identity> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|_| failure("test_database_journal_directory_failed"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failure("test_database_journal_directory_invalid"));
    }
    private_permissions(&metadata)?;
    let identity = identity(&metadata);
    if expected.is_some_and(|expected| expected != identity) {
        return Err(failure("test_database_journal_identity_changed"));
    }
    Ok(identity)
}

fn validate_file_metadata(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(failure("test_database_journal_file_invalid"));
    }
    private_permissions(metadata)
}

fn validate_open_file(path: &Path, file: &File) -> Result<()> {
    let opened = file
        .metadata()
        .map_err(|_| failure("test_database_journal_read_failed"))?;
    let current =
        fs::symlink_metadata(path).map_err(|_| failure("test_database_journal_read_failed"))?;
    validate_file_metadata(&opened)?;
    validate_file_metadata(&current)?;
    if identity(&opened) != identity(&current) {
        return Err(failure("test_database_journal_identity_changed"));
    }
    Ok(())
}

#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}
#[cfg(not(unix))]
fn identity(_: &fs::Metadata) -> Identity {
    // 非 Unix 在目录／文件权限校验阶段始终拒绝，不能借此占位身份接管记录。
    Identity {
        device: 0,
        inode: 0,
    }
}

#[cfg(unix)]
fn private_permissions(metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    validate_private_owner_mode(
        metadata.uid(),
        metadata.permissions().mode(),
        rustix::process::geteuid().as_raw(),
    )
}

#[cfg(unix)]
fn validate_private_owner_mode(owner: u32, mode: u32, current_uid: u32) -> Result<()> {
    if owner != current_uid {
        return Err(failure("test_database_journal_owner_invalid"));
    }
    if mode & 0o077 != 0 {
        Err(failure("test_database_journal_permissions_invalid"))
    } else {
        Ok(())
    }
}
#[cfg(not(unix))]
fn private_permissions(_: &fs::Metadata) -> Result<()> {
    Err(failure("test_database_journal_private_acl_unsupported"))
}

fn sync_directory(root: &Path) -> Result<()> {
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| failure("test_database_journal_write_failed"))
}

fn failure(code: &'static str) -> Error {
    Error::custom(code, "测试数据库归属记录无法安全读取或持久化")
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;

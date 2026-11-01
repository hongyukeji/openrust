//! 模块启用快照与文件状态的持久管理；文件提交不改变当前 App 的已加载资源。

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::core::{Error, Result};

/// 同一次读取的模块状态；未知键和值保留，调试输出仅显示条目数。
#[derive(Clone, Default)]
pub struct ModuleActivationSnapshot {
    entries: Map<String, Value>,
}

impl ModuleActivationSnapshot {
    /// 按来源顺序建立快照；重复键使用后值，不归一化名称大小写。
    pub fn from_entries<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    /// 将只提供启用名称的来源投影为严格布尔状态。
    pub fn from_enabled(enabled: impl IntoIterator<Item = String>) -> Self {
        Self::from_entries(enabled.into_iter().map(|name| (name, Value::Bool(true))))
    }

    /// 返回值为严格布尔 true 的原始名称集合。
    pub fn enabled(&self) -> HashSet<String> {
        self.entries
            .iter()
            .filter(|(_, value)| **value == Value::Bool(true))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// 按精确名称比较状态；缺失和 null 仅匹配 false，其他非布尔值均不匹配。
    pub fn has_status(&self, name: &str, status: bool) -> bool {
        match self.entries.get(name) {
            None | Some(Value::Null) => !status,
            Some(Value::Bool(value)) => *value == status,
            Some(_) => false,
        }
    }

    /// 仅严格布尔 true 表示启用。
    pub fn is_enabled(&self, name: &str) -> bool {
        self.has_status(name, true)
    }

    /// 按模块对象语义返回未启用状态；非布尔值也算未启用。
    pub fn is_disabled(&self, name: &str) -> bool {
        !self.is_enabled(name)
    }

    /// 借用指定精确名称的原始值；调用方输出前负责脱敏。
    pub fn value(&self, name: &str) -> Option<&Value> {
        self.entries.get(name)
    }

    /// 借用完整原始表；未知项不会在持久修改其他模块时丢弃。
    pub fn entries(&self) -> &Map<String, Value> {
        &self.entries
    }
}

impl fmt::Debug for ModuleActivationSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModuleActivationSnapshot")
            .field("entries", &self.entries.len())
            .finish()
    }
}

/// 模块状态驱动；既有只读实现可继续仅提供启用名称。
pub trait ModuleActivator: Send + Sync + 'static {
    /// 返回精确启用名称；失败不能自行恢复或覆写原状态。
    fn enabled(&self) -> Result<HashSet<String>>;

    /// 读取同次状态；默认从既有 enabled 实现投影。
    fn snapshot(&self) -> Result<ModuleActivationSnapshot> {
        self.enabled().map(ModuleActivationSnapshot::from_enabled)
    }

    /// 持久设置精确名称；只读驱动默认明确拒绝。
    fn set_active_by_name(&self, _name: &str, _active: bool) -> Result<()> {
        Err(read_only())
    }

    /// 仅移除指定状态记录；只读驱动默认明确拒绝。
    fn delete(&self, _name: &str) -> Result<()> {
        Err(read_only())
    }

    /// 显式清除驱动状态；不删除模块代码或数据库。
    fn reset(&self) -> Result<()> {
        Err(read_only())
    }

    /// 文件型驱动可返回配置路径；其他驱动默认无路径。
    fn path(&self) -> Option<&Path> {
        None
    }
}

fn read_only() -> Error {
    Error::custom(
        "module_activator_read_only",
        "模块状态驱动为只读，不能持久修改",
    )
}

/// 读取并持久修改模块 JSON 状态；每次读取均捕获文件当前内容。
///
/// 写操作使用协作式 sidecar 文件锁与同目录原子替换；不运行事件或用户回调。
/// 当前 App 已加载的 Provider、路由和资源保持原启动快照。
pub struct FileModuleActivator {
    path: PathBuf,
    captured: bool,
}

impl FileModuleActivator {
    /// 在构造时捕获配置路径的绝对形式；不解析符号链接、不读取状态或建立目录。
    ///
    /// 捕获失败会延后到首次读取或修改时明确报错，不随之后的工作目录变化重试。
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        match std::path::absolute(&path) {
            Ok(path) => Self {
                path,
                captured: true,
            },
            Err(_) => Self {
                path,
                captured: false,
            },
        }
    }

    /// 返回构造时捕获的配置绝对路径；不是 `canonicalize` 后的符号链接目标。
    ///
    /// 捕获失败时仅保留原输入供定位；所有读取或修改均报路径无效，不使用该输入操作。
    pub fn statuses_file_path(&self) -> &Path {
        &self.path
    }

    /// 读取精确名称的当前状态；缺失/null 与未知非布尔值按快照规则区分。
    pub fn has_status(&self, name: &str, status: bool) -> Result<bool> {
        Ok(self.snapshot()?.has_status(name, status))
    }

    /// 原子设置一个精确名称，锁内重新读取并保留其他项。
    pub fn set_active_by_name(&self, name: &str, active: bool) -> Result<()> {
        let path = self.operation_path()?;
        let locked = LockedTarget::acquire(path)?;
        let mut snapshot = read_snapshot(&locked.target)?;
        snapshot
            .entries
            .insert(name.to_owned(), Value::Bool(active));
        locked.write(path, &snapshot.entries)
    }

    /// 删除非 null 的状态记录；缺失/null 不改写文件。
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.operation_path()?;
        if missing(path)? {
            return Ok(());
        }
        let locked = LockedTarget::acquire(path)?;
        let mut snapshot = read_snapshot(&locked.target)?;
        if snapshot.value(name).is_none_or(Value::is_null) {
            return Ok(());
        }
        snapshot.entries.remove(name);
        locked.write(path, &snapshot.entries)
    }

    /// 显式删除状态文件；允许清除损坏 JSON，不删除模块目录。
    ///
    /// 使用符号链接配置时只移除其正规目标，保留链接本身以便后续写入重建状态。
    pub fn reset(&self) -> Result<()> {
        let path = self.operation_path()?;
        if missing(path)? {
            return Ok(());
        }
        let locked = LockedTarget::acquire(path)?;
        locked.verify(path)?;
        match fs::remove_file(&locked.target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(state_error(
                "module_state_write_failed",
                "无法清除模块状态文件",
            )),
        }
    }

    fn operation_path(&self) -> Result<&Path> {
        if self.captured {
            Ok(&self.path)
        } else {
            Err(state_error(
                "module_state_path_invalid",
                "构造模块状态驱动时无法捕获绝对路径",
            ))
        }
    }
}

impl fmt::Debug for FileModuleActivator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FileModuleActivator { .. }")
    }
}

impl ModuleActivator for FileModuleActivator {
    fn enabled(&self) -> Result<HashSet<String>> {
        Ok(self.snapshot()?.enabled())
    }

    fn snapshot(&self) -> Result<ModuleActivationSnapshot> {
        let path = self.operation_path()?;
        if missing(path)? {
            return Ok(ModuleActivationSnapshot::default());
        }
        read_snapshot(&resolve_target(path)?)
    }

    fn set_active_by_name(&self, name: &str, active: bool) -> Result<()> {
        Self::set_active_by_name(self, name, active)
    }

    fn delete(&self, name: &str) -> Result<()> {
        Self::delete(self, name)
    }

    fn reset(&self) -> Result<()> {
        Self::reset(self)
    }

    fn path(&self) -> Option<&Path> {
        Some(self.statuses_file_path())
    }
}

fn state_error(code: &'static str, message: &'static str) -> Error {
    Error::custom(code, message)
}

fn missing(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(state_error(
            "module_state_unreadable",
            "无法检查模块状态路径",
        )),
    }
}

fn read_snapshot(path: &Path) -> Result<ModuleActivationSnapshot> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ModuleActivationSnapshot::default());
        }
        Err(_) => {
            return Err(state_error(
                "module_state_unreadable",
                "模块状态文件无法读取",
            ));
        }
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| state_error("module_state_invalid", "模块状态不是有效 JSON"))?;
    if text.trim().is_empty() {
        return Ok(ModuleActivationSnapshot::default());
    }
    match serde_json::from_str(text)
        .map_err(|_| state_error("module_state_invalid", "模块状态不是有效 JSON"))?
    {
        Value::Object(entries) => Ok(ModuleActivationSnapshot { entries }),
        Value::Array(entries) if entries.is_empty() => Ok(ModuleActivationSnapshot::default()),
        _ => Err(state_error(
            "module_state_invalid",
            "模块状态必须是对象或空数组",
        )),
    }
}

// 最终链接映射到真正目标后再加锁/替换，避免 rename 把配置链接本身换成文件。
fn resolve_target(path: &Path) -> Result<PathBuf> {
    let mut current = path.to_owned();
    for _ in 0..40 {
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let linked = fs::read_link(&current).map_err(|_| {
                    state_error("module_state_path_invalid", "模块状态链接无法解析")
                })?;
                current = if linked.is_absolute() {
                    linked
                } else {
                    current.parent().unwrap_or(Path::new(".")).join(linked)
                };
            }
            Ok(metadata) if metadata.is_file() => {
                return current.canonicalize().map_err(|_| {
                    state_error("module_state_path_invalid", "模块状态目标无法规范化")
                });
            }
            Ok(_) => {
                return Err(state_error(
                    "module_state_path_invalid",
                    "模块状态目标不是普通文件",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = current.file_name().ok_or_else(|| {
                    state_error("module_state_path_invalid", "模块状态路径缺少文件名")
                })?;
                let parent = current
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                let parent = parent.canonicalize().map_err(|_| {
                    state_error(
                        "module_state_path_invalid",
                        "模块状态父目录不存在或不可读取",
                    )
                })?;
                if !parent.is_dir() {
                    return Err(state_error(
                        "module_state_path_invalid",
                        "模块状态父路径不是目录",
                    ));
                }
                return Ok(parent.join(name));
            }
            Err(_) => {
                return Err(state_error(
                    "module_state_unreadable",
                    "模块状态路径无法读取",
                ));
            }
        }
    }
    Err(state_error(
        "module_state_path_invalid",
        "模块状态链接存在循环或层数过多",
    ))
}

fn sidecar_path(target: &Path) -> Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| state_error("module_state_path_invalid", "模块状态路径缺少文件名"))?;
    let mut lock_name = OsString::from(".");
    lock_name.push(name);
    lock_name.push(".lock");
    Ok(target.with_file_name(lock_name))
}

fn verify_lock_file(path: &Path, opened: &File) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| state_error("module_state_lock_failed", "模块状态锁在操作期间消失"))?;
    if !metadata.file_type().is_file() {
        return Err(state_error(
            "module_state_lock_failed",
            "模块状态锁不是普通文件",
        ));
    }
    let opened = opened
        .metadata()
        .map_err(|_| state_error("module_state_lock_failed", "无法读取已打开的模块状态锁"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (metadata.dev(), metadata.ino()) != (opened.dev(), opened.ino()) {
            return Err(state_error(
                "module_state_lock_failed",
                "模块状态锁在操作期间被替换",
            ));
        }
    }
    #[cfg(not(unix))]
    if !opened.is_file() {
        return Err(state_error(
            "module_state_lock_failed",
            "已打开的模块状态锁不是普通文件",
        ));
    }
    Ok(())
}

struct LockedTarget {
    target: PathBuf,
    lock: File,
}

impl Drop for LockedTarget {
    fn drop(&mut self) {
        // fork期间可短暂复制同一文件描述；显式解锁，不能等所有副本关闭才释放。
        // Drop无法返回错误；随后File关闭仍作为释放兜底，不改写已经提交的文件结果。
        let _ = self.lock.unlock();
    }
}

impl LockedTarget {
    fn acquire(configured: &Path) -> Result<Self> {
        let target = resolve_target(configured)?;
        let lock_path = sidecar_path(&target)?;
        match fs::symlink_metadata(&lock_path) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(state_error(
                    "module_state_lock_failed",
                    "模块状态锁不是普通文件",
                ));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(state_error(
                    "module_state_lock_failed",
                    "无法检查模块状态锁",
                ));
            }
            _ => {}
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|_| state_error("module_state_lock_failed", "无法打开模块状态锁"))?;
        verify_lock_file(&lock_path, &lock)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(state_error(
                    "module_state_busy",
                    "模块状态正由其他操作修改，请稍后重试",
                ));
            }
            Err(TryLockError::Error(_)) => {
                return Err(state_error(
                    "module_state_lock_failed",
                    "无法取得模块状态锁",
                ));
            }
        }
        let locked = Self { target, lock };
        verify_lock_file(&lock_path, &locked.lock)?;
        locked.verify(configured)?;
        Ok(locked)
    }

    fn verify(&self, configured: &Path) -> Result<()> {
        if resolve_target(configured)? != self.target {
            return Err(state_error(
                "module_state_path_changed",
                "模块状态目标在操作期间发生变化",
            ));
        }
        Ok(())
    }

    fn write(&self, configured: &Path, entries: &Map<String, Value>) -> Result<()> {
        let permissions = match fs::metadata(&self.target) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => {
                return Err(state_error(
                    "module_state_unreadable",
                    "无法读取模块状态权限",
                ));
            }
        };
        write_atomic(&self.target, entries, permissions, || {
            self.verify(configured)
        })
    }
}

fn write_atomic(
    target: &Path,
    entries: &Map<String, Value>,
    permissions: Option<Permissions>,
    verify: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| state_error("module_state_path_invalid", "模块状态路径缺少父目录"))?;
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|_| state_error("module_state_invalid", "模块状态无法序列化"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".module-state-")
        .tempfile_in(parent)
        .map_err(|_| state_error("module_state_write_failed", "无法建立模块状态临时文件"))?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(|_| state_error("module_state_write_failed", "无法保留模块状态文件权限"))?;
    }
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| state_error("module_state_write_failed", "无法同步模块状态临时文件"))?;
    verify()?;
    temporary
        .persist(target)
        .map_err(|_| state_error("module_state_write_failed", "无法原子替换模块状态文件"))?;
    // rename 是提交点；其后不做会错误宣称“旧字节仍在”的回滚或删除。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn snapshot_preserves_unknown_values_exact_keys_and_private_debug() {
        let snapshot = ModuleActivationSnapshot::from_entries([
            ("Blog", json!(true)),
            ("off", json!(false)),
            ("null", Value::Null),
            ("PRIVATE_KEY", json!({"token":"PRIVATE_VALUE"})),
            ("text", json!("true")),
        ]);
        assert_eq!(snapshot.enabled(), HashSet::from(["Blog".to_owned()]));
        assert!(snapshot.is_enabled("Blog"));
        assert!(!snapshot.is_enabled("blog"));
        for name in ["missing", "null", "off"] {
            assert!(snapshot.has_status(name, false));
            assert!(!snapshot.has_status(name, true));
        }
        for name in ["text", "PRIVATE_KEY"] {
            assert!(!snapshot.has_status(name, false));
            assert!(!snapshot.has_status(name, true));
            assert!(snapshot.is_disabled(name));
        }
        assert_eq!(snapshot.value("null"), Some(&Value::Null));
        assert_eq!(snapshot.entries().len(), 5);
        let debug = format!("{snapshot:?}");
        assert!(!debug.contains("PRIVATE_KEY"));
        assert!(!debug.contains("PRIVATE_VALUE"));
    }

    #[test]
    fn read_only_driver_keeps_legacy_enabled_and_refuses_all_mutations() -> Result<()> {
        struct ReadOnly;
        impl ModuleActivator for ReadOnly {
            fn enabled(&self) -> Result<HashSet<String>> {
                Ok(HashSet::from(["Blog".to_owned()]))
            }
        }
        let driver = ReadOnly;
        assert!(driver.snapshot()?.is_enabled("Blog"));
        assert!(driver.path().is_none());
        for result in [
            driver.set_active_by_name("Blog", false),
            driver.delete("Blog"),
            driver.reset(),
        ] {
            assert_eq!(
                result.err().map(|error| error.code().to_owned()).as_deref(),
                Some("module_activator_read_only")
            );
        }
        Ok(())
    }

    // Unix共享open-file-description的副本不能延长操作守卫已经结束的临界区。
    #[cfg(unix)]
    #[test]
    fn guard_explicitly_unlocks_while_a_duplicated_descriptor_remains_alive() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("state.json");
        let locked = LockedTarget::acquire(&path)?;
        let duplicate = locked.lock.try_clone().map_err(Error::internal)?;
        let activator = FileModuleActivator::new(&path);
        assert_eq!(
            activator
                .set_active_by_name("Blog", true)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("module_state_busy")
        );
        drop(locked);
        let independent = LockedTarget::acquire(&path)?;
        duplicate.unlock().map_err(Error::internal)?;
        assert_eq!(
            activator
                .set_active_by_name("Blog", true)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("module_state_busy")
        );
        drop(independent);
        activator.set_active_by_name("Blog", true)?;
        assert!(activator.snapshot()?.is_enabled("Blog"));
        assert!(duplicate.metadata().map_err(Error::internal)?.is_file());
        drop(duplicate);
        Ok(())
    }

    #[test]
    fn two_instances_reread_and_preserve_unknown_values_and_noop_delete_bytes() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("custom.json");
        let first = FileModuleActivator::new(&path);
        let second = FileModuleActivator::new(&path);
        assert_eq!(first.statuses_file_path(), path);
        assert!(first.snapshot()?.entries().is_empty());
        assert_eq!(
            fs::read_dir(directory.path())
                .map_err(Error::internal)?
                .count(),
            0
        );
        let bytes = r#"{ "unknown": {"value":"PRIVATE_VALUE"}, "null":null, "blog":true }"#;
        fs::write(&path, bytes).map_err(Error::internal)?;
        let before = first.snapshot()?;
        assert!(!first.has_status("Blog", true)?);
        assert!(first.has_status("blog", true)?);
        second.delete("null")?;
        second.delete("missing")?;
        assert_eq!(fs::read_to_string(&path).map_err(Error::internal)?, bytes);
        first.set_active_by_name("Blog", true)?;
        second.set_active_by_name("Other", true)?;
        first.set_active_by_name("Blog", false)?;
        first.set_active_by_name("Blog", false)?;
        let current = second.snapshot()?;
        assert!(current.has_status("Blog", false));
        assert!(current.is_enabled("Other"));
        assert_eq!(
            current.value("unknown"),
            Some(&json!({"value":"PRIVATE_VALUE"}))
        );
        assert_eq!(current.value("null"), Some(&Value::Null));
        assert_eq!(before.value("Blog"), None);
        second.delete("Other")?;
        second.delete("Other")?;
        assert_eq!(first.snapshot()?.value("Other"), None);
        assert!(first.snapshot()?.is_enabled("blog"));
        Ok(())
    }

    #[test]
    fn invalid_bytes_are_never_rewritten_but_explicit_reset_clears_them() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("state.json");
        let activator = FileModuleActivator::new(&path);
        for bytes in [b"{broken".as_slice(), b"[true]", b"null", b"false", b"\xff"] {
            fs::write(&path, bytes).map_err(Error::internal)?;
            assert!(activator.snapshot().is_err());
            assert!(activator.set_active_by_name("Blog", true).is_err());
            assert!(activator.delete("Blog").is_err());
            assert_eq!(fs::read(&path).map_err(Error::internal)?, bytes);
            assert_no_temporaries(directory.path())?;
        }
        activator.reset()?;
        assert!(!path.exists());
        activator.reset()?;
        for text in ["", " \n\t", "[]", "{}"] {
            fs::write(&path, text).map_err(Error::internal)?;
            assert!(activator.snapshot()?.entries().is_empty());
            activator.set_active_by_name("Blog", true)?;
            assert!(activator.snapshot()?.is_enabled("Blog"));
        }
        Ok(())
    }

    #[test]
    fn missing_parent_is_not_created_and_failed_commit_cleans_temporary_file() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let missing_path = directory.path().join("absent/state.json");
        let activator = FileModuleActivator::new(&missing_path);
        assert!(activator.snapshot()?.entries().is_empty());
        activator.delete("Blog")?;
        activator.reset()?;
        assert!(activator.set_active_by_name("Blog", true).is_err());
        assert!(!directory.path().join("absent").exists());

        let original = directory.path().join("state.json");
        fs::write(&original, b"{ \"Blog\":true }").map_err(Error::internal)?;
        let entries = ModuleActivationSnapshot::from_enabled(["Other".to_owned()]);
        let result = write_atomic(&original, entries.entries(), None, || {
            Err(state_error(
                "module_state_path_changed",
                "测试提交前路径变化",
            ))
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read(&original).map_err(Error::internal)?,
            b"{ \"Blog\":true }"
        );
        assert_no_temporaries(directory.path())?;

        let directory_target = directory.path().join("directory-target");
        fs::create_dir(&directory_target).map_err(Error::internal)?;
        fs::write(directory_target.join("keep"), b"preserve").map_err(Error::internal)?;
        assert!(write_atomic(&directory_target, entries.entries(), None, || Ok(())).is_err());
        assert_eq!(
            fs::read(directory_target.join("keep")).map_err(Error::internal)?,
            b"preserve"
        );
        assert_no_temporaries(directory.path())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_configuration_keeps_link_identity_and_existing_target_permissions() -> Result<()> {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let target = directory.path().join("real.json");
        fs::write(&target, b"{}").map_err(Error::internal)?;
        fs::set_permissions(&target, Permissions::from_mode(0o640)).map_err(Error::internal)?;
        let alias = directory.path().join("alias.json");
        symlink("real.json", &alias).map_err(Error::internal)?;
        let activator = FileModuleActivator::new(&alias);
        activator.set_active_by_name("Blog", true)?;
        assert!(
            fs::symlink_metadata(&alias)
                .map_err(Error::internal)?
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_link(&alias).map_err(Error::internal)?,
            Path::new("real.json")
        );
        assert_eq!(
            fs::metadata(&target)
                .map_err(Error::internal)?
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        activator.reset()?;
        assert!(!target.exists());
        assert!(
            fs::symlink_metadata(&alias)
                .map_err(Error::internal)?
                .file_type()
                .is_symlink()
        );
        activator.set_active_by_name("Other", true)?;
        assert!(activator.snapshot()?.is_enabled("Other"));
        assert!(target.is_file());
        let directory_alias = directory.path().join("directory-alias");
        symlink(directory.path(), &directory_alias).map_err(Error::internal)?;
        assert!(
            FileModuleActivator::new(directory_alias)
                .set_active_by_name("Blog", true)
                .is_err()
        );
        let protected = directory.path().join("protected.json");
        fs::write(&protected, b"preserve").map_err(Error::internal)?;
        let other = directory.path().join("other.json");
        symlink(&protected, sidecar_path(&other)?).map_err(Error::internal)?;
        assert!(
            FileModuleActivator::new(&other)
                .set_active_by_name("Blog", true)
                .is_err()
        );
        assert_eq!(fs::read(protected).map_err(Error::internal)?, b"preserve");
        assert!(!other.exists());
        Ok(())
    }

    fn assert_no_temporaries(directory: &Path) -> Result<()> {
        let names = fs::read_dir(directory)
            .map_err(Error::internal)?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(Error::internal)?;
        assert!(!names.iter().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".module-state-")
        }));
        Ok(())
    }

    struct ChildProcess(Child);

    impl ChildProcess {
        fn spawn(mode: &str, path: &Path, ready: &Path, release: &Path) -> Result<Self> {
            let module = module_path!()
                .split_once("::")
                .map_or(module_path!(), |(_, name)| name);
            let child = Command::new(std::env::current_exe().map_err(Error::internal)?)
                .args([
                    "--exact",
                    &format!("{module}::subprocess_worker"),
                    "--nocapture",
                ])
                .env("OPENRUST_MODULE_ACTIVATOR_CHILD", mode)
                .env("OPENRUST_MODULE_ACTIVATOR_PATH", path)
                .env("OPENRUST_MODULE_ACTIVATOR_READY", ready)
                .env("OPENRUST_MODULE_ACTIVATOR_RELEASE", release)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(Error::internal)?;
            Ok(Self(child))
        }

        fn wait_success(&mut self) -> Result<()> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.0.try_wait().map_err(Error::internal)? {
                    assert!(status.success(), "模块状态测试子进程失败");
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(state_error("test_child_timeout", "模块状态测试子进程超时"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for ChildProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn wait_for_file(path: &Path) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            if Instant::now() >= deadline {
                return Err(state_error("test_child_timeout", "模块状态测试信号超时"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    fn retry_write(activator: &FileModuleActivator, name: &str) -> Result<()> {
        for _ in 0..200 {
            match activator.set_active_by_name(name, true) {
                Ok(()) => return Ok(()),
                Err(error) if error.code() == "module_state_busy" => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        Err(state_error("test_child_timeout", "模块状态重试次数用尽"))
    }

    #[test]
    fn subprocess_worker() -> Result<()> {
        let Some(mode) = std::env::var_os("OPENRUST_MODULE_ACTIVATOR_CHILD") else {
            return Ok(());
        };
        let path = std::env::var_os("OPENRUST_MODULE_ACTIVATOR_PATH")
            .ok_or_else(|| state_error("test_child_input", "缺少测试状态路径"))?;
        let ready = std::env::var_os("OPENRUST_MODULE_ACTIVATOR_READY")
            .ok_or_else(|| state_error("test_child_input", "缺少测试就绪路径"))?;
        let release = std::env::var_os("OPENRUST_MODULE_ACTIVATOR_RELEASE")
            .ok_or_else(|| state_error("test_child_input", "缺少测试释放路径"))?;
        if mode == "cwd" {
            std::env::set_current_dir(&ready).map_err(Error::internal)?;
            let activator = FileModuleActivator::new("state.json");
            assert_eq!(
                activator.statuses_file_path(),
                Path::new(&ready).join("state.json")
            );
            std::env::set_current_dir(&release).map_err(Error::internal)?;
            assert!(activator.snapshot()?.is_enabled("First"));
            assert!(!activator.snapshot()?.is_enabled("Second"));
            activator.set_active_by_name("Added", true)?;
            activator.delete("First")?;
            assert!(activator.snapshot()?.is_enabled("Added"));
            assert_eq!(activator.snapshot()?.value("First"), None);
            activator.reset()?;
            assert!(activator.snapshot()?.entries().is_empty());
            activator.set_active_by_name("Final", true)
        } else if mode == "hold" {
            let _locked = LockedTarget::acquire(Path::new(&path))?;
            fs::write(Path::new(&ready), b"ready").map_err(Error::internal)?;
            wait_for_file(Path::new(&release))?;
            Ok(())
        } else {
            retry_write(&FileModuleActivator::new(path), "Child")
        }
    }

    #[test]
    fn relative_path_is_captured_before_cwd_changes_in_an_isolated_child() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let root = directory.path().canonicalize().map_err(Error::internal)?;
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir(&first).map_err(Error::internal)?;
        fs::create_dir(&second).map_err(Error::internal)?;
        fs::write(first.join("state.json"), br#"{"First":true}"#).map_err(Error::internal)?;
        let second_bytes = br#"{ "Second":true }"#;
        fs::write(second.join("state.json"), second_bytes).map_err(Error::internal)?;
        let mut child = ChildProcess::spawn("cwd", directory.path(), &first, &second)?;
        child.wait_success()?;
        assert_eq!(
            FileModuleActivator::new(first.join("state.json"))
                .snapshot()?
                .entries(),
            ModuleActivationSnapshot::from_enabled(["Final".to_owned()]).entries()
        );
        assert_eq!(
            fs::read(second.join("state.json")).map_err(Error::internal)?,
            second_bytes
        );
        assert!(!sidecar_path(&second.join("state.json"))?.exists());
        assert_no_temporaries(&first)?;
        assert_no_temporaries(&second)
    }

    #[test]
    fn failed_path_capture_is_reported_by_every_operation_without_cwd_fallback() {
        let activator = FileModuleActivator::new("");
        for error in [
            activator.snapshot().err(),
            activator.set_active_by_name("Blog", true).err(),
            activator.delete("Blog").err(),
            activator.reset().err(),
        ] {
            assert_eq!(
                error.map(|error| error.code().to_owned()).as_deref(),
                Some("module_state_path_invalid")
            );
        }
    }

    #[test]
    fn subprocess_lock_reports_busy_then_releases_and_retry_preserves_both_writes() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("state.json");
        let ready = directory.path().join("ready");
        let release = directory.path().join("release");
        let activator = FileModuleActivator::new(&path);
        activator.set_active_by_name("Existing", true)?;
        let mut child = ChildProcess::spawn("hold", &path, &ready, &release)?;
        wait_for_file(&ready)?;
        let error = activator
            .set_active_by_name("Parent", true)
            .err()
            .ok_or_else(|| state_error("test_expected_busy", "子进程持锁时写入未被拒绝"))?;
        assert_eq!(error.code(), "module_state_busy");
        assert!(!activator.snapshot()?.is_enabled("Parent"));
        fs::write(&release, b"release").map_err(Error::internal)?;
        child.wait_success()?;
        let mut child = ChildProcess::spawn("write", &path, &ready, &release)?;
        retry_write(&activator, "Parent")?;
        child.wait_success()?;
        let snapshot = activator.snapshot()?;
        assert!(snapshot.is_enabled("Existing"));
        assert!(snapshot.is_enabled("Parent"));
        assert!(snapshot.is_enabled("Child"));
        assert_no_temporaries(directory.path())
    }
}

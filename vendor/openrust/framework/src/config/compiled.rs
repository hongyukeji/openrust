//! 引导配置缓存的数据文件；文件内容可能包含秘密，不进入日志或错误。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::{ConfigError, ConfigSources, EnvironmentSnapshot};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// 缓存路径相对应用根目录；显式绝对路径原样使用。
#[cfg(test)]
pub(crate) fn selected_path(
    namespace: &str,
    environment: &EnvironmentSnapshot,
) -> Result<PathBuf, ConfigError> {
    let app_dir = std::env::current_dir().map_err(|_| failure("无法定位应用根目录"))?;
    selected_path_from(&app_dir, namespace, environment)
}

pub(crate) fn selected_path_from(
    app_dir: &Path,
    namespace: &str,
    environment: &EnvironmentSnapshot,
) -> Result<PathBuf, ConfigError> {
    cache_path(
        app_dir,
        namespace,
        environment,
        "CONFIG_CACHE",
        "bootstrap/cache/config.json",
    )
}

pub(crate) fn cache_path(
    app_dir: &Path,
    namespace: &str,
    environment: &EnvironmentSnapshot,
    suffix: &str,
    default: &str,
) -> Result<PathBuf, ConfigError> {
    super::validate_namespace(namespace)?;
    let key = format!("{namespace}{suffix}");
    let selected = environment.get_os(&key);
    if selected.as_ref().is_some_and(|value| value.is_empty()) {
        return Err(failure("缓存路径不能为空"));
    }
    let path = selected.map_or_else(|| PathBuf::from(default), PathBuf::from);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(app_dir.join(path))
    }
}

/// 不存在时不命中；非普通文件及损坏内容均明确失败。
pub(crate) fn load(path: &Path) -> Result<Option<ConfigSources>, ConfigError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure("无法检查缓存文件")),
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(failure("缓存路径不是普通文件"));
        }
        Ok(_) => {}
    }
    let bytes = fs::read(path).map_err(|_| failure("无法读取缓存文件"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| failure("缓存文件格式无效"))?;
    ConfigSources::from_cached(value).map(Some)
}

/// 只删除选中的缓存文件；缺失可重复清除，其他错误必须报告。
pub(crate) fn clear(path: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(failure("无法检查缓存文件")),
        Ok(metadata) if !metadata.file_type().is_file() => Err(failure("缓存路径不是普通文件")),
        Ok(_) => fs::remove_file(path).map_err(|_| failure("无法清除缓存文件")),
    }
}

/// 完整验证后同目录原子替换；临时文件仅允许所有者读取。
pub(crate) fn write(path: &Path, value: &Value) -> Result<(), ConfigError> {
    let bytes = serde_json::to_vec(value).map_err(|_| failure("无法序列化配置缓存"))?;
    let decoded: Value =
        serde_json::from_slice(&bytes).map_err(|_| failure("缓存序列化校验失败"))?;
    ConfigSources::from_cached(decoded)?;
    let parent = path.parent().ok_or_else(|| failure("缓存路径缺少父目录"))?;
    fs::create_dir_all(parent).map_err(|_| failure("无法创建缓存目录"))?;
    // 只检查最终目标；调用者显式选择的父目录可由应用部署机制管理。
    if fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_file()) {
        return Err(failure("缓存路径不是普通文件"));
    }
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".config-{}-{sequence}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|_| failure("无法建立缓存临时文件"))?;
    let result: Result<(), ConfigError> = (|| {
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| failure("无法写入缓存临时文件"))?;
        drop(file);
        fs::rename(&temp, path).map_err(|_| failure("无法替换缓存文件"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    let verified = load(path).and_then(|sources| {
        sources
            .and_then(|sources| sources.into_parts().1)
            .ok_or_else(|| failure("缓存写入后缺少有序值"))
    });
    match verified {
        Ok(verified) if verified == *value => Ok(()),
        _ => {
            let _ = fs::remove_file(path);
            Err(failure("缓存写入后校验失败"))
        }
    }
}

fn failure(message: &str) -> ConfigError {
    ConfigError::Invalid(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_preserves_order_and_rejects_broken_cache_without_leaking_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("config.json");
        let value = json!({
            "z": {"01": "first", "0": "secret-sample", "2": "third"},
            "a": ["one", "two"]
        });
        write(&path, &value)?;
        let sources = load(&path)?.ok_or("cache should exist")?;
        let repository = crate::config::repository::ConfigRepository::new(sources);
        assert_eq!(repository.all()?, value);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        }
        fs::write(&path, b"secret-sample invalid json")?;
        let error = load(&path)
            .err()
            .ok_or("broken cache should fail")?
            .to_string();
        assert!(!error.contains("secret-sample"));
        clear(&path)?;
        clear(&path)?;
        assert!(load(&path)?.is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_is_neither_read_nor_removed() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target");
        let link = directory.path().join("config.json");
        fs::write(&target, b"{}")?;
        symlink(&target, &link)?;
        assert!(load(&link).is_err());
        assert!(clear(&link).is_err());
        assert!(target.exists());
        Ok(())
    }

    // Figment 的 Jail 回调固定返回较大的 figment::Error；仅此测试使用该 API。
    #[allow(clippy::result_large_err)]
    #[test]
    fn namespaced_selection_is_relative_to_application_root() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            let root =
                std::env::current_dir().map_err(|error| figment::Error::from(error.to_string()))?;
            let environment = EnvironmentSnapshot::capture();
            assert_eq!(
                selected_path("SHOP_", &environment)
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                root.join("bootstrap/cache/config.json")
            );
            jail.set_env("SHOP_CONFIG_CACHE", "storage/private/config.json");
            let environment = EnvironmentSnapshot::capture();
            assert_eq!(
                selected_path("SHOP_", &environment)
                    .map_err(|error| figment::Error::from(error.to_string()))?,
                root.join("storage/private/config.json")
            );
            Ok(())
        });
    }
}

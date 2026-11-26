//! 在用户路由装配前检查套件使用的本地配置，不向远端供应商发请求。
use super::{avatar, configuration};
use openrust::{
    auth::AuthError,
    contracts::config::Repository,
    database::{DatabaseManager, eloquent::Model},
    filesystem::FilesystemManager,
    foundation::Application,
    support::ProviderResult,
};

fn name(config: &dyn Repository, key: &'static str, default: &str) -> Result<String, AuthError> {
    match config.get(key) {
        None => Ok(default.into()),
        Some(serde_json::Value::String(value))
            if !value.is_empty()
                && value.len() <= 255
                && value.trim() == value
                && !value.chars().any(char::is_control) =>
        {
            Ok(value)
        }
        _ => Err(AuthError::Configuration(key)),
    }
}

pub(super) fn validate<M: Model>(app: &Application) -> ProviderResult {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let container = app.container();
    let config = container.make::<dyn Repository>()?;
    if configuration::boolean(config.as_ref(), "user.avatar.enabled", true)? {
        avatar::view_limits(config.as_ref())?;
        let disk = name(config.as_ref(), "user.avatar.disk", "public")?;
        let settings = config
            .get("filesystems.disks")
            .and_then(|disks| disks.get(&disk).cloned())
            .ok_or(AuthError::Configuration(
                "用户头像磁盘不存在，请配置filesystems.disks",
            ))?;
        if settings
            .get("visibility")
            .and_then(serde_json::Value::as_str)
            != Some("public")
        {
            return Err(AuthError::Configuration(
                "默认用户头像需要明确配置公开磁盘；私有存储须替换处理器与资源",
            )
            .into());
        }
        container.make::<FilesystemManager>()?.disk(Some(&disk))?;
    }
    let queue = name(config.as_ref(), "user.cleanup.connection", "database")?;
    name(config.as_ref(), "user.cleanup.queue", "user-resources")?;
    let settings = config
        .get("queue.connections")
        .and_then(|connections| connections.get(&queue).cloned())
        .ok_or(AuthError::Configuration(
            "用户清理队列连接不存在，请配置同库database队列",
        ))?;
    if settings.get("driver").and_then(serde_json::Value::as_str) != Some("database") {
        return Err(AuthError::Configuration("用户清理队列必须使用同库database驱动").into());
    }
    let queue_connection = match settings.get("connection") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) if !value.is_empty() && value.len() <= 255 => {
            Some(value.as_str())
        }
        _ => {
            return Err(
                AuthError::Configuration("用户清理队列数据库连接必须为字符串或null").into(),
            );
        }
    };
    let database = container.make::<DatabaseManager>()?;
    let model = M::from_state(Default::default());
    if database.connection(queue_connection)?.get_name() != model.get_connection()?.get_name() {
        return Err(
            AuthError::Configuration("用户清理队列与用户模型必须选中同一具名数据库连接").into(),
        );
    }
    Ok(())
}

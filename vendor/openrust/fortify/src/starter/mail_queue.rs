//! 显式开启用户套件认证邮件队列；默认仍沿用原生同步通知。
use openrust::{
    auth::{
        AuthError,
        notifications::{AuthMailQueueOptions, ResetPassword, VerifyEmail},
    },
    contracts::{
        auth::Authenticatable, config::Repository, container::Container as _,
        foundation::Application as _,
    },
    database::eloquent::Model,
    foundation::Application,
    queue::{DispatchOptions, QueueManager},
    support::ProviderResult,
};
use std::{
    any::{Any, TypeId},
    collections::BTreeSet,
};
fn option(config: &dyn Repository, key: &str) -> Result<Option<String>, AuthError> {
    match config.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value))
            if !value.is_empty()
                && value.trim() == value
                && !value.contains(',')
                && value.len() <= 255
                && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value))
        }
        _ => Err(AuthError::Configuration(
            "用户邮件队列名称和连接必须为非空字符串或null",
        )),
    }
}
fn persistent(
    config: &dyn Repository,
    name: &str,
    seen: &mut BTreeSet<String>,
) -> Result<(), AuthError> {
    if !seen.insert(name.into()) {
        return Err(AuthError::Configuration(
            "认证邮件队列的failover连接存在循环",
        ));
    }
    let raw = config
        .get("queue.connections")
        .and_then(|value| value.get(name).cloned());
    let driver = raw
        .as_ref()
        .and_then(|value| value.get("driver"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(if name == "sync" { "sync" } else { "" });
    if matches!(driver, "sync" | "null" | "deferred" | "background" | "") {
        return Err(AuthError::Configuration(
            "认证邮件队列需要持久化连接，不能使用同步、丢弃或进程内驱动",
        ));
    }
    if driver == "failover" {
        let entries = raw
            .as_ref()
            .and_then(|value| value.get("connections"))
            .and_then(serde_json::Value::as_array)
            .filter(|values| !values.is_empty())
            .ok_or(AuthError::Configuration("认证邮件failover连接列表无效"))?;
        for value in entries {
            persistent(
                config,
                value
                    .as_str()
                    .ok_or(AuthError::Configuration("认证邮件failover连接名称无效"))?,
                seen,
            )?;
        }
    }
    seen.remove(name);
    Ok(())
}

/// 队列名为null时同步发送；开启后按实际用户类型装配原生通知加密与队列策略。
pub fn configure_mail_queue<M: Authenticatable + Model>(app: &Application) -> ProviderResult {
    let config = app.container().make::<dyn Repository>()?;
    let queue = option(config.as_ref(), "user.mail.queue")?;
    let Some(queue) = queue else {
        ResetPassword::queue_for::<M>(app, None)?;
        VerifyEmail::queue_for::<M>(app, None)?;
        return Ok(());
    };
    let connection = option(config.as_ref(), "user.mail.connection")?;
    let default = config
        .get("queue.default")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "sync".into());
    persistent(
        config.as_ref(),
        connection.as_deref().unwrap_or(&default),
        &mut BTreeSet::new(),
    )?;
    let probe = M::new_instance(Default::default())?;
    if probe
        .as_password_reset()
        .is_none_or(|user| (user as &dyn Any).type_id() != TypeId::of::<M>())
    {
        return Err(AuthError::Configuration(
            "队列用户套件需要模型直接提供CanResetPassword；包装能力请手动注册原生邮件队列策略",
        )
        .into());
    }
    if super::configuration::boolean(config.as_ref(), "user.email_verification", true)?
        && probe
            .as_email_verification()
            .is_none_or(|user| (user as &dyn Any).type_id() != TypeId::of::<M>())
    {
        return Err(AuthError::Configuration(
            "队列用户套件需要模型直接提供MustVerifyEmail；包装能力请手动注册原生邮件队列策略",
        )
        .into());
    }
    app.container()
        .make::<QueueManager>()?
        .connection(connection.as_deref())?;
    let options = AuthMailQueueOptions {
        dispatch: DispatchOptions {
            connection,
            queue: Some(queue),
            after_commit: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    ResetPassword::queue_for::<M>(app, Some(options.clone()))?;
    VerifyEmail::queue_for::<M>(
        app,
        if super::configuration::boolean(config.as_ref(), "user.email_verification", true)? {
            Some(options)
        } else {
            None
        },
    )?;
    Ok(())
}

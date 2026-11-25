//! 认证邮件的可选队列策略；复用通知加密、队列连接和Worker，不改变独立应用默认值。
use super::MailCallbacks;
use crate::{
    auth::{AuthError, AuthResult},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    queue::{DispatchOptions, JobOptions},
};
use std::any::{Any, TypeId};

/// 认证链接进入队列时始终加密；重试和超时沿用原生JobOptions／Worker选项。
#[derive(Clone, Default)]
pub struct AuthMailQueueOptions {
    pub dispatch: DispatchOptions,
    pub job: JobOptions,
}

/// 按实际认证能力类型选择队列；传None恢复同步，不影响其他模型或应用。
/// 自定义CanResetPassword／MustVerifyEmail包装对象须注册其实际具体类型。
pub(super) fn configure_for<T: Any>(
    app: &Application,
    kind: &'static str,
    options: Option<AuthMailQueueOptions>,
) -> AuthResult<()> {
    let callbacks = app
        .container()
        .make::<std::sync::RwLock<MailCallbacks>>()
        .map_err(AuthError::backend)?;
    let mut callbacks = callbacks.write().expect("auth mail callbacks lock");
    match options {
        Some(options) => {
            callbacks.queues.insert((TypeId::of::<T>(), kind), options);
        }
        None => {
            callbacks.queues.remove(&(TypeId::of::<T>(), kind));
        }
    }
    Ok(())
}
pub(super) fn for_type(
    name: &'static str,
    kind: Option<TypeId>,
) -> AuthResult<Option<AuthMailQueueOptions>> {
    let Some(kind) = kind else { return Ok(None) };
    Ok(MailCallbacks::get()?
        .read()
        .expect("auth mail callbacks lock")
        .queues
        .get(&(kind, name))
        .cloned())
}

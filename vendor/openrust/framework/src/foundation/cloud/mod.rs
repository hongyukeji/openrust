//! 托管环境识别与实际托管队列连接。
mod agent;
mod job;
pub(crate) mod worker;
pub use agent::AgentUnreachable;

mod facade;
mod macros;
mod provider;
mod queue;
use crate::{
    config::Environment,
    container::Container,
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
    },
    queue::{QueueError, QueueManager},
};
pub use facade::CloudFacade;
pub use macros::CloudMacro;
pub use provider::CloudServiceProvider;
pub use queue::Queue;
use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, RwLock, Weak},
};
pub type CloudResult<T> = Result<T, CloudError>;
#[derive(Debug)]
pub enum CloudError {
    Binding(BindingResolutionError),
    Queue(QueueError),
    NotConfigured,
    WrongConnectionType,
    UnknownMacro(String),
    MacroType {
        name: String,
        expected: &'static str,
        actual: &'static str,
    },
    Callback(Box<dyn Error + Send + Sync>),
}
impl fmt::Display for CloudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(error) => write!(f, "托管服务不可用：{error}"),
            Self::Queue(error) => write!(f, "托管队列操作失败：{error}"),
            Self::NotConfigured => write!(f, "当前应用没有配置托管队列"),
            Self::WrongConnectionType => write!(f, "cloud连接不是托管队列服务"),
            Self::UnknownMacro(name) => write!(f, "没有注册托管服务宏[{name}]"),
            Self::MacroType {
                name,
                expected,
                actual,
            } => write!(
                f,
                "托管服务宏[{name}]类型不符：需要{expected}，实际为{actual}"
            ),
            Self::Callback(error) => write!(f, "托管服务回调失败：{error}"),
        }
    }
}
impl Error for CloudError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Queue(error) => Some(error),
            Self::Callback(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
impl From<BindingResolutionError> for CloudError {
    fn from(error: BindingResolutionError) -> Self {
        Self::Binding(error)
    }
}
impl From<QueueError> for CloudError {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}
impl CloudError {
    pub fn callback(error: impl Error + Send + Sync + 'static) -> Self {
        Self::Callback(Box::new(error))
    }
}
/// 应用级管理器；托管标记使用既有环境快照，队列选择读取实时配置。
pub struct CloudManager {
    owner: Weak<Container>,
    macros: RwLock<HashMap<String, CloudMacro>>,
}
impl CloudManager {
    pub(crate) fn new(owner: Weak<Container>) -> Self {
        Self {
            owner,
            macros: RwLock::default(),
        }
    }
    fn owner(&self) -> CloudResult<Arc<Container>> {
        self.owner
            .upgrade()
            .ok_or_else(|| BindingResolutionError::FacadeNotSet.into())
    }
    pub fn hosted(&self) -> CloudResult<bool> {
        Ok(self.owner()?.make::<Environment>()?.get("LARAVEL_CLOUD") == Some("1"))
    }
    pub fn uses_managed_queues(&self) -> CloudResult<bool> {
        Ok(self
            .owner()?
            .get::<dyn Repository>("config")?
            .get("queue.connections.cloud.driver")
            .as_ref()
            .and_then(serde_json::Value::as_str)
            == Some("cloud"))
    }
    pub fn queue(&self) -> CloudResult<Arc<Queue>> {
        if !self.uses_managed_queues()? {
            return Err(CloudError::NotConfigured);
        }
        let queue = self
            .owner()?
            .make::<QueueManager>()?
            .connection(Some("cloud"))?;
        let erased = queue.into_any().ok_or(CloudError::WrongConnectionType)?;
        erased
            .downcast::<Queue>()
            .map_err(|_| CloudError::WrongConnectionType)
    }
    pub fn is_managed_queue(&self, name: &str) -> CloudResult<bool> {
        if !self.uses_managed_queues()? {
            return Ok(false);
        }
        Ok(self
            .queue()?
            .managed_queues()?
            .contains(&serde_json::Value::String(name.to_owned())))
    }
}

pub use worker::AgentAwareLostConnectionDetector;

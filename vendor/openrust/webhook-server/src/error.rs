/// 发送扩展错误，不包含密钥或原始正文。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Webhook 发送配置无效：{0}")]
    InvalidConfig(&'static str),
    #[error("Webhook 目的地未通过验证")]
    TargetDenied,
    #[error("Webhook 目的地暂时无法解析")]
    TargetResolutionFailed,
    #[error("Webhook 投递存储操作失败")]
    Database,
    #[error("Webhook 队列操作失败")]
    Queue,
    #[error("Webhook 投递记录无效")]
    InvalidRecord,
    #[error("Webhook 幂等键已对应另一份快照")]
    IdempotencyConflict,
    #[error("Webhook 投递租约已失效")]
    LeaseLost,
    #[error("Webhook 投递已结束且未成功")]
    DeliveryFailed,
    #[error("Webhook 重放未获授权")]
    ReplayDenied,
    #[error("Webhook 投递暂不能重放")]
    ReplayUnavailable,
    #[error("Webhook 重放代次已由另一进程登记")]
    ReplayConflict,
    #[error("Webhook 同步投递已结束且未成功：{delivery_id}")]
    SynchronousDeliveryFailed { delivery_id: String },
    #[error("Webhook 派发前事件失败：{delivery_id}")]
    DispatchEventFailed { delivery_id: String },
    #[error("Webhook 正文不符合签名协议")]
    InvalidPayload,
}

impl From<openrust::database::DatabaseError> for Error {
    fn from(_: openrust::database::DatabaseError) -> Self {
        // 驱动错误可能包含执行语句或带敏感内容的字段。
        Self::Database
    }
}

impl From<openrust::queue::QueueError> for Error {
    fn from(_: openrust::queue::QueueError) -> Self {
        Self::Queue
    }
}

pub type Result<T> = std::result::Result<T, Error>;

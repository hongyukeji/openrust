/// 接收扩展错误；显示信息不包含正文、签名或密钥。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Webhook 配置无效：{0}")]
    InvalidConfig(&'static str),
    #[error("Webhook 正文超过限制")]
    PayloadTooLarge,
    #[error("Webhook 请求头超过限制")]
    HeadersTooLarge,
    #[error("Webhook 签名无效")]
    InvalidSignature,
    #[error("Webhook 无签名提示处理失败")]
    HintProcessingFailed,
    #[error("Webhook 验证器未提供有效的已签名事件 ID")]
    MissingVerifiedEventId,
    #[error("Webhook 事件 ID 已用于不同的请求内容")]
    VerifiedEventConflict,
    #[error("未找到 Webhook 配置：{name:?}")]
    ConfigNotFound { name: String },
    #[error("Webhook 方法不允许：{method:?}")]
    InvalidMethod { method: String },
    #[error("Webhook 实现绑定无效：{field}")]
    InvalidImplementation {
        field: &'static str,
        #[source]
        source: openrust::contracts::container::BindingResolutionError,
    },
    #[error("Webhook HTTP 读取失败")]
    Http(#[from] openrust::http::HttpError),
    #[error("Webhook 存储操作失败")]
    Database(#[from] openrust::database::DatabaseError),
    #[error("Webhook 存储记录无效")]
    InvalidRecord,
    #[error("Webhook 附件存储操作失败")]
    AttachmentStorage,
    #[error("Webhook 重放未获授权")]
    ReplayDenied,
    #[error("Webhook 接收记录已不可重放")]
    ReplayUnavailable,
    #[error("Webhook 队列操作失败")]
    Queue(#[from] openrust::queue::QueueError),
    #[error("Webhook 处理任务派发失败")]
    DispatchFailed,
    #[error("Webhook 处理任务执行失败")]
    ProcessingFailed,
    #[error("Webhook 处理租约已失效")]
    ProcessingLeaseLost,
    #[error("Webhook 待关联通知已超出处理期限或延期次数")]
    ProcessingDeferredExhausted,
    #[error("Webhook 存储记录不存在或不属于当前范围")]
    RecordNotFound,
    #[error("Webhook 存储适配未实现载荷读取")]
    PayloadUnsupported,
    #[error("Webhook 存储适配未实现保留期查询")]
    RetentionUnsupported,
    #[error("Webhook 运行操作失败")]
    Runtime {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn binding(source: openrust::contracts::container::BindingResolutionError) -> Self {
        Self::Runtime {
            source: Box::new(source),
        }
    }
}

impl From<openrust::contracts::container::BindingResolutionError> for Error {
    fn from(source: openrust::contracts::container::BindingResolutionError) -> Self {
        Self::binding(source)
    }
}

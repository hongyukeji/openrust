use std::fmt;

pub type AiResult<T> = Result<T, AiError>;

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AiError {
    Unavailable,
    DefaultNotConfigured,
    ProviderNotConfigured,
    DriverNotRegistered,
    InvalidConfiguration(&'static str),
    InvalidPrompt,
    MissingModel,
    MissingCredential,
    ModelsNotSupported,
    StreamingNotSupported,
    EndpointNotSupported,
    EmbeddingsNotSupported,
    Transport,
    Timeout,
    ProviderStatus(u16),
    InvalidResponse,
    Refused,
    FakeExhausted,
}

impl fmt::Display for AiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("当前应用未装配 AI SDK"),
            Self::DefaultNotConfigured => formatter.write_str("未配置 AI 默认连接"),
            Self::ProviderNotConfigured => formatter.write_str("AI 连接未配置"),
            Self::DriverNotRegistered => formatter.write_str("AI 协议驱动未注册"),
            Self::InvalidConfiguration(field) => write!(formatter, "AI 配置字段无效：{field}"),
            Self::InvalidPrompt => formatter.write_str("AI 提示词不能为空"),
            Self::MissingModel => formatter.write_str("请选择 AI 模型，或为当前连接配置默认模型"),
            Self::MissingCredential => formatter.write_str("AI 连接的环境凭证未配置"),
            Self::ModelsNotSupported => formatter.write_str("该 AI 协议驱动未提供模型目录"),
            Self::StreamingNotSupported => formatter.write_str("该 AI 协议驱动未提供流式输出"),
            Self::EndpointNotSupported => formatter.write_str("该 AI 协议驱动未提供所选端点"),
            Self::EmbeddingsNotSupported => formatter.write_str("该 AI 连接不支持 Embeddings"),
            Self::Transport => formatter.write_str("AI 供应商请求失败"),
            Self::Timeout => formatter.write_str("AI 供应商请求超时"),
            Self::ProviderStatus(status) => write!(formatter, "AI 供应商返回 HTTP {status}"),
            Self::InvalidResponse => formatter.write_str("AI 供应商响应格式无效"),
            Self::Refused => formatter.write_str("AI 供应商拒绝了生成请求"),
            Self::FakeExhausted => formatter.write_str("AI Fake 响应序列已耗尽"),
        }
    }
}

impl std::error::Error for AiError {}

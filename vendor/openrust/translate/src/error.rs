use std::fmt;

pub type TranslationResult<T> = Result<T, TranslationError>;

/// 对外只保留错误类别和可公开的 HTTP 状态，不携带供应商响应或原译文。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TranslationError {
    EmptyText,
    InvalidLanguage,
    UnsupportedLanguage,
    AutoDetectionUnsupported,
    Unavailable,
    DefaultNotConfigured,
    ProfileNotConfigured,
    DriverNotRegistered,
    InvalidConfiguration(&'static str),
    Rejected,
    Authentication,
    Quota,
    RateLimited,
    Timeout,
    Transport,
    Service { status: Option<u16> },
    InvalidResponse,
}

impl fmt::Display for TranslationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyText => "翻译原文不能为空白。",
            Self::InvalidLanguage => "无效的 BCP 47 语言标签。",
            Self::UnsupportedLanguage => "翻译服务商不支持所选语言。",
            Self::AutoDetectionUnsupported => "翻译服务商需要显式指定源语言。",
            Self::Unavailable => "应用未装配文本翻译服务。",
            Self::DefaultNotConfigured => "未配置默认文本翻译服务。",
            Self::ProfileNotConfigured => "找不到具名文本翻译配置。",
            Self::DriverNotRegistered => "指定的文本翻译驱动未注册。",
            Self::InvalidConfiguration(field) => {
                return write!(formatter, "无效的文本翻译配置字段：{field}");
            }
            Self::Rejected => "文本翻译请求被拒绝。",
            Self::Authentication => "文本翻译服务鉴权失败。",
            Self::Quota => "文本翻译服务的使用额度已用尽。",
            Self::RateLimited => "文本翻译服务限制了请求速率。",
            Self::Timeout => "文本翻译请求超时，服务商执行结果未知。",
            Self::Transport => "文本翻译请求传输失败，服务商执行结果未知。",
            Self::Service {
                status: Some(status),
            } => return write!(formatter, "文本翻译服务失败（HTTP {status}）。"),
            Self::Service { status: None } => "文本翻译服务失败。",
            Self::InvalidResponse => "文本翻译服务返回了无效响应。",
        })
    }
}

impl std::error::Error for TranslationError {}

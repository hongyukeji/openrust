use std::fmt;

use crate::localization;

pub type AiResult<T> = Result<T, AiError>;

pub(crate) enum ProviderAttemptError {
    BeforeCommit(AiError),
    Committed(AiError),
}

impl ProviderAttemptError {
    pub(crate) fn into_error(self) -> AiError {
        match self {
            Self::BeforeCommit(error) | Self::Committed(error) => error,
        }
    }
}

impl From<AiError> for ProviderAttemptError {
    fn from(error: AiError) -> Self {
        Self::Committed(error)
    }
}

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AiError {
    Unavailable,
    DefaultNotConfigured,
    ProviderNotConfigured,
    ProviderNameTaken,
    DriverNotRegistered,
    InvalidConfiguration(&'static str),
    InvalidPrompt,
    InvalidGenerationOptions,
    GenerationOptionsNotSupported,
    InvalidProviderOptions,
    ProviderOptionsNotSupported,
    InvalidConversationParticipant,
    ConversationNotFound,
    ConversationStorageUnavailable,
    MissingModel,
    MissingCredential,
    ModelsNotSupported,
    MessagesNotSupported,
    AttachmentsNotSupported,
    TooManyAttachments,
    InvalidDocument,
    StructuredOutputNotSupported,
    InvalidSchema,
    InvalidTools,
    InvalidToolChoice,
    InvalidApprovalDecisions,
    ApprovalNotFound,
    ApprovalExpired,
    ApprovalConsumed,
    ApprovalPaused(crate::PendingToolApprovals),
    ApprovalNotResumable {
        pending: Vec<crate::PendingApproval>,
    },
    InvalidToolArguments,
    InvalidSkill,
    McpToolCallFailed,
    SimilaritySearchFailed,
    ToolsNotSupported,
    InvalidStructuredResponse,
    StreamingNotSupported,
    StreamingMiddlewareNotSupported,
    EndpointNotSupported,
    EmbeddingsNotSupported,
    RerankingNotSupported,
    ClassificationNotSupported,
    InvalidClassification,
    ImagesNotSupported,
    InvalidImage,
    AudioNotSupported,
    TranscriptionNotSupported,
    InvalidAudio,
    FilesNotSupported,
    InvalidFile,
    FileNotDownloadable,
    StoresNotSupported,
    InvalidStore,
    StoreSourceFileUnavailable,
    StoreImportFailed(i32),
    StorePopulationFailed {
        store_id: String,
        source: Box<AiError>,
    },
    ResourceScopeMismatch,
    DetachedFileDeletionFailed {
        file_id: String,
        source: Box<AiError>,
    },

    MediaTooLarge,
    MediaStorageUnavailable,
    EmbeddingCacheUnavailable,
    Transport,
    ProviderConnectionFailed,
    Timeout,
    ProviderStatus(u16),
    ProviderOverloaded(u16),
    ProviderInsufficientCredits(u16),
    InvalidResponse,
    Refused,
    FakeExhausted,
    FakeNotInstalled,
}

impl AiError {
    pub(crate) fn allows_provider_failover(&self) -> bool {
        match self {
            Self::ProviderConnectionFailed
            | Self::ProviderOverloaded(_)
            | Self::ProviderInsufficientCredits(_) => true,
            Self::ProviderStatus(status) => {
                matches!(status, 402 | 429) || provider_overloaded(*status)
            }
            _ => false,
        }
    }
}

pub(crate) fn provider_overloaded(status: u16) -> bool {
    matches!(status, 502 | 503 | 504 | 520 | 522 | 524)
}

impl fmt::Display for AiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("当前应用未装配 AI SDK"),
            Self::DefaultNotConfigured => formatter.write_str("未配置 AI 默认连接"),
            Self::ProviderNotConfigured => formatter.write_str("AI 连接未配置"),
            Self::ProviderNameTaken => formatter.write_str("AI 连接名称已被保留或配置"),
            Self::DriverNotRegistered => formatter.write_str("AI 协议驱动未注册"),
            Self::InvalidConfiguration(field) => write!(formatter, "AI 配置字段无效：{field}"),
            Self::InvalidPrompt => formatter.write_str("AI 提示词不能为空"),
            Self::InvalidGenerationOptions => formatter.write_str("AI 文本生成选项超出允许范围"),
            Self::GenerationOptionsNotSupported => {
                formatter.write_str("该 AI 协议驱动不支持文本生成选项")
            }
            Self::InvalidProviderOptions => formatter.write_str("AI 供应商选项含无效或保留字段"),
            Self::ProviderOptionsNotSupported => {
                formatter.write_str("该 AI 协议驱动不支持供应商扩展选项")
            }
            Self::InvalidConversationParticipant => formatter.write_str("AI 会话参与者无效"),
            Self::ConversationNotFound => formatter.write_str("未找到当前参与者的 AI 会话"),
            Self::ConversationStorageUnavailable => formatter.write_str("AI 会话存储暂不可用"),
            Self::MissingModel => formatter.write_str("请选择 AI 模型，或为当前连接配置默认模型"),
            Self::MissingCredential => formatter.write_str("AI 连接的环境凭证未配置"),
            Self::ModelsNotSupported => formatter.write_str("该 AI 协议驱动未提供模型目录"),
            Self::AttachmentsNotSupported => formatter.write_str("该 AI 协议驱动不支持提示词附件"),
            Self::TooManyAttachments => formatter.write_str("AI 提示词附件数量超出限制"),
            Self::InvalidDocument => formatter.write_str("AI 文档名称或格式无效"),
            Self::MessagesNotSupported => formatter.write_str("该 AI 协议驱动未接收历史消息"),
            Self::StructuredOutputNotSupported => {
                formatter.write_str("该 AI 协议驱动未提供结构化输出")
            }
            Self::InvalidSchema => formatter.write_str("AI 结构化输出模式无效"),
            Self::InvalidTools => formatter.write_str("AI 工具名称无效或重复"),
            Self::InvalidToolChoice => formatter.write_str("AI 工具选择与当前步骤的工具不匹配"),
            Self::InvalidApprovalDecisions => {
                approval_error(formatter, "invalid_decisions", "AI 工具审批决定无效")
            }
            Self::ApprovalNotFound => {
                approval_error(formatter, "not_found", "未找到当前会话的 AI 工具审批")
            }
            Self::ApprovalExpired => approval_error(formatter, "expired", "AI 工具审批已过期"),
            Self::ApprovalConsumed => {
                approval_error(formatter, "consumed", "AI 工具审批已提交或正在执行")
            }
            Self::ApprovalPaused(_) => approval_error(formatter, "paused", "AI 工具执行等待审批"),
            Self::ApprovalNotResumable { .. } => approval_error(
                formatter,
                "not_resumable",
                "该工具须经审批，当前调用没有审批恢复入口",
            ),
            Self::InvalidToolArguments => formatter.write_str("AI 工具参数无效"),
            Self::InvalidSkill => formatter.write_str("AI 技能文档无效或无法读取"),
            Self::McpToolCallFailed => formatter.write_str("MCP 工具调用失败"),
            Self::SimilaritySearchFailed => formatter.write_str("AI 相似度搜索查询失败"),
            Self::ToolsNotSupported => formatter.write_str("当前 AI 连接或调用方式未提供工具执行"),
            Self::InvalidStructuredResponse => formatter.write_str("AI 响应不是预期的 JSON 对象"),
            Self::StreamingNotSupported => formatter.write_str("该 AI 协议驱动未提供流式输出"),
            Self::StreamingMiddlewareNotSupported => {
                formatter.write_str("流式生成尚未接入 Agent 中间件")
            }
            Self::EndpointNotSupported => formatter.write_str("该 AI 协议驱动未提供所选端点"),
            Self::EmbeddingsNotSupported => formatter.write_str("该 AI 连接不支持 Embeddings"),
            Self::RerankingNotSupported => formatter.write_str("该 AI 连接不支持 Reranking"),
            Self::ClassificationNotSupported => formatter.write_str("该 AI 连接不支持分类"),
            Self::InvalidClassification => formatter.write_str("AI 分类的输入、问题或超时无效"),
            Self::ImagesNotSupported => formatter.write_str("该 AI 连接不支持图像生成"),
            Self::InvalidImage => formatter.write_str("AI 图像格式或大小无效"),
            Self::AudioNotSupported => formatter.write_str("该 AI 连接不支持语音生成"),
            Self::TranscriptionNotSupported => formatter.write_str("该 AI 连接不支持语音转录"),
            Self::InvalidAudio => formatter.write_str("AI 音频格式或大小无效"),
            Self::FilesNotSupported => formatter.write_str("该 AI 连接不支持文件操作"),
            Self::InvalidFile => formatter.write_str("AI 文件名称、标识或内容无效"),
            Self::FileNotDownloadable => formatter.write_str("AI 供应商未开放此文件的下载"),
            Self::StoresNotSupported => formatter.write_str("该 AI 连接不支持向量仓库"),
            Self::InvalidStore => formatter.write_str("AI 向量仓库配置无效"),
            Self::StoreSourceFileUnavailable => formatter.write_str("AI 向量仓库未返回原文件 ID"),
            Self::StoreImportFailed(code) => {
                write!(formatter, "AI 向量仓库导入失败，供应商状态码为 {code}")
            }
            Self::StorePopulationFailed { source, .. } => {
                write!(formatter, "AI 向量仓库已创建，预填文件失败：{source}")
            }
            Self::ResourceScopeMismatch => formatter.write_str("AI 资源不属于当前连接或仓库"),
            Self::DetachedFileDeletionFailed { source, .. } => {
                write!(formatter, "文件已移出向量仓库，删除原文件失败：{source}")
            }

            Self::MediaTooLarge => formatter.write_str("AI 媒体超出大小限制"),
            Self::MediaStorageUnavailable => formatter.write_str("AI 媒体存储暂不可用"),
            Self::EmbeddingCacheUnavailable => formatter.write_str("Embeddings 缓存暂不可用"),
            Self::Transport | Self::ProviderConnectionFailed => {
                formatter.write_str("AI 供应商请求失败")
            }
            Self::Timeout => formatter.write_str("AI 供应商请求超时"),
            Self::ProviderStatus(status)
            | Self::ProviderOverloaded(status)
            | Self::ProviderInsufficientCredits(status) => {
                write!(formatter, "AI 供应商返回 HTTP {status}")
            }
            Self::InvalidResponse => formatter.write_str("AI 供应商响应格式无效"),
            Self::Refused => formatter.write_str("AI 供应商拒绝了生成请求"),
            Self::FakeNotInstalled => formatter.write_str("尚未启用 AI 替身"),
            Self::FakeExhausted => formatter.write_str("AI Fake 响应序列已耗尽"),
        }
    }
}

fn approval_error(formatter: &mut fmt::Formatter<'_>, key: &str, fallback: &str) -> fmt::Result {
    formatter.write_str(&localization::text(
        &format!("ai::errors.approval.{key}"),
        &Default::default(),
        fallback,
    ))
}

impl std::error::Error for AiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DetachedFileDeletionFailed { source, .. }
            | Self::StorePopulationFailed { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

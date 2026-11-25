use openrust::http::HttpFuture;

use crate::{Assessment, ImageInput, TaskId, TaskStatus, TextInput, VideoInput};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Text,
    Image,
    SubmitVideo,
    QueryVideo,
    CancelVideo,
}

/// 错误只包含稳定的协议分类或静态字段名，避免附带原文、签名和密钥。
#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    #[error("内容审核输入无效：{0}")]
    InvalidInput(&'static str),
    #[error("内容审核配置无效：{0}")]
    InvalidConfiguration(&'static str),
    #[error("内容审核配置未定义")]
    ProfileNotConfigured,
    #[error("内容审核配置修订已移除或不可用")]
    ProfileRevisionUnavailable,
    #[error("内容审核配置修订与原任务不一致或版本不受支持")]
    ProfileRevisionChanged,
    #[error("相同的审核幂等键已关联另一份请求")]
    ReviewConflict,
    #[error("当前应用和租户内未找到审核记录")]
    ReviewNotFound,
    #[error("内容审核记录读写失败")]
    ReviewStorage,
    #[error("内容审核恢复任务派发失败")]
    RecoveryDispatch,
    #[error("内容审核默认配置未指定")]
    DefaultNotConfigured,
    #[error("内容审核驱动未注册")]
    DriverNotRegistered,
    #[error("当前应用的内容审核管理器不可用")]
    Unavailable,
    #[error("内容审核驱动不支持操作 {0:?}")]
    Unsupported(Operation),
    #[error("内容审核服务未授权")]
    Unauthorized,
    #[error("内容审核供应商拒绝请求")]
    Rejected,
    #[error("内容审核请求已被限流")]
    Throttled,
    #[error("内容审核供应商暂不可用")]
    ServiceUnavailable,
    #[error("内容审核请求超时")]
    Timeout,
    #[error("内容审核供应商响应不符合协议")]
    Protocol,
    #[error("无法为指定版本的私有素材生成送审地址")]
    MediaUnavailable,
    #[error("提交结果未知，须核对原任务后再重试")]
    SubmissionUnknown,
}

pub type ModerationResult<T> = Result<T, ModerationError>;
pub type ModerationFuture<'a, T> = HttpFuture<'a, ModerationResult<T>>;

/// 部分能力的驱动只需声明和实现自己支持的操作。远端任务 ID 必须可跨进程恢复。
pub trait ModerationDriver: Send + Sync + 'static {
    fn supports(&self, operation: Operation) -> bool;

    fn text<'a>(&'a self, _: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        unsupported(Operation::Text)
    }

    fn image<'a>(&'a self, _: &'a ImageInput) -> ModerationFuture<'a, Assessment> {
        unsupported(Operation::Image)
    }

    fn submit_video<'a>(&'a self, _: &'a VideoInput) -> ModerationFuture<'a, TaskId> {
        unsupported(Operation::SubmitVideo)
    }

    fn query_video<'a>(&'a self, _: &'a TaskId) -> ModerationFuture<'a, TaskStatus> {
        unsupported(Operation::QueryVideo)
    }

    fn cancel_video<'a>(&'a self, _: &'a TaskId) -> ModerationFuture<'a, ()> {
        unsupported(Operation::CancelVideo)
    }
}

fn unsupported<T>(operation: Operation) -> ModerationFuture<'static, T> {
    Box::pin(async move { Err(ModerationError::Unsupported(operation)) })
}

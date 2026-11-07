//! 自定义原子存储的公开领域失败；第三方无需猜测默认DB的内部错误码。

/// 本次尝试生成的新标识符种类；不表示旧凭据或任意SQL唯一约束。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantIdentifierKind {
    /// 新授权码。
    Code,
    /// 新access token。
    Access,
    /// 新refresh token。
    Refresh,
}

/// 原子存储的可区分失败；基础设施错误不冒充不存在、撤销或凭据无效。
pub enum GrantStorageError {
    /// 当前客户端已撤销、消失或不再匹配预检快照。
    ClientChanged,
    /// 授权码不存在或已消费。
    CodeRevoked,
    /// 刷新记录不存在或已撤销。
    RefreshRevoked,
    /// 最后受信时间检查已超过适用期限。
    Expired,
    /// subject、client、scope或记录关联不匹配。
    BindingMismatch,
    /// 同原子context中的委托权威明确拒绝。
    AuthorityDenied,
    /// 本次新标识符碰撞，且可信后端确认未提交或已完整回滚。
    ///
    /// 必须没有消费旧凭据或遗留新记录；提交结果未知不能返回此变体。
    /// 仅用于准确对应本次新记录的kind，不得从普通SQL错误字符串猜测。
    IdentifierCollision(GrantIdentifierKind),
    /// 基础设施或内部失败；仅可信诊断读取source，不向协议响应回显。
    Unavailable(openrust::Error),
}

impl GrantStorageError {
    /// 只在默认DB适配器内部桥接旧实现的稳定代码；不作为第三方仓储契约。
    pub(crate) fn from_framework(error: openrust::Error) -> Self {
        match error.code() {
            "passport_client_changed" => Self::ClientChanged,
            "passport_code_revoked" => Self::CodeRevoked,
            "passport_refresh_revoked" => Self::RefreshRevoked,
            "passport_grant_expired" => Self::Expired,
            "passport_grant_binding_invalid" => Self::BindingMismatch,
            "passport_authority_rejected" => Self::AuthorityDenied,
            _ => Self::Unavailable(error),
        }
    }
}

impl std::fmt::Display for GrantStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ClientChanged => "OAuth客户端状态已改变",
            Self::CodeRevoked => "OAuth授权码不存在或已消费",
            Self::RefreshRevoked => "OAuth刷新记录不存在或已撤销",
            Self::Expired => "OAuth授权凭据已过期",
            Self::BindingMismatch => "OAuth授权关联不匹配",
            Self::AuthorityDenied => "OAuth委托权威拒绝操作",
            Self::IdentifierCollision(_) => "OAuth新标识符碰撞",
            Self::Unavailable(_) => "OAuth存储不可用",
        })
    }
}

impl std::fmt::Debug for GrantStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for GrantStorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

/// 原子存储公开结果；与协议输出和默认框架内部错误分别承担职责。
pub type GrantStorageResult<T> = std::result::Result<T, GrantStorageError>;

impl From<openrust::Error> for GrantStorageError {
    /// 通用框架错误默认归基础设施；领域失败应明确选择公开enum，不猜测字符串。
    fn from(error: openrust::Error) -> Self {
        Self::Unavailable(error)
    }
}

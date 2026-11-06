//! 模型通过同一账户对象公开的双因素持久状态。

use std::fmt;

use chrono::{DateTime, Utc};

use crate::{Result, auth::Authenticatable, core::BoxFuture, security::Encryptor};

/// 对应官方账户上的三项双因素属性；`secret` 与 `recovery_codes` 必须是密文。
///
/// 条件更新比较当前快照，不能据此承诺跨一次Disable后回到空值的永久代际隔离。
#[derive(Clone, Default, PartialEq, Eq)]
pub struct TwoFactorState {
    /// `two_factor_secret` 的加密值。
    pub secret: Option<String>,
    /// `two_factor_recovery_codes` 的加密JSON数组。
    pub recovery_codes: Option<String>,
    /// `two_factor_confirmed_at`，未确认时为None。
    pub confirmed_at: Option<DateTime<Utc>>,
}

impl TwoFactorState {
    /// 对应模型hasEnabledTwoFactorAuthentication的非null与确认判断；登录Redirect还检查PHP真值。
    pub fn is_enabled(&self, confirm: bool) -> bool {
        self.secret.is_some() && (!confirm || self.confirmed_at.is_some())
    }
}

impl fmt::Debug for TwoFactorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwoFactorState")
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .field(
                "recovery_codes",
                &self.recovery_codes.as_ref().map(|_| "[REDACTED]"),
            )
            .field("confirmed_at", &self.confirmed_at)
            .finish()
    }
}

/// 对应Fortify模型trait；不要求Eloquent、整数主键或全局必填邮箱。
///
/// 模型应将读写委托给自己的现有持久来源。CAS是Rust异步并发适配：一次条件
/// 更新不能拆成先读后无条件写；未知数据库写结果不能伪称回滚成功。
pub trait TwoFactorAuthenticatable: Authenticatable {
    /// 显式选择旧/新数据库因子的统一状态与版本策略；官方三字段模型默认不启用。
    ///
    /// 适配器必须对应本模型已有持久来源，不能按相同账户ID借用另一个Provider。
    fn two_factor_authority(&self) -> Option<&super::DatabaseFactorAuthority> {
        None
    }
    /// 从账户持久来源读取当前三字段快照，不使用跨请求缓存。
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>>;
    /// 仅当前状态等于expected时替换，竞争失败返回false，存储错误原样传播。
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>>;
    /// 返回指定登录字段的二维码账户标签，缺字段时由应用选择可用标识。
    fn two_factor_account_name(&self, username: &str) -> Result<Option<String>>;
    /// 模型专用加密器；Fortify显式覆盖优先于此值，其次使用默认Crypt。
    fn two_factor_encrypter(&self) -> Option<Encryptor> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::TwoFactorState;

    #[test]
    fn confirmation_flag_changes_challenge_eligibility_without_erasing_secret() {
        let state = TwoFactorState {
            secret: Some("private-secret-ciphertext".into()),
            recovery_codes: Some("private-recovery-ciphertext".into()),
            confirmed_at: None,
        };
        assert!(state.is_enabled(false));
        assert!(!state.is_enabled(true));
        let debug = format!("{state:?}");
        assert!(!debug.contains("private-secret"));
        assert!(!debug.contains("private-recovery"));
        assert!(debug.contains("REDACTED"));
    }
}

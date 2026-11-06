//! 单次Provider验证的窗口与计数事实，不是可直接建立Guard身份的认证收据。

use crate::{Error, Result};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

/// 同一次验证码校验产生的实际窗口与匹配计数。
///
/// 自定义Provider须报告实际计数、窗口及周期，或委托其内层Provider；
/// 不能用Actions配置或校验结束后的时钟猜测已经发生的验证。
/// 此值只提供验证事实，账户绑定、当前代际、一次消费与Guard登录仍由框架核验。
#[derive(Clone)]
pub struct TwoFactorCodeVerification {
    code_hash: Option<[u8; 32]>,
    window: u64,
    current_counter: i64,
    matched_counter: i64,
    period_seconds: u64,
    replay_lifetime_seconds: u64,
    observed_at: Option<std::time::Instant>,
}
impl TwoFactorCodeVerification {
    /// 返回未通过验证的结果；不携带可用于消费的验证码事实。
    #[must_use]
    pub const fn rejected() -> Self {
        Self {
            code_hash: None,
            window: 0,
            current_counter: 0,
            matched_counter: 0,
            period_seconds: 0,
            replay_lifetime_seconds: 0,
            observed_at: None,
        }
    }
    /// 记录实际成功校验的验证码、窗口及匹配计数，不执行第二次校验。
    ///
    /// `current_counter`是该次校验读取的Unix秒/30，不是之后写缓存时的时钟；
    /// `matched_counter`须在实际窗口内。负候选保留Google2FA低32位计数映射。
    /// 空验证码、负当前计数、窗口/范围溢出或越界匹配均明确拒绝。
    pub fn verified(
        code: &str,
        window: u64,
        current_counter: i64,
        matched_counter: i64,
    ) -> Result<Self> {
        Self::verified_with_period(code, window, current_counter, matched_counter, 30)
    }
    /// 为实际使用其他周期的Provider记录同一次验证事实；默认Google2FA使用30秒。
    ///
    /// Provider须保证此次接受域可由该窗口/周期覆盖；周期非零，消费保留期
    /// `(2 * window + 3) * period_seconds`必须可表示。当前计数使用该Provider的时间域。
    pub fn verified_with_period(
        code: &str,
        window: u64,
        current_counter: i64,
        matched_counter: i64,
        period_seconds: u64,
    ) -> Result<Self> {
        let replay_lifetime_seconds = window
            .checked_mul(2)
            .and_then(|n| n.checked_add(3))
            .and_then(|n| n.checked_mul(period_seconds))
            .filter(|n| *n != 0)
            .ok_or_else(invalid)?;
        let signed_window = i64::try_from(window).map_err(|_| invalid())?;
        let start = current_counter
            .checked_sub(signed_window)
            .ok_or_else(invalid)?;
        let end = current_counter
            .checked_add(signed_window)
            .ok_or_else(invalid)?;
        if code.is_empty() || current_counter < 0 || !(start..=end).contains(&matched_counter) {
            return Err(invalid());
        }
        Ok(Self {
            code_hash: Some(Sha256::digest(code.as_bytes()).into()),
            window,
            current_counter,
            matched_counter,
            period_seconds,
            replay_lifetime_seconds,
            observed_at: Some(std::time::Instant::now()),
        })
    }
    /// 是否实际通过本次验证码校验；不表示账户当前仍可登录。
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.code_hash.is_some()
    }
    /// 成功校验使用的实际相邻时间步窗口。
    #[must_use]
    pub const fn window(&self) -> Option<u64> {
        if self.is_valid() {
            Some(self.window)
        } else {
            None
        }
    }
    /// 成功校验读取的当前计数，不被后续缓存时钟覆盖。
    #[must_use]
    pub const fn current_counter(&self) -> Option<i64> {
        if self.is_valid() {
            Some(self.current_counter)
        } else {
            None
        }
    }
    /// 实际匹配的计数，供同一次原子消费核对。
    #[must_use]
    pub const fn matched_counter(&self) -> Option<i64> {
        if self.is_valid() {
            Some(self.matched_counter)
        } else {
            None
        }
    }
    /// 实际验证码周期；拒绝结果不携带消费期限。
    #[must_use]
    pub const fn period_seconds(&self) -> Option<u64> {
        if self.is_valid() {
            Some(self.period_seconds)
        } else {
            None
        }
    }
    /// 该次窗口/周期对应的有限原子消费保留期，不承诺覆盖未来任意扩窗或时钟回退。
    #[must_use]
    pub const fn replay_lifetime_seconds(&self) -> Option<u64> {
        if self.is_valid() {
            Some(self.replay_lifetime_seconds)
        } else {
            None
        }
    }
    pub(crate) fn require_code(&self, code: &str) -> Result<i64> {
        // 验证事实只在一个实际周期内可消费；等待不能跨过nonce账本期限重新取得授权。
        if self.observed_at.is_none_or(|observed| {
            observed.elapsed() >= std::time::Duration::from_secs(self.period_seconds)
        }) {
            return Err(Error::Unauthorized);
        }
        let expected = self.code_hash.as_ref().ok_or_else(invalid)?;
        let actual: [u8; 32] = Sha256::digest(code.as_bytes()).into();
        if !bool::from(expected.ct_eq(&actual)) {
            return Err(invalid());
        }
        Ok(self.matched_counter)
    }
}
impl std::fmt::Debug for TwoFactorCodeVerification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwoFactorCodeVerification")
            .finish_non_exhaustive()
    }
}
fn invalid() -> Error {
    Error::custom(
        "fortify_verification_evidence_invalid",
        "验证码验证事实缺失、范围无效或与本次验证码不一致",
    )
}

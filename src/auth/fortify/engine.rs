//! 固定 Fortify / `Google2FA` 语义的验证码引擎与可替换账户无关接口。

use super::TwoFactorCodeVerification;
use crate::auth::totp::totp_code;
use crate::config::repository::ConfigRepository;
use crate::{Error, Result, cache::Cache, core::BoxFuture};
use md5_11::{Digest, Md5};
use std::sync::atomic::{AtomicI64, Ordering};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const PERIOD: u64 = 30;

/// Fortify 的对象安全双因素验证 Provider；不负责账户存储、HTTP 或原子防重放。
pub trait TwoFactorAuthenticationProvider: Send + Sync {
    /// 生成指定字符数的无填充 Base32 密钥；Fortify 调用方默认传入 16。
    fn generate_secret_key(&self, length: usize) -> Result<String>;
    /// 返回供二维码渲染器使用的完整 provisioning URI。
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String>;
    /// 校验Provider的原始验证码；内置Google为六位并保留前导零，缓存错误不能成为未命中。
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.verify_with_context(secret, code)
                .await
                .map(|fact| fact.is_valid())
        })
    }
    /// 在同一次验证中返回实际窗口/时钟/匹配计数；自定义包装器应委托内层同一方法。
    ///
    /// 布尔值无法表达防重放账本与旧种子下界所需事实；实现者不能事后按配置猜测，
    /// 也不能先调用verify再重新校验而重复消费缓存。`verify`保留官方布尔查询门面。
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<TwoFactorCodeVerification>>;
}

/// SHA1 / 六位 / 30 秒的 `Google2FA` 兼容实现。
///
/// 默认窗口为 1；`None` 缓存允许重复验证码。有缓存时按官方 get/verify/put 执行，
/// 不承诺并发单次消费。窗口属于受信任部署配置；运算溢出与内存分配失败明确返回错误。
pub struct GoogleTwoFactorAuthenticationProvider {
    cache: Option<Cache>,
    clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>,
    window: AtomicI64,
    config: Option<ConfigRepository>,
}
impl fmt::Debug for GoogleTwoFactorAuthenticationProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleTwoFactorAuthenticationProvider")
            .field("window", &self.window.load(Ordering::Relaxed))
            .field("cache_enabled", &self.cache.is_some())
            .finish_non_exhaustive()
    }
}
impl GoogleTwoFactorAuthenticationProvider {
    /// 使用可选的框架缓存构造；`None` 明确关闭官方验证码缓存。
    #[must_use]
    pub fn new(cache: Option<Cache>) -> Self {
        Self {
            cache,
            clock: Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|time| time.as_secs())
                    .map_err(|_| time_error())
            }),
            window: AtomicI64::new(1),
            config: None,
        }
    }
    /// 替换 Unix 秒时钟，用于确定性验证；错误原样传播。
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }
    /// 设置验证码窗口；零只校验当前步，负数按上游执行空扫描。
    pub fn with_window(mut self, window: i64) -> Result<Self> {
        self.window = AtomicI64::new(window);
        Ok(self)
    }
    /// 每次验证读取同一配置仓储的整数window；null/缺失/非整数沿用本实例最后的窗口。
    #[must_use]
    pub fn with_config_repository(mut self, config: ConfigRepository) -> Self {
        self.config = Some(config);
        self
    }
    fn current_window(&self) -> Result<i64> {
        if let Some(config) = &self.config
            && let Some(window) = config
                .get("fortify-options.two-factor-authentication.window")?
                .and_then(|value| value.as_i64())
        {
            self.window.store(window, Ordering::Relaxed);
            return Ok(window);
        }
        Ok(self.window.load(Ordering::Relaxed))
    }
    async fn verify_code(&self, secret: &str, code: &str) -> Result<TwoFactorCodeVerification> {
        // 与上游一样先读取window，再get缓存；本次快照不受等待期间的后续配置修改影响。
        let window = self.current_window()?;
        let key = cache_key(code);
        let old = if let Some(cache) = &self.cache {
            cache.get::<i64>(&key).await?
        } else {
            None
        };
        let current = i64::try_from((self.clock)()? / PERIOD).map_err(|_| time_error())?;
        if window < 0 {
            return Ok(TwoFactorCodeVerification::rejected());
        }
        let end = current.checked_add(window).ok_or_else(time_error)?;
        let mut start = current.checked_sub(window).ok_or_else(time_error)?;
        if let Some(old) = old {
            start = start.max(old.checked_add(1).ok_or_else(time_error)?);
        }
        if start > end {
            return Ok(TwoFactorCodeVerification::rejected());
        }
        let decoded = decode_secret(secret)?;
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(TwoFactorCodeVerification::rejected());
        }
        for step in start..=end {
            // 固定 Google2FA 使用 pack('N*', 0, counter)，包括负候选的低32位映射。
            let bytes = step.to_be_bytes();
            let counter = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            let expected = totp_code(&decoded, u64::from(counter), 6)?;
            if bool::from(expected.as_bytes().ct_eq(code.as_bytes())) {
                // old=null时上游返回true；Fortify随后再次getTimestamp，即使没有Cache也求值。
                let value = if old.is_some() {
                    step
                } else {
                    i64::try_from((self.clock)()? / PERIOD).map_err(|_| time_error())?
                };
                if let Some(cache) = &self.cache {
                    let ttl = u64::try_from(window.max(1))
                        .map_err(|_| time_error())?
                        .checked_mul(60)
                        .ok_or_else(time_error)?;
                    cache
                        .set(&key, &value, Some(Duration::from_secs(ttl)))
                        .await?;
                }
                return TwoFactorCodeVerification::verified(
                    code,
                    u64::try_from(window).map_err(|_| time_error())?,
                    current,
                    step,
                );
            }
        }
        Ok(TwoFactorCodeVerification::rejected())
    }
}
impl TwoFactorAuthenticationProvider for GoogleTwoFactorAuthenticationProvider {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        validate_length(length)?;
        let mut entropy = Zeroizing::new(Vec::new());
        entropy
            .try_reserve_exact(length)
            .map_err(|_| allocation_error())?;
        entropy.resize(length, 0);
        getrandom::fill(&mut entropy).map_err(|_| {
            Error::custom("fortify_totp_entropy_unavailable", "双因素密钥随机源不可用")
        })?;
        let mut secret = String::new();
        secret
            .try_reserve_exact(length)
            .map_err(|_| allocation_error())?;
        secret.extend(
            entropy
                .iter()
                .map(|byte| char::from(ALPHABET[usize::from(byte & 31)])),
        );
        Ok(secret)
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        // 上游仅编码标签，secret 按原值传入；账户 action 在保存密钥时负责其校验。
        let issuer = raw_url_encode(company);
        Ok(format!(
            "otpauth://totp/{issuer}:{}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits=6&period=30",
            raw_url_encode(account)
        ))
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<TwoFactorCodeVerification>> {
        Box::pin(self.verify_code(secret, code))
    }
}

fn validate_length(length: usize) -> Result<()> {
    if length < 16 || !length.is_power_of_two() {
        return Err(Error::custom(
            "fortify_totp_secret_invalid",
            "双因素密钥必须为长度至少16的二次幂 Base32 字符串",
        ));
    }
    Ok(())
}
pub(super) fn decode_secret(secret: &str) -> Result<Zeroizing<Vec<u8>>> {
    validate_length(secret.len())?;
    let capacity = secret.len().checked_mul(5).ok_or_else(allocation_error)? / 8;
    let mut output = Zeroizing::new(Vec::new());
    output
        .try_reserve_exact(capacity)
        .map_err(|_| allocation_error())?;
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for character in secret.bytes() {
        let upper = character.to_ascii_uppercase();
        let value = match upper {
            b'A'..=b'Z' => upper - b'A',
            b'2'..=b'7' => upper - b'2' + 26,
            _ => {
                return Err(Error::custom(
                    "fortify_totp_secret_invalid",
                    "双因素密钥包含非法 Base32 字符",
                ));
            }
        };
        buffer = (buffer << 5) | u16::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits).to_be_bytes()[1]);
            buffer &= (1_u16 << bits) - 1;
        }
    }
    Ok(output)
}
fn cache_key(code: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut key = String::from("fortify.2fa_codes.");
    for byte in Md5::digest(code.as_bytes()) {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 15)]));
    }
    key
}
fn raw_url_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    output
}
fn allocation_error() -> Error {
    Error::custom("fortify_totp_allocation_failed", "双因素密钥内存分配失败")
}
fn time_error() -> Error {
    Error::custom("fortify_totp_time_invalid", "双因素验证时间或窗口溢出")
}

#[cfg(test)]
mod tests;

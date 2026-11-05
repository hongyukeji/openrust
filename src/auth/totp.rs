//! 共享TOTP密码学primitive；密钥强度与重放策略由上层选择。

use crate::{Error, Result};
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

/// RFC 4226 动态截断 primitive；策略层自行限制密钥强度，允许官方 10 字节默认值。
pub(crate) fn totp_code(secret: &[u8], step: u64, digits: u32) -> Result<String> {
    if secret.is_empty() || !(6..=8).contains(&digits) {
        return Err(Error::custom(
            "totp_parameters_invalid",
            "双因素验证码参数无效",
        ));
    }
    let mut mac = Hmac::<Sha1>::new_from_slice(secret)
        .map_err(|_| Error::custom("totp_parameters_invalid", "双因素验证码参数无效"))?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 15);
    let binary = (u32::from(digest[offset] & 127) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!(
        "{:0width$}",
        binary % 10_u32.pow(digits),
        width = digits as usize
    ))
}

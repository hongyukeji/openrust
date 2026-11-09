//! 通用 Eloquent 属性转换，含密码哈希转换。

use crate::{
    Result,
    hashing::{HashManager, HashOptions},
};

/// 将属性转换为哈希字符串；空值旁路，已有合法哈希保持原值，不因成本较低而重新计算。
///
/// hashed cast 对已有哈希只检查驱动配置，
/// 不应用认证输入策略。驱动或配置解析失败向调用方传播。
pub async fn cast_attribute_as_hashed_string(
    hashing: &HashManager,
    value: Option<&str>,
) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !hashing.is_hashed(value)? {
        return hashing
            .make(value.as_bytes(), &HashOptions::new())
            .await
            .map(Some);
    }
    if !hashing.verify_configuration(value)? {
        return Err(crate::Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "model_hash_configuration_invalid",
            "无法验证哈希属性的配置",
        ));
    }
    Ok(Some(value.to_owned()))
}

#[cfg(test)]
#[path = "casts_tests.rs"]
mod tests;

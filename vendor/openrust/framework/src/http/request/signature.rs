use crate::{
    http::HttpError,
    routing::{SignatureMode, SignatureQueryFilter},
    support::facades::Url,
};

use super::Request;

impl Request {
    /// 校验当前请求的签名及有效期；absolute 控制是否将来源纳入签名。
    pub fn has_valid_signature(&self, absolute: bool) -> Result<bool, HttpError> {
        self.has_valid_signature_while_ignoring(&[], absolute)
    }

    /// 校验不包含域名的相对 URL 签名。
    pub fn has_valid_relative_signature(&self) -> Result<bool, HttpError> {
        self.has_valid_signature(false)
    }

    /// 忽略指定查询参数后校验相对签名；被忽略的值不受签名保护。
    pub fn has_valid_relative_signature_while_ignoring<'a>(
        &self,
        ignore: impl Into<SignatureQueryFilter<'a>>,
    ) -> Result<bool, HttpError> {
        self.has_valid_signature_while_ignoring(ignore, false)
    }

    /// 仅使用当前请求的 URL 查询串验签，不将请求正文当作已签名输入。
    pub fn has_valid_signature_while_ignoring<'a>(
        &self,
        ignore: impl Into<SignatureQueryFilter<'a>>,
        absolute: bool,
    ) -> Result<bool, HttpError> {
        Url::has_valid_signature(
            self,
            if absolute {
                SignatureMode::Absolute
            } else {
                SignatureMode::Relative
            },
            ignore.into(),
        )
        .map_err(HttpError::from)
    }
}

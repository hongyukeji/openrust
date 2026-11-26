use crate::{Error, Result};
use aws_lc_rs::hmac;
use http::{HeaderMap, header::HeaderName};
use std::fmt::Write as _;

/// 每次尝试的不可变签名上下文；不实现 Debug 或 Serialize。
pub struct SigningContext<'a> {
    pub webhook_url: &'a str,
    pub body: &'a [u8],
    pub secret: &'a str,
    pub message_id: &'a str,
    pub timestamp: i64,
}
pub trait Signer: Send + Sync {
    fn signature_header_name(&self) -> &str;
    fn calculate_signature(&self, context: &SigningContext<'_>) -> Result<String>;
    fn headers(&self, context: &SigningContext<'_>) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_bytes(self.signature_header_name().as_bytes())
                .map_err(|_| Error::InvalidConfig("signature_header_name"))?,
            self.calculate_signature(context)?
                .parse()
                .map_err(|_| Error::InvalidConfig("signature"))?,
        );
        Ok(headers)
    }
}
pub struct DefaultSigner {
    header: String,
}
impl Default for DefaultSigner {
    fn default() -> Self {
        Self {
            header: "Signature".into(),
        }
    }
}
impl DefaultSigner {
    pub fn new(header: &str) -> Result<Self> {
        HeaderName::from_bytes(header.as_bytes())
            .map_err(|_| Error::InvalidConfig("signature_header_name"))?;
        Ok(Self {
            header: header.into(),
        })
    }
}
impl Signer for DefaultSigner {
    fn signature_header_name(&self) -> &str {
        self.header.as_str()
    }
    fn calculate_signature(&self, context: &SigningContext<'_>) -> Result<String> {
        // 兼容协议仅签正文；URL、时间戳和消息ID不在此默认签名范围。
        let key = hmac::Key::new(hmac::HMAC_SHA256, context.secret.as_bytes());
        let tag = hmac::sign(&key, context.body);
        let mut value = String::with_capacity(64);
        for byte in tag.as_ref() {
            write!(value, "{byte:02x}").expect("写入字符串");
        }
        Ok(value)
    }
}
pub struct StandardWebhookSigner;
impl Signer for StandardWebhookSigner {
    fn signature_header_name(&self) -> &str {
        "webhook-signature"
    }
    fn calculate_signature(&self, context: &SigningContext<'_>) -> Result<String> {
        standardwebhooks::Webhook::new(context.secret)
            .map_err(|_| Error::InvalidConfig("signing_secret"))?
            .sign(context.message_id, context.timestamp, context.body)
            .map_err(|_| Error::InvalidPayload)
    }
    fn headers(&self, context: &SigningContext<'_>) -> Result<HeaderMap> {
        if context.message_id.is_empty()
            || context.message_id.len() > 256
            || !context.message_id.is_ascii()
            || context
                .message_id
                .bytes()
                .any(|byte| byte.is_ascii_control())
            || context.message_id.trim() != context.message_id
        {
            return Err(Error::InvalidConfig("message_id"));
        }
        let mut headers = HeaderMap::new();
        for (key, value) in [
            ("webhook-id", context.message_id.to_owned()),
            ("webhook-timestamp", context.timestamp.to_string()),
            ("webhook-signature", self.calculate_signature(context)?),
        ] {
            headers.insert(
                key,
                value
                    .parse()
                    .map_err(|_| Error::InvalidConfig("signature_headers"))?,
            );
        }
        Ok(headers)
    }
}

use crate::{Error, Result, SignatureValidator, WebhookConfig, WebhookRequest};
use aws_lc_rs::hmac;

pub struct DefaultSignatureValidator;
impl SignatureValidator for DefaultSignatureValidator {
    fn is_valid(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool> {
        if request.body().len() > config.max_body_bytes() {
            return Err(Error::PayloadTooLarge);
        }
        let mut values = request
            .headers()
            .get_all(config.signature_header_name())
            .iter();
        let Some(value) = values.next() else {
            return Ok(false);
        };
        if values.next().is_some() {
            return Ok(false);
        }
        if value.as_bytes().is_empty() || value.as_bytes() == b"0" {
            return Ok(false);
        }
        if config.signing_secret().is_empty() || config.signing_secret() == "0" {
            return Err(Error::InvalidConfig("signing_secret"));
        }
        let Ok(value) = value.to_str() else {
            return Ok(false);
        };
        // 固定协议为小写十六进制；不能把不同大小写的头自动改写成合法签名。
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Ok(false);
        }
        let mut signature = [0u8; 32];
        for (i, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
            signature[i] = (digit(pair[0]) << 4) | digit(pair[1]);
        }
        let mut valid = false;
        for secret in config.secrets() {
            let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
            valid |= hmac::verify(&key, request.body(), &signature).is_ok();
        }
        Ok(valid)
    }
}

pub struct StandardSignatureValidator;
impl StandardSignatureValidator {
    pub fn is_valid_at(
        &self,
        request: &WebhookRequest,
        config: &WebhookConfig,
        now: i64,
    ) -> Result<bool> {
        if request.body().len() > config.max_body_bytes() {
            return Err(Error::PayloadTooLarge);
        }
        for name in ["webhook-id", "webhook-signature", "webhook-timestamp"] {
            if request.headers().get_all(name).iter().count() != 1 {
                return Ok(false);
            }
        }
        if request.headers()["webhook-id"].is_empty() || request.headers()["webhook-id"].len() > 256
        {
            return Ok(false);
        }
        if now.checked_add(300).is_none() || now.checked_sub(300).is_none() {
            return Err(Error::InvalidConfig("timestamp"));
        }
        let timestamp = request.headers()["webhook-timestamp"]
            .to_str()
            .ok()
            .and_then(|s| s.parse::<i64>().ok());
        // 先用宽整数界定协议时间窗，避免上游库在极值时间戳处进行溢出运算。
        if timestamp.is_none_or(|ts| (i128::from(ts) - i128::from(now)).abs() > 300) {
            return Ok(false);
        }
        let mut valid = false;
        for secret in config.secrets() {
            let webhook = standardwebhooks::Webhook::new(secret)
                .map_err(|_| Error::InvalidConfig("signing_secret"))?;
            valid |= webhook
                .verify_with_timestamp(request.body(), request.headers(), now)
                .is_ok();
        }
        Ok(valid)
    }
}
impl SignatureValidator for StandardSignatureValidator {
    fn is_valid(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|source| Error::Runtime {
                source: Box::new(source),
            })?
            .as_secs();
        self.is_valid_at(
            request,
            config,
            i64::try_from(now).map_err(|_| Error::InvalidConfig("timestamp"))?,
        )
    }

    fn verified_event_id<'a>(
        &self,
        request: &'a WebhookRequest,
        _: &WebhookConfig,
    ) -> Result<Option<std::borrow::Cow<'a, str>>> {
        // 固定协议用点分割 ID、时间戳及正文，去重时要求 ID 无分隔歧义。
        let id = request
            .headers()
            .get("webhook-id")
            .and_then(|value| value.to_str().ok())
            .filter(|id| !id.contains('.'))
            .ok_or(Error::MissingVerifiedEventId)?;
        Ok(Some(id.into()))
    }
}

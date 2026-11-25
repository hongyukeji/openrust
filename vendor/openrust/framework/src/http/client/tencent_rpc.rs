//! 腾讯云 TC3 JSON RPC 签名，供使用相同 API 签名协议的服务共享。

use std::fmt::Write;

use aws_lc_rs::{digest, hmac};
use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use url::{Position, Url};
use zeroize::Zeroizing;

use super::{Error, Result};

pub const CONTENT_TYPE: &str = "application/json; charset=utf-8";

pub struct TencentRpcSigner<'a> {
    pub secret_id: &'a str,
    pub secret_key: &'a [u8],
    pub security_token: Option<&'a str>,
}

pub struct TencentRpcOperation<'a> {
    pub service: &'a str,
    pub action: &'a str,
    pub version: &'a str,
    pub region: &'a str,
}

impl TencentRpcSigner<'_> {
    /// 签名与发送方共用完全一致的 JSON 正文及 Content-Type。
    pub fn sign_post(
        &self,
        url: &Url,
        operation: TencentRpcOperation<'_>,
        body: &[u8],
    ) -> Result<HeaderMap> {
        if url.host().is_none() || url.path() != "/" || url.query().is_some() {
            return Err(Error::InvalidRequest);
        }
        let now = jiff::Timestamp::now();
        let timestamp = crate::support::timestamp_seconds(now).to_string();
        let date = now.strftime("%Y-%m-%d").to_string();
        let host = &url[Position::BeforeHost..Position::AfterPort];
        let signed = "content-type;host";
        let canonical = format!(
            "POST\n/\n\ncontent-type:{CONTENT_TYPE}\nhost:{host}\n\n{signed}\n{}",
            sha256(body),
        );
        let scope = format!("{date}/{}/tc3_request", operation.service);
        let to_sign = format!(
            "TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
            sha256(canonical.as_bytes())
        );
        let mut master_key = Zeroizing::new(Vec::with_capacity(3 + self.secret_key.len()));
        master_key.extend_from_slice(b"TC3");
        master_key.extend_from_slice(self.secret_key);
        let date_key = sign(&master_key, &date);
        let service_key = sign(date_key.as_ref(), operation.service);
        let signing_key = sign(service_key.as_ref(), "tc3_request");
        let signature = hex(sign(signing_key.as_ref(), &to_sign).as_ref());
        let authorization = format!(
            "TC3-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",
            self.secret_id,
        );

        let mut headers = HeaderMap::new();
        for (key, value) in [
            ("host", host),
            ("content-type", CONTENT_TYPE),
            ("x-tc-action", operation.action),
            ("x-tc-version", operation.version),
            ("x-tc-timestamp", &timestamp),
            ("x-tc-region", operation.region),
        ] {
            headers.insert(key, header_value(value)?);
        }
        let mut value = header_value(&authorization)?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
        if let Some(token) = self.security_token {
            let mut value = header_value(token)?;
            value.set_sensitive(true);
            headers.insert("x-tc-token", value);
        }
        Ok(headers)
    }
}

fn header_value(value: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(value).map_err(|_| Error::InvalidRequest)
}

fn sha256(value: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, value).as_ref())
}

fn sign(key: &[u8], value: &str) -> hmac::Tag {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), value.as_bytes())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String");
    }
    output
}

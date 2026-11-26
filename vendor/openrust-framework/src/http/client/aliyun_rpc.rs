//! 阿里云 RPC ACS3 请求签名，供使用同一签名协议的服务共享。

use std::{collections::BTreeMap, fmt::Write};

use aws_lc_rs::{digest, hmac};
use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use url::{Position, Url};

use super::{Error, Result};

pub struct AliyunRpcSigner<'a> {
    pub access_key_id: &'a str,
    pub access_key_secret: &'a [u8],
    pub security_token: Option<&'a str>,
}

impl AliyunRpcSigner<'_> {
    /// 为 POST 的实际编码正文与规范查询串生成请求头。请求发送方负责保留相同路径、查询和正文。
    pub fn sign_post(
        &self,
        url: &Url,
        action: &str,
        version: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<HeaderMap> {
        let date = jiff::Timestamp::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let nonce = uuid::Uuid::now_v7().to_string();
        let hash = sha256(body);
        let mut values = BTreeMap::from([
            ("host", &url[Position::BeforeHost..Position::AfterPort]),
            ("x-acs-action", action),
            ("x-acs-content-sha256", hash.as_str()),
            ("x-acs-date", date.as_str()),
            ("x-acs-signature-nonce", nonce.as_str()),
            ("x-acs-version", version),
        ]);
        if let Some(content_type) = content_type {
            values.insert("content-type", content_type);
        }
        if let Some(token) = self.security_token {
            values.insert("x-acs-security-token", token);
        }
        let signed_names = values.keys().copied().collect::<Vec<_>>().join(";");
        let mut canonical = format!(
            "POST\n{}\n{}\n",
            url.path(),
            url.query().unwrap_or_default()
        );
        let mut headers = HeaderMap::new();
        for (name, value) in values {
            writeln!(canonical, "{name}:{value}").expect("writing to String");
            let mut header = HeaderValue::from_str(value).map_err(|_| Error::InvalidRequest)?;
            header.set_sensitive(name == "x-acs-security-token");
            headers.insert(name, header);
        }
        write!(canonical, "\n{signed_names}\n{hash}").expect("writing to String");
        let to_sign = format!("ACS3-HMAC-SHA256\n{}", sha256(canonical.as_bytes()));
        let signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, self.access_key_secret),
            to_sign.as_bytes(),
        );
        let authorization = format!(
            "ACS3-HMAC-SHA256 Credential={},SignedHeaders={signed_names},Signature={}",
            self.access_key_id,
            hex(signature.as_ref()),
        );
        let mut header =
            HeaderValue::from_str(&authorization).map_err(|_| Error::InvalidRequest)?;
        header.set_sensitive(true);
        headers.insert(AUTHORIZATION, header);
        Ok(headers)
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String");
    }
    output
}

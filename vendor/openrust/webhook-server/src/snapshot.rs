use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result, SigningContext,
    body::MAX_BODY_BYTES,
    extensions,
    webhook_call::{validate_headers, validate_secret},
};

pub(crate) const MAX_SNAPSHOT_BYTES: usize = 23 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub version: u8,
    pub uuid: String,
    pub url: String,
    pub signing_url: String,
    pub method: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
    pub credential: Option<String>,
    pub signer: String,
    pub signature_header: String,
    pub timestamp_header: Option<String>,
    pub timeout: u32,
    pub tries: u32,
    pub retryable: Option<Vec<u16>>,
    pub backoff: String,
    pub respect_retry_after: bool,
    pub jitter: bool,
    pub throws_on_failure: bool,
    pub queue: Option<String>,
    pub connection: Option<String>,
    pub job: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom_job: bool,
    pub tags: Vec<String>,
    pub meta: serde_json::Value,
    pub lifetime: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replay: bool,
}

impl Snapshot {
    pub fn encode(&self) -> Result<String> {
        let value = serde_json::to_string(self).map_err(|_| Error::InvalidRecord)?;
        if value.len() > MAX_SNAPSHOT_BYTES {
            return Err(Error::InvalidRecord);
        }
        Ok(value)
    }

    pub fn decode(value: &str) -> Result<Self> {
        if value.len() > MAX_SNAPSHOT_BYTES {
            return Err(Error::InvalidRecord);
        }
        let snapshot: Self = serde_json::from_str(value).map_err(|_| Error::InvalidRecord)?;
        if snapshot.version != 1
            || !(1..=120).contains(&snapshot.timeout)
            || !(1..=100).contains(&snapshot.tries)
            || snapshot.headers.len() > 128
        {
            return Err(Error::InvalidRecord);
        }
        Ok(snapshot)
    }

    pub fn request_parts(
        &self,
        timestamp: i64,
        secret: Option<&str>,
    ) -> Result<(Method, HeaderMap, Vec<u8>)> {
        let method =
            Method::from_bytes(self.method.as_bytes()).map_err(|_| Error::InvalidRecord)?;
        if method == Method::CONNECT {
            return Err(Error::InvalidRecord);
        }
        let bytes = STANDARD
            .decode(&self.body)
            .map_err(|_| Error::InvalidRecord)?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(Error::InvalidPayload);
        }
        let mut headers = HeaderMap::new();
        for (name, value) in &self.headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::InvalidRecord)?;
            let bytes = STANDARD.decode(value).map_err(|_| Error::InvalidRecord)?;
            headers.append(
                name,
                HeaderValue::from_bytes(&bytes).map_err(|_| Error::InvalidRecord)?,
            );
        }
        if self.credential.is_some() {
            let secret = secret.ok_or(Error::InvalidConfig("signing_secret"))?;
            validate_secret(secret)?;
            headers.extend(
                extensions::signer(&self.signer, &self.signature_header)?.headers(
                    &SigningContext {
                        webhook_url: &self.signing_url,
                        body: &bytes,
                        secret,
                        message_id: &self.uuid,
                        timestamp,
                    },
                )?,
            );
        }
        if let Some(name) = &self.timestamp_header {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::InvalidRecord)?,
                HeaderValue::from(timestamp),
            );
        }
        validate_headers(&headers)?;
        let wire = if method == Method::GET { vec![] } else { bytes };
        Ok((method, headers, wire))
    }

    pub fn retries_status(&self, status: StatusCode) -> bool {
        !status.is_success()
            && self
                .retryable
                .as_ref()
                .is_none_or(|codes| codes.contains(&status.as_u16()))
    }
}

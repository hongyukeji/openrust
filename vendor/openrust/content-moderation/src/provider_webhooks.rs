use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, KeyInit, Mac};
use openrust::{container::Container, contracts::container::Container as _};
use openrust_webhook_client::{
    Error, Result, SignatureValidator, WebhookConfig, WebhookRequest, WebhookRuntime,
};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{CredentialReference, ModerationCredentialProvider};

const HEADER: &str = "sightengine-signature";
const TOLERANCE_SECONDS: u64 = 300;
const MAX_SIGNATURES: usize = 8;
const MAX_PREVIOUS_KEYS: usize = 3;

struct SightengineSignature(Vec<Zeroizing<String>>);

impl SignatureValidator for SightengineSignature {
    fn is_valid(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool> {
        if request.body().len() > config.max_body_bytes() {
            return Err(Error::PayloadTooLarge);
        }
        let mut headers = request.headers().get_all(HEADER).iter();
        let Some(value) = headers.next().and_then(|value| value.to_str().ok()) else {
            return Ok(false);
        };
        if headers.next().is_some() || value.len() > 1024 {
            return Ok(false);
        }
        let Some((timestamp, signatures)) = signature_fields(value) else {
            return Ok(false);
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidConfig("sightengine timestamp"))?
            .as_secs();
        if timestamp
            .parse::<u64>()
            .ok()
            .is_none_or(|signed| signed.abs_diff(now) > TOLERANCE_SECONDS)
        {
            return Ok(false);
        }
        let mut valid = false;
        for secret in &self.0 {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|_| Error::InvalidConfig("sightengine callback credential"))?;
            mac.update(timestamp.as_bytes());
            mac.update(b".");
            mac.update(request.body());
            for signature in &signatures {
                valid |= mac.clone().verify_slice(signature).is_ok();
            }
        }
        Ok(valid)
    }

    fn has_signature(&self, request: &WebhookRequest, _: &WebhookConfig) -> bool {
        request.headers().contains_key(HEADER)
    }
}

fn signature_fields(value: &str) -> Option<(&str, Vec<[u8; 32]>)> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for entry in value.split(',') {
        let (name, value) = entry.split_once('=')?;
        let value = value.trim();
        match name.trim() {
            "t" => {
                if timestamp.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return None;
                }
                timestamp = Some(value);
            }
            "v1" => {
                if signatures.len() >= MAX_SIGNATURES {
                    return None;
                }
                signatures.push(signature_bytes(value)?);
            }
            _ => {}
        }
    }
    let timestamp = timestamp?;
    (!signatures.is_empty()).then_some((timestamp, signatures))
}

fn signature_bytes(value: &str) -> Option<[u8; 32]> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    if value.len() != 64 {
        return None;
    }
    let mut decoded = [0; 32];
    for (target, pair) in decoded.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *target = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(decoded)
}

pub(crate) fn register(container: &Container) -> Result<()> {
    WebhookRuntime::register_signature_validator(
        container,
        "moderation-sightengine",
        |config, container| {
            let provider = container
                .make::<dyn ModerationCredentialProvider>()
                .map_err(|_| Error::InvalidConfig("sightengine callback credential provider"))?;
            let parse_reference = |value: &serde_json::Value| {
                serde_json::from_value::<CredentialReference>(value.clone())
                    .map_err(|_| Error::InvalidConfig("sightengine callback credential reference"))
            };
            let mut references = vec![parse_reference(
                &config.options()["moderation_callback_credential"],
            )?];
            if let Some(older) = config
                .options()
                .get("moderation_previous_callback_credentials")
            {
                let older = older
                    .as_array()
                    .filter(|older| older.len() <= MAX_PREVIOUS_KEYS)
                    .ok_or(Error::InvalidConfig(
                        "sightengine previous callback credentials",
                    ))?;
                for value in older {
                    let reference = parse_reference(value)?;
                    if references.contains(&reference) {
                        return Err(Error::InvalidConfig(
                            "sightengine duplicate callback credential",
                        ));
                    }
                    references.push(reference);
                }
            }
            let keys = references
                .into_iter()
                .map(|reference| {
                    let credentials = provider
                        .resolve(&reference)
                        .map_err(|_| Error::InvalidConfig("sightengine callback credential"))?;
                    credentials
                        .get("signing_secret")
                        .filter(|value| {
                            value.starts_with("casec_")
                                && (7..=512).contains(&value.len())
                                && value.bytes().all(|byte| byte.is_ascii_graphic())
                        })
                        .map(|value| Zeroizing::new(value.to_owned()))
                        .ok_or(Error::InvalidConfig("sightengine callback credential"))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(SightengineSignature(keys)))
        },
    )
}

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, KeyInit, Mac, digest::CtOutput};
use openrust::{container::Container, contracts::container::Container as _};
use openrust_webhook_client::{
    Error, Result, SignatureValidator, WebhookConfig, WebhookRequest, WebhookRuntime,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    CredentialReference, ModerationCredentialProvider,
    aliyun_callback::CallbackForm,
    drivers::{valid_aliyun_callback_seed, valid_tencent_callback_seed},
};

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

const TENCENT_HEADER: &str = "x-signature";

struct TencentSignature(Vec<Zeroizing<String>>);

struct AliyunSignature {
    account_uid: String,
    seeds: Vec<Zeroizing<String>>,
}

impl SignatureValidator for AliyunSignature {
    fn is_valid(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool> {
        if request.body().len() > config.max_body_bytes() {
            return Err(Error::PayloadTooLarge);
        }
        if request.headers().get("content-type").is_none_or(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.split(';').next())
                .is_none_or(|value| {
                    !value
                        .trim()
                        .eq_ignore_ascii_case("application/x-www-form-urlencoded")
                })
        }) {
            return Ok(false);
        }
        let Some(form) = CallbackForm::parse(request.body()) else {
            return Ok(false);
        };
        let Some(signature) = signature_bytes(&form.checksum) else {
            return Ok(false);
        };
        Ok(sha256_matches(
            self.account_uid.as_bytes(),
            &self.seeds,
            form.content.as_bytes(),
            signature,
        ))
    }

    fn has_signature(&self, request: &WebhookRequest, _: &WebhookConfig) -> bool {
        CallbackForm::contains_checksum(request.body())
    }
}

impl SignatureValidator for TencentSignature {
    fn is_valid(&self, request: &WebhookRequest, config: &WebhookConfig) -> Result<bool> {
        if request.body().len() > config.max_body_bytes() {
            return Err(Error::PayloadTooLarge);
        }
        let mut signatures = request.headers().get_all(TENCENT_HEADER).iter();
        let Some(signature) = signatures
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(signature_bytes)
        else {
            return Ok(false);
        };
        if signatures.next().is_some() {
            return Ok(false);
        }
        Ok(sha256_matches(b"", &self.0, request.body(), signature))
    }

    fn has_signature(&self, request: &WebhookRequest, _: &WebhookConfig) -> bool {
        request.headers().contains_key(TENCENT_HEADER)
    }
}

fn sha256_matches(
    prefix: &[u8],
    seeds: &[Zeroizing<String>],
    body: &[u8],
    signature: [u8; 32],
) -> bool {
    let received = CtOutput::<Sha256>::new(signature.into());
    let mut valid = false;
    for seed in seeds {
        let mut digest = Sha256::new();
        digest.update(prefix);
        digest.update(seed.as_bytes());
        digest.update(body);
        valid |= CtOutput::<Sha256>::new(digest.finalize()) == received;
    }
    valid
}

fn callback_keys(
    config: &WebhookConfig,
    container: &Container,
    field: &'static str,
    valid: impl Fn(&str) -> bool,
) -> Result<Vec<Zeroizing<String>>> {
    let provider = container
        .make::<dyn ModerationCredentialProvider>()
        .map_err(|_| Error::InvalidConfig("moderation callback credential provider"))?;
    let parse_reference = |value: &serde_json::Value| {
        serde_json::from_value::<CredentialReference>(value.clone())
            .map_err(|_| Error::InvalidConfig("moderation callback credential reference"))
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
                "moderation previous callback credentials",
            ))?;
        for value in older {
            let reference = parse_reference(value)?;
            if references.contains(&reference) {
                return Err(Error::InvalidConfig(
                    "moderation duplicate callback credential",
                ));
            }
            references.push(reference);
        }
    }
    references
        .into_iter()
        .map(|reference| {
            let credentials = provider
                .resolve(&reference)
                .map_err(|_| Error::InvalidConfig("moderation callback credential"))?;
            credentials
                .get(field)
                .filter(|value| valid(value))
                .map(|value| Zeroizing::new(value.to_owned()))
                .ok_or(Error::InvalidConfig("moderation callback credential"))
        })
        .collect()
}

pub(crate) fn register(container: &Container) -> Result<()> {
    WebhookRuntime::register_signature_validator(
        container,
        "moderation-sightengine",
        |config, container| {
            let keys = callback_keys(config, container, "signing_secret", |value| {
                value.starts_with("casec_")
                    && (7..=512).contains(&value.len())
                    && value.bytes().all(|byte| byte.is_ascii_graphic())
            })?;
            Ok(Arc::new(SightengineSignature(keys)))
        },
    )?;
    WebhookRuntime::register_signature_validator(
        container,
        "moderation-tencent",
        |config, container| {
            let keys = callback_keys(
                config,
                container,
                "video_callback_seed",
                valid_tencent_callback_seed,
            )?;
            Ok(Arc::new(TencentSignature(keys)))
        },
    )?;
    WebhookRuntime::register_signature_validator(
        container,
        "moderation-aliyun",
        |config, container| {
            let account_uid = config.options()["moderation_callback_account_uid"]
                .as_str()
                .filter(|uid| {
                    (1..=32).contains(&uid.len()) && uid.bytes().all(|byte| byte.is_ascii_digit())
                })
                .ok_or(Error::InvalidConfig("aliyun callback account UID"))?
                .to_owned();
            let seeds = callback_keys(
                config,
                container,
                "video_callback_seed",
                valid_aliyun_callback_seed,
            )?;
            Ok(Arc::new(AliyunSignature { account_uid, seeds }))
        },
    )
}

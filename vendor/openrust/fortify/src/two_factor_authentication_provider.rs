use crate::{FortifyError, FortifyResult, contracts::TwoFactorAuthenticationProvider as Contract};
use aws_lc_rs::hmac;
use md5::{Digest as _, Md5};
use openrust::{
    cache::{Ttl, Value},
    contracts::{cache::Repository, config::Repository as Config},
    http::HttpFuture,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
type Clock = dyn Fn() -> FortifyResult<i64> + Send + Sync;

/// Google2FA's SHA1 / six-digit / thirty-second engine with Fortify's optional cache.
/// The cache follows upstream get / verify / put semantics; it is not an atomic claim.
pub struct TwoFactorAuthenticationProvider {
    cache: Option<Arc<dyn Repository>>,
    config: Option<Arc<dyn Config>>,
    clock: Arc<Clock>,
    window: AtomicI64,
}

#[allow(non_snake_case, reason = "Matches Fortify and Google2FA method names")]
impl TwoFactorAuthenticationProvider {
    pub fn new(cache: Option<Arc<dyn Repository>>) -> Self {
        Self {
            cache,
            config: None,
            clock: Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|time| i64::try_from(time.as_secs()).ok())
                    .ok_or(FortifyError::InvalidTime)
            }),
            window: AtomicI64::new(1),
        }
    }
    pub fn withClock(
        mut self,
        clock: impl Fn() -> FortifyResult<i64> + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }
    pub fn with_config(mut self, config: Arc<dyn Config>) -> Self {
        self.config = Some(config);
        self
    }
    pub fn withWindow(self, window: i64) -> Self {
        self.window.store(window, Ordering::Relaxed);
        self
    }
    pub fn getWindow(&self) -> i64 {
        self.window.load(Ordering::Relaxed)
    }
    fn current_window(&self) -> i64 {
        if let Some(window) = self
            .config
            .as_ref()
            .and_then(|config| config.get("fortify-options.two-factor-authentication.window"))
            .and_then(|value| value.as_i64())
        {
            self.window.store(window, Ordering::Relaxed);
            window
        } else {
            self.getWindow()
        }
    }
    async fn verify_code(&self, secret: &str, code: &str) -> FortifyResult<bool> {
        let window = self.current_window();
        let digest = Md5::digest(code.as_bytes());
        let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let key = format!("fortify.2fa_codes.{hash}");
        let old = match &self.cache {
            Some(cache) => match cache.get(&key).await? {
                Some(Value::Json(serde_json::Value::Null)) | None => None,
                Some(Value::Json(value)) => {
                    Some(value.as_i64().ok_or(FortifyError::InvalidCacheTimestamp)?)
                }
                Some(Value::Bytes(_)) => return Err(FortifyError::InvalidCacheTimestamp),
            },
            None => None,
        };
        let current = (self.clock)()?.div_euclid(30);
        let end = current
            .checked_add(window)
            .ok_or(FortifyError::InvalidTime)?;
        let mut start = current
            .checked_sub(window)
            .ok_or(FortifyError::InvalidTime)?;
        if let Some(old) = old {
            start = start.max(old.checked_add(1).ok_or(FortifyError::InvalidTime)?);
        }
        if start > end {
            return Ok(false);
        }
        // Google2FA validates the secret even if the submitted code is malformed.
        let decoded = decode_secret(secret)?;
        for step in start..=end {
            // Google2FA packs a zero high word and the low 32 bits of the counter.
            let counter = u64::from(step as u32).to_be_bytes();
            let tag = hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &decoded),
                &counter,
            );
            let bytes = tag.as_ref();
            let offset = usize::from(bytes[19] & 15);
            let binary = u32::from_be_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("SHA1 truncation"),
            ) & 0x7fff_ffff;
            let expected = format!("{:06}", binary % 1_000_000);
            if bool::from(expected.as_bytes().ct_eq(code.as_bytes())) {
                // verifyKeyNewer returns true for a null old timestamp; Fortify reads the clock again.
                let timestamp = if old.is_some() {
                    step
                } else {
                    (self.clock)()?.div_euclid(30)
                };
                if let Some(cache) = &self.cache {
                    let ttl = window
                        .max(1)
                        .checked_mul(60)
                        .ok_or(FortifyError::InvalidTime)?;
                    cache
                        .put(&key, &Value::from(timestamp), Some(Ttl::Seconds(ttl)))
                        .await?;
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl Contract for TwoFactorAuthenticationProvider {
    fn generateSecretKey(&self, length: usize) -> FortifyResult<String> {
        validate_length(length)?;
        let mut entropy = Zeroizing::new(Vec::new());
        entropy
            .try_reserve_exact(length)
            .map_err(|_| FortifyError::Allocation)?;
        entropy.resize(length, 0);
        aws_lc_rs::rand::fill(&mut entropy).map_err(|_| FortifyError::Entropy)?;
        let mut secret = String::new();
        secret
            .try_reserve_exact(length)
            .map_err(|_| FortifyError::Allocation)?;
        secret.extend(
            entropy
                .iter()
                .map(|byte| char::from(ALPHABET[usize::from(byte & 31)])),
        );
        Ok(secret)
    }
    fn qrCodeUrl(&self, company: &str, account: &str, secret: &str) -> String {
        let issuer = raw_url_encode(company);
        format!(
            "otpauth://totp/{issuer}:{}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits=6&period=30",
            raw_url_encode(account)
        )
    }
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> HttpFuture<'a, FortifyResult<bool>> {
        Box::pin(self.verify_code(secret, code))
    }
}
fn validate_length(length: usize) -> FortifyResult<()> {
    if !length.is_power_of_two() && length != 0 {
        return Err(FortifyError::IncompatibleWithGoogleAuthenticator);
    }
    if length < 16 {
        return Err(FortifyError::SecretKeyTooShort);
    }
    Ok(())
}
fn decode_secret(secret: &str) -> FortifyResult<Zeroizing<Vec<u8>>> {
    if secret.len() < 8 {
        return Err(FortifyError::SecretKeyTooShort);
    }
    if !secret
        .bytes()
        .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'2'..=b'7'))
    {
        return Err(FortifyError::InvalidCharacters);
    }
    validate_length(secret.len())?;
    let capacity = secret
        .len()
        .checked_mul(5)
        .ok_or(FortifyError::Allocation)?
        / 8;
    let mut output = Zeroizing::new(Vec::new());
    output
        .try_reserve_exact(capacity)
        .map_err(|_| FortifyError::Allocation)?;
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in secret.bytes() {
        let upper = byte.to_ascii_uppercase();
        let value = if upper.is_ascii_uppercase() {
            upper - b'A'
        } else {
            upper - b'2' + 26
        };
        buffer = (buffer << 5) | u16::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(output)
}
fn raw_url_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    out
}

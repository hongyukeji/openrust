use crate::{PasskeyError, PasskeyResult};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use url::Url;
use webauthn_rs_core::{WebauthnCore, proto::*};

/// Server-only state. Return only `options()` to the browser.
#[derive(Clone, Serialize, Deserialize)]
pub struct RegistrationOptions {
    options: Value,
    state: RegistrationState,
    user_handle: String,
}
impl RegistrationOptions {
    pub fn options(&self) -> &Value {
        &self.options
    }
}
/// Server-only state. Never accept this structure from a browser.
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthenticationOptions {
    options: Value,
    state: AuthenticationState,
    allowed: Vec<Vec<u8>>,
}
impl AuthenticationOptions {
    pub fn options(&self) -> &Value {
        &self.options
    }
}
/// Public-key material stored on the server, never an authenticator private key.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredCredential {
    #[serde(default)]
    aaguid: Option<String>,
    credential: Credential,
    user_handle: String,
}
impl StoredCredential {
    pub fn aaguid(&self) -> Option<&str> {
        self.aaguid
            .as_deref()
            .filter(|id| *id != crate::Aaguids::unknown())
    }
    pub fn credential_id(&self) -> String {
        URL_SAFE_NO_PAD.encode(&self.credential.cred_id)
    }
    pub fn user_handle(&self) -> &str {
        &self.user_handle
    }
    /// 绑定近期确认的稳定凭据身份；签名计数器及备份状态更新不会改变该指纹。
    pub fn identity_fingerprint(&self) -> PasskeyResult<String> {
        let bytes = serde_json::to_vec(&(
            &self.credential.cred_id,
            &self.credential.cred,
            &self.user_handle,
        ))?;
        Ok(
            aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &bytes)
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }
    pub fn counter(&self) -> u32 {
        self.credential.counter
    }
}

/// Fixed Fortify policy: ES256/RS256, resident key, UV required, none attestation,
/// exact origins and ports. The core API permits the upstream 64-byte HMAC user
/// handle; the high-level wrapper limits handles to UUIDs.
pub struct WebAuthn(WebauthnCore);
impl WebAuthn {
    pub fn new(rp_id: &str, origins: &[String], timeout: u64) -> PasskeyResult<Self> {
        if rp_id.is_empty() || timeout == 0 || timeout > u64::from(u32::MAX) || origins.is_empty() {
            return Err(PasskeyError::Configuration(
                "RP ID, allowed origins and positive timeout are required",
            ));
        }
        let mut allowed = Vec::new();
        for origin in origins {
            let url =
                Url::parse(origin).map_err(|_| PasskeyError::Configuration("invalid origin"))?;
            let host = url
                .host_str()
                .ok_or(PasskeyError::Configuration("origin has no host"))?;
            if !(host == rp_id || host.ends_with(&format!(".{rp_id}")))
                || !(url.scheme() == "https" || (url.scheme() == "http" && host == "localhost"))
                || url.origin().ascii_serialization() != *origin
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(PasskeyError::Configuration(
                    "origin must be an HTTPS origin under the RP ID (HTTP localhost is allowed)",
                ));
            }
            allowed.push(url);
        }
        Ok(Self(WebauthnCore::new_unsafe_experts_only(
            rp_id,
            rp_id,
            allowed,
            Duration::from_millis(timeout),
            Some(false),
            Some(false),
        )))
    }
    pub fn registration_options(
        &self,
        handle: &str,
        username: &str,
        display_name: &str,
        excluded: &[StoredCredential],
    ) -> PasskeyResult<RegistrationOptions> {
        if handle.is_empty() || handle.len() > 64 {
            return Err(PasskeyError::Configuration(
                "user handle must contain 1 to 64 bytes",
            ));
        }
        let builder = self
            .0
            .new_challenge_register_builder(handle.as_bytes(), username, display_name)
            .map_err(|_| PasskeyError::Format)?
            .user_verification_policy(UserVerificationPolicy::Required)
            .require_resident_key(true)
            .credential_algorithms(vec![COSEAlgorithm::ES256, COSEAlgorithm::RS256])
            .attestation(AttestationConveyancePreference::None)
            .attestation_formats(Some(vec![AttestationFormat::None]))
            .exclude_credentials(Some(
                excluded
                    .iter()
                    .map(|c| c.credential.cred_id.clone())
                    .collect(),
            ));
        let (options, state) = self
            .0
            .generate_challenge_register(builder)
            .map_err(|_| PasskeyError::Verification)?;
        Ok(RegistrationOptions {
            options: serde_json::to_value(options.public_key)?,
            state,
            user_handle: handle.into(),
        })
    }
    pub fn register(
        &self,
        value: Value,
        options: &RegistrationOptions,
    ) -> PasskeyResult<StoredCredential> {
        identity(&value)?;
        let reg: RegisterPublicKeyCredential =
            serde_json::from_value(value).map_err(|_| PasskeyError::Format)?;
        let credential = self
            .0
            .register_credential(&reg, &options.state, None)
            .map_err(|_| PasskeyError::Verification)?;
        if credential.attestation_format != AttestationFormat::None
            || !credential.user_verified
            || credential.cred_id != reg.raw_id
        {
            return Err(PasskeyError::Verification);
        }
        let aaguid = registration_aaguid(reg.response.attestation_object.as_ref())?;
        Ok(StoredCredential {
            aaguid: Some(aaguid),
            credential,
            user_handle: options.user_handle.clone(),
        })
    }
    pub fn verification_options(
        &self,
        allowed: &[StoredCredential],
    ) -> PasskeyResult<AuthenticationOptions> {
        let creds = allowed.iter().map(|c| c.credential.clone()).collect();
        let builder = self
            .0
            .new_challenge_authenticate_builder(creds, Some(UserVerificationPolicy::Required))
            .map_err(|_| PasskeyError::Verification)?;
        let (options, state) = self
            .0
            .generate_challenge_authenticate(builder)
            .map_err(|_| PasskeyError::Verification)?;
        Ok(AuthenticationOptions {
            options: serde_json::to_value(options.public_key)?,
            state,
            allowed: allowed
                .iter()
                .map(|c| c.credential.cred_id.clone())
                .collect(),
        })
    }
    /// Caller loads the current credential under a database row lock and persists
    /// this updated value before issuing authentication or confirmation.
    pub fn verify(
        &self,
        value: Value,
        options: &AuthenticationOptions,
        credential: &mut StoredCredential,
        discoverable: bool,
    ) -> PasskeyResult<()> {
        let id = identity(&value)?;
        if id != credential.credential.cred_id
            || (!options.allowed.is_empty() && !options.allowed.contains(&id))
        {
            return Err(PasskeyError::Verification);
        }
        let assertion: PublicKeyCredential =
            serde_json::from_value(value).map_err(|_| PasskeyError::Format)?;
        let user_handle = assertion.response.user_handle.as_deref();
        if (discoverable && user_handle.is_none())
            || user_handle.is_some_and(|h| h != credential.user_handle.as_bytes())
        {
            return Err(PasskeyError::Verification);
        }
        let result = self
            .0
            .authenticate_single_credential(&assertion, &options.state, &credential.credential)
            .map_err(|_| PasskeyError::Verification)?;
        if !result.user_verified() {
            return Err(PasskeyError::Verification);
        }
        credential.credential.counter = result.counter();
        credential.credential.backup_state = result.backup_state();
        credential.credential.backup_eligible = result.backup_eligible();
        Ok(())
    }
}
fn identity(value: &Value) -> PasskeyResult<Vec<u8>> {
    let raw = value
        .get("rawId")
        .and_then(Value::as_str)
        .ok_or(PasskeyError::Format)?;
    if value.get("id").and_then(Value::as_str) != Some(raw)
        || value.get("type").and_then(Value::as_str) != Some("public-key")
    {
        return Err(PasskeyError::Format);
    }
    let id = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| PasskeyError::Format)?;
    if id.is_empty() || URL_SAFE_NO_PAD.encode(&id) != raw {
        return Err(PasskeyError::Format);
    }
    Ok(id)
}

pub(crate) fn validate_credential(value: &Value, registration: bool) -> PasskeyResult<()> {
    identity(value)?;
    if registration {
        let _: RegisterPublicKeyCredential =
            serde_json::from_value(value.clone()).map_err(|_| PasskeyError::Format)?;
    } else {
        let _: PublicKeyCredential =
            serde_json::from_value(value.clone()).map_err(|_| PasskeyError::Format)?;
    }
    Ok(())
}

// The core has already validated this registration object. Retain its AAGUID
// only for display; none attestation does not establish manufacturer identity.
fn registration_aaguid(bytes: &[u8]) -> PasskeyResult<String> {
    let object: serde_cbor_2::Value =
        serde_cbor_2::from_slice(bytes).map_err(|_| PasskeyError::Format)?;
    let serde_cbor_2::Value::Map(object) = object else {
        return Err(PasskeyError::Format);
    };
    let Some(serde_cbor_2::Value::Bytes(data)) =
        object.get(&serde_cbor_2::Value::Text("authData".into()))
    else {
        return Err(PasskeyError::Format);
    };
    if data.len() < 55 || data[32] & 0x40 == 0 {
        return Err(PasskeyError::Format);
    }
    let mut id = String::with_capacity(36);
    for (index, byte) in data[37..53].iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            id.push('-');
        }
        use std::fmt::Write as _;
        write!(id, "{byte:02x}").expect("write to String");
    }
    Ok(id)
}

//! Authentication, password hashing, and shared session configuration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) const MIN_MEMORY_KIB: u32 = 19_456;

/// JWT signing and verification settings. Key values are Base64-encoded CSPRNG output.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtConfig {
    pub(crate) issuer: String,
    pub(crate) audience: String,
    pub(crate) active_key_id: String,
    pub(crate) keys: BTreeMap<String, String>,
    #[serde(default = "default_access_ttl")]
    pub(crate) access_ttl_secs: u64,
    #[serde(default = "default_max_valid_ttl")]
    pub(crate) max_valid_ttl_secs: u64,
}

impl JwtConfig {
    /// Creates JWT settings from issuer, audience, active key ID and Base64 key ring.
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        active_key_id: impl Into<String>,
        keys: BTreeMap<String, String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            active_key_id: active_key_id.into(),
            keys,
            access_ttl_secs: default_access_ttl(),
            max_valid_ttl_secs: default_max_valid_ttl(),
        }
    }

    /// Sets access-token issuance and maximum accepted lifetime in seconds.
    #[must_use]
    pub fn with_ttl(mut self, access_ttl_secs: u64, max_valid_ttl_secs: u64) -> Self {
        self.access_ttl_secs = access_ttl_secs;
        self.max_valid_ttl_secs = max_valid_ttl_secs;
        self
    }
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JwtConfig")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("active_key_id", &self.active_key_id)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .field("keys", &"[REDACTED]")
            .field("access_ttl_secs", &self.access_ttl_secs)
            .field("max_valid_ttl_secs", &self.max_valid_ttl_secs)
            .finish()
    }
}

/// Argon2id parameters used for newly hashed passwords.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PasswordConfig {
    /// Memory cost in kibibytes.
    pub memory_kib: u32,
    /// Number of Argon2 iterations.
    pub iterations: u32,
    /// Argon2 parallelism degree.
    pub parallelism: u32,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            memory_kib: MIN_MEMORY_KIB,
            iterations: 2,
            parallelism: 1,
        }
    }
}

/// Settings for the shared Cookie Session middleware.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Cookie name. Defaults to `openrust_session`.
    pub cookie_name: String,
    /// Whether browsers send the cookie only over HTTPS. Defaults to `true`.
    pub secure: bool,
    /// Whether the cookie is inaccessible to JavaScript (`HttpOnly`). Defaults to `true`.
    pub http_only: bool,
    /// `SameSite` policy: `strict`, `lax`, or `none`. Defaults to `lax`.
    pub same_site: String,
    /// Inactivity lifetime, in seconds. Defaults to two weeks.
    pub ttl_secs: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_name: "openrust_session".to_owned(),
            secure: true,
            http_only: true,
            same_site: "lax".to_owned(),
            ttl_secs: 1_209_600,
        }
    }
}

impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionConfig")
            .field("cookie_name", &self.cookie_name)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("ttl_secs", &self.ttl_secs)
            .finish()
    }
}

fn default_access_ttl() -> u64 {
    900
}

fn default_max_valid_ttl() -> u64 {
    3_600
}

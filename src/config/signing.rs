//! Signed-link configuration.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Key ring and issuer used exclusively for application signed links.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLinksConfig {
    pub(crate) issuer: String,
    pub(crate) active_key_id: String,
    pub(crate) keys: BTreeMap<String, String>,
    #[serde(default = "default_max_ttl_secs")]
    pub(crate) max_ttl_secs: u64,
}

impl SignedLinksConfig {
    /// Creates signed-link settings from an issuer, active key ID and Base64 key ring.
    pub fn new(
        issuer: impl Into<String>,
        active_key_id: impl Into<String>,
        keys: BTreeMap<String, String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            active_key_id: active_key_id.into(),
            keys,
            max_ttl_secs: default_max_ttl_secs(),
        }
    }

    /// Sets the maximum accepted link lifetime in seconds.
    #[must_use]
    pub const fn with_max_ttl_secs(mut self, max_ttl_secs: u64) -> Self {
        self.max_ttl_secs = max_ttl_secs;
        self
    }
}

impl std::fmt::Debug for SignedLinksConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignedLinksConfig")
            .field("issuer", &self.issuer)
            .field("active_key_id", &self.active_key_id)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .field("keys", &"[REDACTED]")
            .field("max_ttl_secs", &self.max_ttl_secs)
            .finish()
    }
}

fn default_max_ttl_secs() -> u64 {
    86_400
}

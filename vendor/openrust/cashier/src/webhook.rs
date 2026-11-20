use crate::{Client, Error, Provider, Result};
use aws_lc_rs::hmac;
use http::HeaderMap;
use serde_json::Value;

/// Constructible only after verifying the untouched body and the named endpoint secret.
#[derive(Clone)]
pub struct VerifiedEvent {
    pub(crate) scope: String,
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) payload: Value,
    pub(crate) fingerprint: String,
    pub(crate) subscription: Option<crate::Subscription>,
    pub(crate) invoice: Option<crate::Invoice>,
    pub(crate) customer_deleted: bool,
}
impl std::fmt::Debug for VerifiedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedBillingEvent([REDACTED])")
    }
}
impl VerifiedEvent {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn eventType(&self) -> &str {
        &self.kind
    }
    /// Verified provider payload for application events. Avoid logging personal information.
    pub fn data(&self) -> &Value {
        &self.payload
    }
    /// Current canonical state, populated only inside BillingStore::process handlers.
    pub fn currentSubscription(&self) -> Option<&crate::Subscription> {
        self.subscription.as_ref()
    }
    pub fn currentInvoice(&self) -> Option<&crate::Invoice> {
        self.invoice.as_ref()
    }
    pub fn customerDeleted(&self) -> bool {
        self.customer_deleted
    }
    pub fn success(&self) -> http::Response<String> {
        http::Response::new(String::new())
    }
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn verify(client: &Client, headers: &HeaderMap, body: &[u8]) -> Result<VerifiedEvent> {
    if body.len() > 1024 * 1024 {
        return Err(Error::Verification);
    }
    let stripe = client.provider() == Provider::Stripe;
    let name = if stripe {
        "stripe-signature"
    } else {
        "paddle-signature"
    };
    if headers.get_all(name).iter().count() != 1 {
        return Err(Error::Verification);
    }
    let header = headers[name].to_str().map_err(|_| Error::Verification)?;
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(if stripe { ',' } else { ';' }) {
        let (key, val) = part.trim().split_once('=').ok_or(Error::Verification)?;
        if key == if stripe { "t" } else { "ts" } {
            if timestamp.replace(val).is_some() {
                return Err(Error::Verification);
            }
        } else if key == if stripe { "v1" } else { "h1" } {
            if val.len() != 64 || !val.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Verification);
            }
            let bytes = (0..64)
                .step_by(2)
                .map(|i| u8::from_str_radix(&val[i..i + 2], 16).map_err(|_| Error::Verification))
                .collect::<Result<Vec<_>>>()?;
            signatures.push(bytes);
        }
    }
    let timestamp = timestamp.ok_or(Error::Verification)?;
    if !timestamp.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Verification);
    }
    let t: i64 = timestamp.parse().map_err(|_| Error::Verification)?;
    let tolerance = if stripe { 300 } else { 5 };
    if jiff::Timestamp::now().as_second().abs_diff(t) > tolerance {
        return Err(Error::Verification);
    }
    let mut message = format!("{timestamp}{}", if stripe { '.' } else { ':' }).into_bytes();
    message.extend_from_slice(body);
    let valid = client.secrets().iter().any(|secret| {
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        signatures
            .iter()
            .any(|s| hmac::verify(&key, &message, s).is_ok())
    });
    if !valid {
        return Err(Error::Verification);
    }
    let event: Value = serde_json::from_slice(body).map_err(|_| Error::Verification)?;
    let (id, kind, payload) = if stripe {
        if event["object"] != "event"
            || event["livemode"].as_bool() != Some(client.live())
            || event.get("account").is_some_and(|v| !v.is_null())
        {
            return Err(Error::Verification);
        }
        (&event["id"], &event["type"], &event["data"]["object"])
    } else {
        (&event["event_id"], &event["event_type"], &event["data"])
    };
    let id = id.as_str().ok_or(Error::Verification)?;
    crate::client::identifier(id).map_err(|_| Error::Verification)?;
    let kind = kind
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(Error::Verification)?;
    if !payload.is_object() {
        return Err(Error::Verification);
    }
    Ok(VerifiedEvent {
        scope: client.scope(),
        id: id.into(),
        kind: kind.into(),
        payload: payload.clone(),
        fingerprint: hash(
            &serde_json::to_vec(&(id, kind, payload)).map_err(|_| Error::Verification)?,
        ),
        subscription: None,
        invoice: None,
        customer_deleted: false,
    })
}

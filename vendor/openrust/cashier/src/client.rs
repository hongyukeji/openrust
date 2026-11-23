use crate::{Error, Result, webhook::VerifiedEvent};
use http::{HeaderMap, HeaderValue, Method};
use openrust::{contracts::config::Repository, http::client::Factory};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use zeroize::Zeroizing;

/// Stripe's API is pinned to the version of the audited official SDK.
pub const STRIPE_VERSION: &str = "2026-11-18.endive";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Stripe,
    Paddle,
}
impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stripe => "stripe",
            Self::Paddle => "paddle",
        }
    }
}
struct Inner {
    name: String,
    provider: Provider,
    endpoint: url::Url,
    key: Zeroizing<String>,
    webhook_secrets: Vec<Zeroizing<String>>,
    http: Arc<Factory>,
    live: bool,
}
/// An immutable named account snapshot; Debug never prints credentials.
#[derive(Clone)]
pub struct Client(Arc<Inner>);
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CashierClient([REDACTED])")
    }
}
impl Client {
    pub fn new(name: &str, profile: &Value, http: Arc<Factory>) -> Result<Self> {
        identifier(name)?;
        let provider = match profile["driver"].as_str() {
            Some("stripe") => Provider::Stripe,
            Some("paddle") => Provider::Paddle,
            _ => return Err(Error::Configuration("driver")),
        };
        let key = required(profile, "secret")?.to_owned();
        HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| Error::Configuration("secret"))?;
        let live = profile["live"].as_bool().unwrap_or(false);
        let default = match provider {
            Provider::Stripe => "https://api.stripe.com",
            Provider::Paddle if live => "https://api.paddle.com",
            Provider::Paddle => "https://sandbox-api.paddle.com",
        };
        let endpoint = url::Url::parse(profile["endpoint"].as_str().unwrap_or(default))
            .map_err(|_| Error::Configuration("endpoint"))?;
        let local = matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "[::1]" | "localhost")
        );
        if (endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && local))
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
        {
            return Err(Error::Configuration("endpoint"));
        }
        let secrets = match profile.get("webhook_secrets") {
            Some(Value::Array(v)) if !v.is_empty() => v
                .iter()
                .map(|s| {
                    s.as_str()
                        .filter(|s| !s.is_empty())
                        .map(|s| Zeroizing::new(s.to_owned()))
                        .ok_or(Error::Configuration("webhook_secrets"))
                })
                .collect::<Result<Vec<_>>>()?,
            _ => vec![Zeroizing::new(required(profile, "webhook_secret")?.into())],
        };
        Ok(Self(Arc::new(Inner {
            name: name.into(),
            provider,
            endpoint,
            key: Zeroizing::new(key),
            webhook_secrets: secrets,
            http,
            live,
        })))
    }
    pub fn name(&self) -> &str {
        &self.0.name
    }
    pub fn provider(&self) -> Provider {
        self.0.provider
    }
    pub(crate) fn scope(&self) -> String {
        format!("{}:{}", self.provider().as_str(), self.name())
    }
    pub(crate) fn secrets(&self) -> &[Zeroizing<String>] {
        &self.0.webhook_secrets
    }
    pub(crate) fn live(&self) -> bool {
        self.0.live
    }
    pub fn verifyWebhook(&self, headers: &HeaderMap, body: &[u8]) -> Result<VerifiedEvent> {
        crate::webhook::verify(self, headers, body)
    }
    /// Advanced server-side API access. Stripe mutation keys must be stable across retries.
    /// Paddle has no provider idempotency guarantee: never blindly repeat an unknown mutation.
    pub async fn api(
        &self,
        method: Method,
        path: &str,
        parameters: &Value,
        key: Option<&str>,
    ) -> Result<Value> {
        if !matches!(
            method,
            Method::GET | Method::POST | Method::PATCH | Method::DELETE
        ) || !parameters.is_object()
        {
            return Err(Error::InvalidInput("request"));
        }
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['?', '#', '\\', '%'])
            || path.split('/').skip(1).any(|s| identifier(s).is_err())
        {
            return Err(Error::InvalidInput("API path"));
        }
        let mutate = method != Method::GET;
        if mutate && self.provider() == Provider::Stripe {
            operation_key(key.ok_or(Error::InvalidInput("idempotency key"))?)?;
        }
        if let Some(k) = key {
            operation_key(k)?;
        }
        let mut url = self
            .0
            .endpoint
            .join(path)
            .map_err(|_| Error::InvalidInput("path"))?;
        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", self.0.key.as_str()))
            .map_err(|_| Error::Configuration("secret"))?;
        auth.set_sensitive(true);
        headers.insert("authorization", auth);
        let (body, content) = if self.provider() == Provider::Stripe {
            headers.insert("stripe-version", HeaderValue::from_static(STRIPE_VERSION));
            if mutate {
                let mut k = HeaderValue::from_str(&crate::webhook::hash(
                    format!("{}\0{}", self.scope(), key.expect("validated")).as_bytes(),
                ))
                .map_err(|_| Error::InvalidInput("idempotency key"))?;
                k.set_sensitive(true);
                headers.insert("idempotency-key", k);
            }
            let mut pairs = Vec::new();
            for (k, v) in parameters.as_object().expect("validated") {
                encode(k, v, &mut pairs)?;
            }
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in pairs {
                form.append_pair(&k, &v);
            }
            let form = form.finish();
            if method == Method::GET {
                if !form.is_empty() {
                    url.set_query(Some(&form));
                }
                (Vec::new(), "application/x-www-form-urlencoded")
            } else {
                (form.into_bytes(), "application/x-www-form-urlencoded")
            }
        } else {
            headers.insert("paddle-version", HeaderValue::from_static("1"));
            if method == Method::GET {
                for (k, v) in parameters.as_object().expect("validated") {
                    if v.is_object() || v.is_array() || v.is_null() {
                        return Err(Error::InvalidInput("query value"));
                    }
                    url.query_pairs_mut().append_pair(k, &scalar(v)?);
                }
                (Vec::new(), "application/json")
            } else {
                (
                    serde_json::to_vec(parameters).map_err(|_| Error::InvalidInput("JSON"))?,
                    "application/json",
                )
            }
        };
        let response = self
            .0
            .http
            .pending_request()
            .redact_payload()
            .without_redirecting()
            .timeout(Duration::from_secs(15))
            .max_response_bytes(1024 * 1024)
            .accept_json()
            .with_headers(headers)
            .with_body(body, HeaderValue::from_static(content))
            .send(method, url.as_str())
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        if !response.successful() {
            return Err(
                if response.status().is_server_error()
                    || response.status().is_redirection()
                    || response.status().as_u16() == 429
                {
                    Error::OutcomeUnknown
                } else {
                    Error::Rejected(response.status().as_u16())
                },
            );
        }
        if response.status() == http::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        response.json().map_err(|_| Error::OutcomeUnknown)
    }
    pub(crate) async fn call(
        &self,
        method: Method,
        path: &str,
        params: &Value,
        key: Option<&str>,
    ) -> Result<Value> {
        let value = self.api(method, path, params, key).await?;
        if self.provider() == Provider::Paddle {
            value.get("data").cloned().ok_or(Error::OutcomeUnknown)
        } else {
            Ok(value)
        }
    }
    pub async fn retrieve(&self, resource: &str, id: &str) -> Result<Value> {
        identifier(resource)?;
        identifier(id)?;
        self.call(
            Method::GET,
            &self.path(resource, Some(id)),
            &json!({}),
            None,
        )
        .await
    }
    pub(crate) fn path(&self, resource: &str, id: Option<&str>) -> String {
        format!(
            "{}{resource}{}",
            if self.provider() == Provider::Stripe {
                "/v1/"
            } else {
                "/"
            },
            id.map(|id| format!("/{id}")).unwrap_or_default()
        )
    }
}
pub struct CashierManager {
    config: Arc<dyn Repository>,
    http: Arc<Factory>,
    drivers: Mutex<HashMap<String, Client>>,
}
impl CashierManager {
    pub fn new(config: Arc<dyn Repository>, http: Arc<Factory>) -> Self {
        Self {
            config,
            http,
            drivers: Mutex::default(),
        }
    }
    pub fn get_default_driver(&self) -> Result<String> {
        self.config
            .get("cashier.default")
            .and_then(|v| v.as_str().map(str::to_owned))
            .filter(|s| !s.is_empty())
            .ok_or(Error::Configuration("cashier.default"))
    }
    pub fn driver(&self, name: &str) -> Result<Client> {
        identifier(name)?;
        let mut drivers = self.drivers.lock().expect("cashier manager");
        if let Some(client) = drivers.get(name) {
            return Ok(client.clone());
        }
        // Named accounts override the provider's canonical configuration as a whole.
        let profile = self.config.get(&format!("cashier.drivers.{name}")).or_else(|| match name {
            "stripe" => Some(json!({"driver":"stripe", "secret":self.config.get("cashier.secret"), "webhook_secret":self.config.get("cashier.webhook.secret"), "live":self.config.get("cashier.live").unwrap_or(false.into())})),
            "paddle" => Some(json!({"driver":"paddle", "secret":self.config.get("cashier.paddle.api_key"), "webhook_secret":self.config.get("cashier.paddle.webhook_secret"), "live":!self.config.get("cashier.paddle.sandbox").and_then(|v|v.as_bool()).unwrap_or(false)})),
            _ => None,
        }).ok_or(Error::Configuration("account"))?;
        let client = Client::new(name, &profile, self.http.clone())?;
        drivers.insert(name.into(), client.clone());
        Ok(client)
    }
    pub fn forget_drivers(&self) -> &Self {
        self.drivers.lock().expect("cashier manager").clear();
        self
    }
}
pub(crate) fn identifier(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 255
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        || matches!(s, "." | "..")
    {
        Err(Error::InvalidInput("identifier"))
    } else {
        Ok(())
    }
}
pub(crate) fn operation_key(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 255 || !s.bytes().all(|b| (33..=126).contains(&b)) {
        Err(Error::InvalidInput("idempotency key"))
    } else {
        Ok(())
    }
}
fn required<'a>(v: &'a Value, k: &'static str) -> Result<&'a str> {
    v[k].as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or(Error::Configuration(k))
}
fn scalar(v: &Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) if n.is_i64() || n.is_u64() => Ok(n.to_string()),
        _ => Err(Error::InvalidInput(
            "form value; use integer minor units or decimal text",
        )),
    }
}
fn encode(prefix: &str, v: &Value, pairs: &mut Vec<(String, String)>) -> Result<()> {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                encode(&format!("{prefix}[{k}]"), v, pairs)?
            }
        }
        Value::Array(a) if a.is_empty() => pairs.push((prefix.into(), String::new())),
        Value::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                encode(&format!("{prefix}[{i}]"), v, pairs)?
            }
        }
        Value::Null => pairs.push((prefix.into(), String::new())),
        _ => pairs.push((prefix.into(), scalar(v)?)),
    };
    Ok(())
}

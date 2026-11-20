use crate::{Error, Result};
use jsonwebtoken::Algorithm;
use serde_json::Value;
use std::time::Duration;
use zeroize::Zeroizing;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientAuth {
    Basic,
    Post,
    None,
}
#[derive(Clone)]
pub struct ProviderConfig {
    pub(crate) issuer: String,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) client_id: String,
    pub(crate) client_secret: Option<Zeroizing<String>>,
    pub(crate) redirect: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) algorithms: Vec<Algorithm>,
    pub(crate) client_auth: ClientAuth,
    pub(crate) allow_loopback_http: bool,
    pub(crate) userinfo: bool,
    pub(crate) attempt_ttl: u64,
    pub(crate) leeway: u64,
    pub(crate) max_token_age: u64,
    pub(crate) timeout: Duration,
    pub(crate) replay_store: Option<String>,
}
impl ProviderConfig {
    pub fn from_config(v: &Value) -> Result<Self> {
        fn string(v: &Value, key: &'static str) -> Result<String> {
            v.get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or(Error::Configuration(key))
        }
        fn number(v: &Value, key: &'static str, default: u64, max: u64) -> Result<u64> {
            let n = match v.get(key) {
                None => default,
                Some(v) => v.as_u64().ok_or(Error::Configuration(key))?,
            };
            if n > max {
                return Err(Error::Configuration(key));
            }
            Ok(n)
        }
        fn flag(v: &Value, key: &'static str, default: bool) -> Result<bool> {
            match v.get(key) {
                None => Ok(default),
                Some(v) => v.as_bool().ok_or(Error::Configuration(key)),
            }
        }
        let issuer = string(v, "issuer")?;
        let redirect = string(v, "redirect")?;
        let allow_loopback_http = flag(v, "allow_loopback_http", false)?;
        let url = endpoint(&issuer, allow_loopback_http)?;
        if url.query().is_some() {
            return Err(Error::Configuration("issuer"));
        }
        endpoint(&redirect, allow_loopback_http)?;
        let mut allowed_origins = vec![url.origin().ascii_serialization()];
        match v.get("allowed_origins") {
            None => {}
            Some(Value::Array(origins)) if origins.len() <= 16 => {
                for origin in origins {
                    let origin = origin
                        .as_str()
                        .ok_or(Error::Configuration("allowed_origins"))?;
                    let parsed = endpoint(origin, allow_loopback_http)?;
                    if parsed.path() != "/" || parsed.query().is_some() {
                        return Err(Error::Configuration("allowed_origins"));
                    }
                    let origin = parsed.origin().ascii_serialization();
                    if !allowed_origins.contains(&origin) {
                        allowed_origins.push(origin);
                    }
                }
            }
            _ => return Err(Error::Configuration("allowed_origins")),
        }

        let client_id = string(v, "client_id")?;
        let client_secret = match v.get("client_secret") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.is_empty() => Some(Zeroizing::new(s.clone())),
            _ => return Err(Error::Configuration("client_secret")),
        };
        let client_auth = match v.get("client_auth") {
            None => {
                if client_secret.is_some() {
                    ClientAuth::Basic
                } else {
                    ClientAuth::None
                }
            }
            Some(Value::String(s)) => match s.as_str() {
                "client_secret_basic" => ClientAuth::Basic,
                "client_secret_post" => ClientAuth::Post,
                "none" => ClientAuth::None,
                _ => return Err(Error::Configuration("client_auth")),
            },
            _ => return Err(Error::Configuration("client_auth")),
        };
        if client_auth != ClientAuth::None && client_secret.is_none() {
            return Err(Error::Configuration("client_secret"));
        }
        let mut scopes = match v.get("scopes") {
            None => vec!["openid".into(), "profile".into(), "email".into()],
            Some(Value::Array(a)) => a
                .iter()
                .map(|s| {
                    s.as_str()
                        .filter(|s| !s.is_empty() && !s.contains(char::is_whitespace))
                        .map(str::to_owned)
                        .ok_or(Error::Configuration("scopes"))
                })
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(Error::Configuration("scopes")),
        };
        if !scopes.iter().any(|s| s == "openid") {
            scopes.insert(0, "openid".into());
        }
        let algorithms = match v.get("algorithms") {
            None => vec![Algorithm::RS256],
            Some(Value::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|v| {
                    algorithm(v.as_str().unwrap_or("")).ok_or(Error::Configuration("algorithms"))
                })
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(Error::Configuration("algorithms")),
        };
        let replay_store = match v.get("replay_store") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => return Err(Error::Configuration("replay_store")),
        };
        let attempt_ttl = number(v, "attempt_ttl", 600, 3600)?;
        let timeout = number(v, "timeout", 10, 60)?;
        if attempt_ttl == 0 || timeout == 0 {
            return Err(Error::Configuration("timeout or attempt_ttl"));
        }
        Ok(Self {
            issuer,
            allowed_origins,
            redirect,
            client_id,
            client_secret,
            client_auth,
            scopes,
            algorithms,
            allow_loopback_http,
            replay_store,
            attempt_ttl,
            timeout: Duration::from_secs(timeout),
            leeway: number(v, "leeway", 0, 60)?,
            max_token_age: number(v, "max_token_age", 600, 86400)?,
            userinfo: flag(v, "userinfo", true)?,
        })
    }
    pub(crate) fn auth_method(&self) -> &'static str {
        match self.client_auth {
            ClientAuth::Basic => "client_secret_basic",
            ClientAuth::Post => "client_secret_post",
            ClientAuth::None => "none",
        }
    }
    pub(crate) fn fingerprint(&self) -> String {
        crate::provider::hash(
            &serde_json::json!([self.issuer, self.client_id, self.redirect]).to_string(),
        )
    }
}
pub(crate) fn algorithm(v: &str) -> Option<Algorithm> {
    match v {
        "RS256" => Some(Algorithm::RS256),
        "RS384" => Some(Algorithm::RS384),
        "RS512" => Some(Algorithm::RS512),
        "PS256" => Some(Algorithm::PS256),
        "PS384" => Some(Algorithm::PS384),
        "PS512" => Some(Algorithm::PS512),
        "ES256" => Some(Algorithm::ES256),
        "ES384" => Some(Algorithm::ES384),
        _ => None,
    }
}
pub(crate) fn endpoint(value: &str, allow_loopback: bool) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| Error::Configuration("endpoint"))?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if (url.scheme() != "https" && !(allow_loopback && url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Configuration("endpoint"));
    }
    Ok(url)
}

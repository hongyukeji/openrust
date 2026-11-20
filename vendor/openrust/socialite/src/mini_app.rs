//! WeChat Mini Program login, following EasyWeChat's codeToSession entry point.
use crate::{Error, Result, two::User};
use openrust::http::client::Factory;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

/// Server-side code exchange. No browser redirect, OAuth token or shared login state.
pub struct MiniApp {
    http: Arc<Factory>,
    app_id: String,
    secret: Zeroizing<String>,
    endpoint: String,
}

/// Session key is deliberately neither serializable nor included in Debug.
pub struct Session {
    pub openid: String,
    pub unionid: Option<String>,
    pub session_key: Zeroizing<String>,
}
impl Session {
    /// Socialite identity containing only public identifiers, never the session key.
    pub fn user(&self) -> User {
        let mut raw = json!({"openid": self.openid});
        let mut attributes = json!({"id": self.openid});
        if let Some(unionid) = &self.unionid {
            raw["unionid"] = json!(unionid);
            attributes["unionid"] = json!(unionid);
        }
        User::from_attributes(raw, attributes.as_object().unwrap().clone())
    }
}
impl MiniApp {
    pub fn new(config: &Value, http: Arc<Factory>) -> Result<Self> {
        fn required(config: &Value, key: &'static str) -> Result<String> {
            config
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .map(str::to_owned)
                .ok_or(Error::Configuration(key))
        }
        Ok(Self {
            http,
            app_id: required(config, "app_id")?,
            secret: Zeroizing::new(required(config, "secret")?),
            endpoint: "https://api.weixin.qq.com/sns/jscode2session".into(),
        })
    }
    /// Explicit override for controlled protocol services, as with OAuth providers.
    pub fn withEndpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
    pub async fn codeToSession(&self, code: &str) -> Result<Session> {
        if code.trim().is_empty() {
            return Err(Error::MissingCode);
        }
        let response = self
            .http
            .pendingRequest()
            .redactPayload()
            .withoutRedirecting()
            .timeout(Duration::from_secs(5))
            .withQueryParameters([
                ("appid".into(), self.app_id.clone()),
                ("secret".into(), self.secret.to_string()),
                ("js_code".into(), code.into()),
                ("grant_type".into(), "authorization_code".into()),
            ])
            .get(&self.endpoint)
            .await?;
        if !response.successful() {
            return Err(Error::ProviderStatus(response.status().as_u16()));
        }
        let value: Value = response.json().map_err(|_| Error::InvalidResponse)?;
        if !value.is_object() || value.get("errcode").is_some_and(|v| v.as_i64() != Some(0)) {
            return Err(Error::InvalidResponse);
        }
        fn identifier(value: &Value, key: &str) -> Result<String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .map(str::to_owned)
                .ok_or(Error::InvalidResponse)
        }
        let unionid = match value.get("unionid") {
            None | Some(Value::Null) => None,
            Some(_) => Some(identifier(&value, "unionid")?),
        };
        Ok(Session {
            openid: identifier(&value, "openid")?,
            unionid,
            session_key: Zeroizing::new(identifier(&value, "session_key")?),
        })
    }
}

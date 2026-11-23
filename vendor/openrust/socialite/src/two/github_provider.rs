use super::{ProviderDefinition, ProviderFuture, User, abstract_provider::json};
use crate::{Error, Result};
use http::HeaderValue;
use openrust::http::{HeaderMap, client::Factory};
use serde_json::Value;
pub struct GithubProvider {
    authorize: String,
    token: String,
    user: String,
    emails: String,
}
impl Default for GithubProvider {
    fn default() -> Self {
        Self {
            authorize: "https://github.com/login/oauth/authorize".into(),
            token: "https://github.com/login/oauth/access_token".into(),
            user: "https://api.github.com/user".into(),
            emails: "https://api.github.com/user/emails".into(),
        }
    }
}
impl GithubProvider {
    /// Endpoint customization for controlled providers and protocol acceptance.
    pub fn withEndpoints(authorize: String, token: String, user: String, emails: String) -> Self {
        Self {
            authorize,
            token,
            user,
            emails,
        }
    }
}
impl ProviderDefinition for GithubProvider {
    fn getAuthUrl(&self) -> &str {
        &self.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.token
    }
    fn scopes(&self) -> &[&str] {
        &["user:email"]
    }
    fn getUserByToken<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let mut headers = HeaderMap::new();
            headers.insert(
                "user-agent",
                HeaderValue::from_static(concat!("OpenRust-Socialite/", env!("CARGO_PKG_VERSION"))),
            );
            headers.insert(
                "accept",
                HeaderValue::from_static("application/vnd.github.v3+json"),
            );
            let mut authorization = HeaderValue::from_str(&format!("token {token}"))
                .map_err(|_| Error::InvalidResponse)?;
            authorization.set_sensitive(true);
            headers.insert("authorization", authorization);
            let mut user = json(
                http.pending_request()
                    .with_headers(headers.clone())
                    .redact_payload()
                    .without_redirecting()
                    .get(&self.user)
                    .await?,
            )?;
            if !user.is_object() {
                return Err(Error::InvalidResponse);
            }
            if scopes.iter().any(|s| s == "user:email") {
                let emails = async {
                    json(
                        http.pending_request()
                            .with_headers(headers)
                            .redact_payload()
                            .without_redirecting()
                            .get(&self.emails)
                            .await?,
                    )
                }
                .await;
                let email = emails.ok().and_then(|v| {
                    v.as_array().and_then(|a| {
                        a.iter()
                            .find(|e| e["primary"] == true && e["verified"] == true)
                            .and_then(|e| e.get("email"))
                            .cloned()
                    })
                });
                user["email"] = email.unwrap_or(Value::Null);
            }
            Ok(user)
        })
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        let id = user
            .get("id")
            .cloned()
            .filter(|v| !v.is_null())
            .ok_or(Error::InvalidResponse)?;
        let attributes = [
            ("id", id),
            ("nodeId", user["node_id"].clone()),
            ("nickname", user["login"].clone()),
            ("name", user["name"].clone()),
            ("email", user["email"].clone()),
            ("avatar", user["avatar_url"].clone()),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        Ok(User::from_attributes(user, attributes))
    }
}

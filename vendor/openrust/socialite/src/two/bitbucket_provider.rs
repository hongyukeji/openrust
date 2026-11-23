use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User,
    abstract_provider::json,
    endpoints::{mapped, value},
};
use crate::Result;
use openrust::http::client::Factory;
use serde_json::Value;
#[derive(Clone)]
pub struct BitbucketProvider {
    endpoints: ProviderEndpoints,
}
impl Default for BitbucketProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://bitbucket.org/site/oauth2/authorize",
                "https://bitbucket.org/site/oauth2/access_token",
                "https://api.bitbucket.org/2.0/user",
            )
            .email("https://api.bitbucket.org/2.0/user/emails"),
        }
    }
}
impl BitbucketProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for BitbucketProvider {
    fn get_auth_url(&self) -> &str {
        &self.endpoints.authorize
    }
    fn get_token_url(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["email"]
    }
    fn scope_separator(&self) -> &str {
        " "
    }
    fn direct_token_basic_auth(&self) -> bool {
        true
    }
    fn get_user_by_token<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let mut user = json(
                http.pending_request()
                    .with_token(token)
                    .redact_payload()
                    .without_redirecting()
                    .get(&self.endpoints.user)
                    .await?,
            )?;
            if !user.is_object() {
                return Err(crate::Error::InvalidResponse);
            }
            if scopes.iter().any(|s| s == "email") {
                let emails = async {
                    let url = self
                        .endpoints
                        .email
                        .as_deref()
                        .ok_or(crate::Error::Configuration("Bitbucket email URL"))?;
                    json(
                        http.pending_request()
                            .with_token(token)
                            .redact_payload()
                            .without_redirecting()
                            .get(url)
                            .await?,
                    )
                }
                .await;
                user["email"] = emails
                    .ok()
                    .and_then(|v| {
                        v["values"].as_array().and_then(|a| {
                            a.iter()
                                .find(|e| {
                                    e["type"] == "email"
                                        && e["is_primary"] == true
                                        && e["is_confirmed"] == true
                                })
                                .and_then(|e| e.get("email"))
                                .cloned()
                        })
                    })
                    .unwrap_or(Value::Null);
            }
            Ok(user)
        })
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        let fields = [
            ("id", user["uuid"].clone()),
            ("nickname", user["username"].clone()),
            ("name", user["display_name"].clone()),
            ("email", user["email"].clone()),
            ("avatar", value(&user, &["links", "avatar", "href"])),
        ];
        Ok(mapped(user, fields))
    }
}

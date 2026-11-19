use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User,
    abstract_provider::json,
    endpoints::{mapped, value},
};
use crate::{Error, Result};
use http::{HeaderMap, HeaderValue};
use openrust::http::client::{Factory, PendingRequest};
use serde_json::Value;
pub(super) fn profile_request(http: &Factory, token: &str) -> PendingRequest {
    http.pendingRequest()
        .withToken(token)
        .withHeaders(HeaderMap::from_iter([(
            "x-restli-protocol-version".parse().unwrap(),
            HeaderValue::from_static("2.0.0"),
        )]))
        .redactPayload()
        .withoutRedirecting()
}
#[derive(Clone)]
pub struct LinkedInProvider {
    endpoints: ProviderEndpoints,
}
impl Default for LinkedInProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://www.linkedin.com/oauth/v2/authorization",
                "https://www.linkedin.com/oauth/v2/accessToken",
                "https://api.linkedin.com/v2/me",
            )
            .email("https://api.linkedin.com/v2/emailAddress"),
        }
    }
}
impl LinkedInProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for LinkedInProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["r_liteprofile", "r_emailaddress"]
    }
    fn scopeSeparator(&self) -> &str {
        " "
    }
    fn getUserByToken<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let mut fields = vec![
                "id",
                "firstName",
                "lastName",
                "profilePicture(displayImage~:playableStreams)",
            ];
            if scopes.iter().any(|s| s == "r_liteprofile") {
                fields.push("vanityName");
            }
            let mut user = json(
                profile_request(http, token)
                    .withQueryParameters([("projection".into(), format!("({})", fields.join(",")))])
                    .get(&self.endpoints.user)
                    .await?,
            )?;
            let email = self
                .endpoints
                .email
                .as_deref()
                .ok_or(Error::Configuration("LinkedIn email URL"))?;
            let emails = json(
                profile_request(http, token)
                    .withQueryParameters([
                        ("q".into(), "members".into()),
                        ("projection".into(), "(elements*(handle~))".into()),
                    ])
                    .get(email)
                    .await?,
            )?;
            let email = value(&emails, &["elements", "0", "handle~"]);
            let user = user.as_object_mut().ok_or(Error::InvalidResponse)?;
            if let Some(email) = email.as_object() {
                user.extend(email.clone());
            }
            Ok(Value::Object(user.clone()))
        })
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        let locale = format!(
            "{}_{}",
            value(&user, &["firstName", "preferredLocale", "language"])
                .as_str()
                .unwrap_or(""),
            value(&user, &["firstName", "preferredLocale", "country"])
                .as_str()
                .unwrap_or("")
        );
        let first = value(&user, &["firstName", "localized", &locale]);
        let last = value(&user, &["lastName", "localized", &locale]);
        let image = |width: u64| -> Value {
            let images = value(&user, &["profilePicture", "displayImage~", "elements"]);
            images
                .as_array()
                .and_then(|images| {
                    images
                        .iter()
                        .find(|image| {
                            let data = value(
                                image,
                                &["data", "com.linkedin.digitalmedia.mediaartifact.StillImage"],
                            );
                            let storage = value(&data, &["storageSize", "width"]);
                            let actual = if storage.is_null() {
                                value(&data, &["displaySize", "width"])
                            } else {
                                storage
                            };
                            actual.as_u64() == Some(width)
                        })
                        .map(|image| value(image, &["identifiers", "0", "identifier"]))
                })
                .unwrap_or(Value::Null)
        };
        let fields = [
            ("id", user["id"].clone()),
            ("nickname", Value::Null),
            (
                "name",
                format!(
                    "{} {}",
                    first.as_str().unwrap_or(""),
                    last.as_str().unwrap_or("")
                )
                .into(),
            ),
            ("first_name", first),
            ("last_name", last),
            ("email", user["emailAddress"].clone()),
            ("avatar", image(100)),
            ("avatar_original", image(800)),
        ];
        Ok(mapped(user, fields))
    }
}

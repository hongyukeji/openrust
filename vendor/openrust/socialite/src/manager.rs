use crate::{
    Error, Provider, Result, one,
    two::{
        AbstractProvider, BitbucketProvider, FacebookProvider, GithubProvider, GitlabProvider,
        GoogleProvider, LinkedInOpenIdProvider, LinkedInProvider, ProviderConfig,
        ProviderDefinition, QqProvider, SlackOpenIdProvider, SlackProvider, TwitterProvider,
        WeixinProvider, WeixinWebProvider, XProvider,
    },
};
use openrust::{
    contracts::config::Repository,
    http::{Request, client::Factory},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
/// Application-local provider factories. Each result borrows one explicit request.
pub struct SocialiteManager {
    config: Arc<dyn Repository>,
    http: Arc<Factory>,
    oauth1: RwLock<BTreeMap<String, one::TwitterProvider>>,
    extensions: RwLock<BTreeMap<String, Arc<dyn ProviderDefinition>>>,
}
impl SocialiteManager {
    pub fn new(config: Arc<dyn Repository>, http: Arc<Factory>) -> Self {
        Self {
            config,
            http,
            oauth1: RwLock::default(),
            extensions: RwLock::default(),
        }
    }
    /// Mini Program code exchange follows EasyWeChat, independently of browser OAuth.
    pub fn miniApp(&self) -> Result<crate::mini_app::MiniApp> {
        crate::mini_app::MiniApp::new(
            &self
                .config
                .get("services.weixin_mini_app")
                .unwrap_or(serde_json::Value::Null),
            self.http.clone(),
        )
    }
    pub fn extend(&self, name: impl Into<String>, provider: Arc<dyn ProviderDefinition>) {
        self.extensions
            .write()
            .expect("Socialite providers")
            .insert(name.into(), provider);
    }
    pub fn extendOAuth1(&self, name: impl Into<String>, provider: one::TwitterProvider) {
        self.oauth1
            .write()
            .expect("Socialite providers")
            .insert(name.into(), provider);
    }
    pub fn driver<'a>(&self, name: &str, request: &'a mut Request) -> Result<Provider<'a>> {
        let value = match name {
            "twitter-oauth-2" => self
                .config
                .get("services.twitter")
                .filter(|v| !v.is_null())
                .or_else(|| self.config.get("services.twitter-oauth-2")),
            "x" => self
                .config
                .get("services.x")
                .filter(|v| !v.is_null())
                .or_else(|| self.config.get("services.x-oauth-2")),
            _ => self.config.get(&format!("services.{name}")),
        }
        .unwrap_or(serde_json::Value::Null);
        if name == "gitlab"
            && value
                .get("host")
                .is_some_and(|host| !host.is_null() && !host.is_string())
        {
            return Err(Error::Configuration("host"));
        }
        let definition = self
            .extensions
            .read()
            .expect("Socialite providers")
            .get(name)
            .cloned()
            .or_else(|| match name {
                "twitter" if value.get("oauth").and_then(serde_json::Value::as_i64) == Some(2) => {
                    Some(Arc::new(TwitterProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "twitter-oauth-2" => {
                    Some(Arc::new(TwitterProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "x" => Some(Arc::new(XProvider::default()) as Arc<dyn ProviderDefinition>),
                "facebook" => {
                    Some(Arc::new(FacebookProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "google" => {
                    Some(Arc::new(GoogleProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "weixin" => {
                    Some(Arc::new(WeixinProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "weixinweb" => {
                    Some(Arc::new(WeixinWebProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "qq" => Some(Arc::new(QqProvider::default()) as Arc<dyn ProviderDefinition>),
                "github" => {
                    Some(Arc::new(GithubProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "gitlab" => {
                    let mut provider = GitlabProvider::default();
                    if let Some(host) = value.get("host").and_then(serde_json::Value::as_str) {
                        provider.setHost(host);
                    }
                    Some(Arc::new(provider) as Arc<dyn ProviderDefinition>)
                }
                "bitbucket" => {
                    Some(Arc::new(BitbucketProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "linkedin" => {
                    Some(Arc::new(LinkedInProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "linkedin-openid" => {
                    Some(Arc::new(LinkedInOpenIdProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                "slack" => Some(Arc::new(SlackProvider::default()) as Arc<dyn ProviderDefinition>),
                "slack-openid" => {
                    Some(Arc::new(SlackOpenIdProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                _ => None,
            });
        if definition.is_none()
            && name != "twitter"
            && !self
                .oauth1
                .read()
                .expect("Socialite providers")
                .contains_key(name)
        {
            return Err(Error::UnsupportedDriver(name.into()));
        }
        let mut config = ProviderConfig::from_config(&value)?;
        if config.redirect.starts_with('/') {
            let root = self
                .config
                .get("app.url")
                .unwrap_or(serde_json::Value::Null);
            let root = root.as_str().ok_or(Error::Configuration("app.url"))?;
            let url = url::Url::parse(root).map_err(|_| Error::Configuration("app.url"))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(Error::Configuration("app.url"));
            }
            config.redirect = format!(
                "{}/{}",
                root.trim_end_matches('/'),
                config.redirect.trim_start_matches('/')
            );
        }
        if let Some(provider) = definition {
            return Ok(Provider::Two(Box::new(AbstractProvider::new(
                request,
                config,
                self.http.clone(),
                provider,
            ))));
        }
        let definition = self
            .oauth1
            .read()
            .expect("Socialite providers")
            .get(name)
            .cloned()
            .or_else(|| (name == "twitter").then(one::TwitterProvider::default))
            .ok_or_else(|| Error::UnsupportedDriver(name.into()))?;
        let scope = match value.get("scope") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(v)) => Some(v.clone()),
            _ => return Err(Error::Configuration("scope")),
        };
        Ok(Provider::One(Box::new(one::AbstractProvider::new(
            request,
            config,
            self.http.clone(),
            definition,
            scope,
        ))))
    }
    pub fn with<'a>(&self, name: &str, request: &'a mut Request) -> Result<Provider<'a>> {
        self.driver(name, request)
    }
}

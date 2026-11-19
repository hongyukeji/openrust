use crate::{
    Error, Result,
    two::{AbstractProvider, GithubProvider, ProviderConfig, ProviderDefinition},
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
    extensions: RwLock<BTreeMap<String, Arc<dyn ProviderDefinition>>>,
}
impl SocialiteManager {
    pub fn new(config: Arc<dyn Repository>, http: Arc<Factory>) -> Self {
        Self {
            config,
            http,
            extensions: RwLock::default(),
        }
    }
    pub fn extend(&self, name: impl Into<String>, provider: Arc<dyn ProviderDefinition>) {
        self.extensions
            .write()
            .expect("Socialite providers")
            .insert(name.into(), provider);
    }
    pub fn driver<'a>(&self, name: &str, request: &'a mut Request) -> Result<AbstractProvider<'a>> {
        let provider = self
            .extensions
            .read()
            .expect("Socialite providers")
            .get(name)
            .cloned()
            .or_else(|| match name {
                "github" => {
                    Some(Arc::new(GithubProvider::default()) as Arc<dyn ProviderDefinition>)
                }
                _ => None,
            })
            .ok_or_else(|| Error::UnsupportedDriver(name.into()))?;
        let value = self
            .config
            .get(&format!("services.{name}"))
            .unwrap_or(serde_json::Value::Null);
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
        Ok(AbstractProvider::new(
            request,
            config,
            self.http.clone(),
            provider,
        ))
    }
    pub fn with<'a>(&self, name: &str, request: &'a mut Request) -> Result<AbstractProvider<'a>> {
        self.driver(name, request)
    }
}

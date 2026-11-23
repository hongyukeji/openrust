use crate::{
    Provider as ProtocolProvider, Result,
    two::{AbstractProvider, ProviderFuture, User as SocialiteUser},
};
use openrust::http::{RedirectResponse, Request};
use serde_json::Value;
pub trait Factory: Send + Sync {
    fn driver<'a>(&self, name: &str, request: &'a mut Request) -> Result<ProtocolProvider<'a>>;
}
pub trait Provider {
    fn redirect(&mut self) -> ProviderFuture<'_, RedirectResponse>;
    fn user(&mut self) -> ProviderFuture<'_, SocialiteUser>;
}
pub trait User {
    fn get_id(&self) -> &Value;
    fn get_nickname(&self) -> Option<&str>;
    fn get_name(&self) -> Option<&str>;
    fn get_email(&self) -> Option<&str>;
    fn get_avatar(&self) -> Option<&str>;
}
impl Factory for crate::SocialiteManager {
    fn driver<'a>(&self, name: &str, request: &'a mut Request) -> Result<ProtocolProvider<'a>> {
        self.driver(name, request)
    }
}
impl Provider for AbstractProvider<'_> {
    fn redirect(&mut self) -> ProviderFuture<'_, RedirectResponse> {
        Box::pin(async move { self.redirect() })
    }
    fn user(&mut self) -> ProviderFuture<'_, SocialiteUser> {
        Box::pin(self.user())
    }
}
impl User for SocialiteUser {
    fn get_id(&self) -> &Value {
        self.get_id()
    }
    fn get_nickname(&self) -> Option<&str> {
        self.get_nickname()
    }
    fn get_name(&self) -> Option<&str> {
        self.get_name()
    }
    fn get_email(&self) -> Option<&str> {
        self.get_email()
    }
    fn get_avatar(&self) -> Option<&str> {
        self.get_avatar()
    }
}

impl Provider for ProtocolProvider<'_> {
    fn redirect(&mut self) -> ProviderFuture<'_, RedirectResponse> {
        Box::pin(self.redirect())
    }
    fn user(&mut self) -> ProviderFuture<'_, SocialiteUser> {
        Box::pin(self.user())
    }
}
impl Provider for crate::one::AbstractProvider<'_> {
    fn redirect(&mut self) -> ProviderFuture<'_, RedirectResponse> {
        Box::pin(self.redirect())
    }
    fn user(&mut self) -> ProviderFuture<'_, SocialiteUser> {
        Box::pin(self.user())
    }
}

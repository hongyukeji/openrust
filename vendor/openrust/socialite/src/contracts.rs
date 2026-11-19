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
    fn getId(&self) -> &Value;
    fn getNickname(&self) -> Option<&str>;
    fn getName(&self) -> Option<&str>;
    fn getEmail(&self) -> Option<&str>;
    fn getAvatar(&self) -> Option<&str>;
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
    fn getId(&self) -> &Value {
        self.getId()
    }
    fn getNickname(&self) -> Option<&str> {
        self.getNickname()
    }
    fn getName(&self) -> Option<&str> {
        self.getName()
    }
    fn getEmail(&self) -> Option<&str> {
        self.getEmail()
    }
    fn getAvatar(&self) -> Option<&str> {
        self.getAvatar()
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

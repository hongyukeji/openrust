use crate::{
    Result,
    two::{AbstractProvider, ProviderFuture, User as SocialiteUser},
};
use openrust::http::{RedirectResponse, Request};
use serde_json::Value;
pub trait Factory: Send + Sync {
    fn driver<'a>(&self, name: &str, request: &'a mut Request) -> Result<AbstractProvider<'a>>;
}
pub trait Provider {
    fn redirect(&mut self) -> Result<RedirectResponse>;
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
    fn driver<'a>(&self, name: &str, request: &'a mut Request) -> Result<AbstractProvider<'a>> {
        self.driver(name, request)
    }
}
impl Provider for AbstractProvider<'_> {
    fn redirect(&mut self) -> Result<RedirectResponse> {
        self.redirect()
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

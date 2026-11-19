use crate::{Error, Result, one, two};
/// Unified protocol entry. Redirecting OAuth1 performs a temporary-credentials HTTP request.
pub enum Provider<'a> {
    One(Box<one::AbstractProvider<'a>>),
    Two(Box<two::AbstractProvider<'a>>),
}
impl<'a> Provider<'a> {
    pub async fn redirect(&mut self) -> Result<openrust::http::RedirectResponse> {
        match self {
            Self::One(p) => p.redirect().await,
            Self::Two(p) => p.redirect(),
        }
    }
    pub async fn user(&mut self) -> Result<two::User> {
        match self {
            Self::One(p) => p.user().await,
            Self::Two(p) => p.user().await,
        }
    }
    pub fn oauth1(self) -> Result<one::AbstractProvider<'a>> {
        match self {
            Self::One(p) => Ok(*p),
            Self::Two(_) => Err(Error::Protocol("OAuth1")),
        }
    }
    pub fn oauth2(self) -> Result<two::AbstractProvider<'a>> {
        match self {
            Self::Two(p) => Ok(*p),
            Self::One(_) => Err(Error::Protocol("OAuth2")),
        }
    }
}

use crate::{Result, SocialiteManager, two::AbstractProvider};
use openrust::{container::ServiceKey, http::Request, support::facades::Facade};
/// Resolves the manager from the current application's request/task context.
pub struct Socialite;
impl Facade for Socialite {
    type Service = SocialiteManager;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<SocialiteManager>()
    }
}
impl Socialite {
    pub fn driver<'a>(name: &str, request: &'a mut Request) -> Result<AbstractProvider<'a>> {
        Self::getFacadeRoot()?.driver(name, request)
    }
    pub fn with<'a>(name: &str, request: &'a mut Request) -> Result<AbstractProvider<'a>> {
        Self::driver(name, request)
    }
}

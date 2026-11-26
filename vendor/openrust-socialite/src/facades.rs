use crate::{Provider, Result, SocialiteManager};
use openrust::{container::ServiceKey, http::Request, support::facades::Facade};
/// Resolves the manager from the current application's request/task context.
pub struct Socialite;
impl Facade for Socialite {
    type Service = SocialiteManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<SocialiteManager>()
    }
}
impl Socialite {
    pub fn mini_app() -> Result<crate::wechat::MiniApp> {
        Self::get_facade_root()?.mini_app()
    }
    pub fn driver<'a>(name: &str, request: &'a mut Request) -> Result<Provider<'a>> {
        Self::get_facade_root()?.driver(name, request)
    }
    pub fn with<'a>(name: &str, request: &'a mut Request) -> Result<Provider<'a>> {
        Self::driver(name, request)
    }
}

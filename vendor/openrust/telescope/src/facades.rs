use openrust::{container::ServiceKey, support::facades::Facade};
pub struct Telescope;
impl Facade for Telescope {
    type Service = crate::Telescope;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<crate::Telescope>()
    }
}

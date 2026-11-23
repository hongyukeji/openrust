use openrust::{container::ServiceKey, support::facades::Facade};
use std::marker::PhantomData;
pub(crate) struct Resolver<T: ?Sized>(PhantomData<fn() -> T>);
impl<T: ?Sized + Send + Sync + 'static> Facade for Resolver<T> {
    type Service = T;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<T>()
    }
}
pub(crate) fn internal(
    error: impl std::error::Error + Send + Sync + 'static,
) -> openrust::http::HttpError {
    openrust::http::HttpError::Internal {
        source: Box::new(error),
    }
}

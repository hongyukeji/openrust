use openrust::{
    foundation::Application,
    http::{HttpError, HttpFuture, Request},
    support::{ProviderResult, ServiceProvider},
};
use openrust_horizon::Horizon;

pub(crate) struct HorizonServiceProvider;
impl ServiceProvider for HorizonServiceProvider {
    fn boot<'a>(&'a mut self, _app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Horizon::auth(authorize)?;
            Ok(())
        })
    }
}
fn authorize(_request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    // Apply your application's operator authorization policy here.
    Box::pin(async { Ok(false) })
}

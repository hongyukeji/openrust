use openrust::contracts::foundation::Application as _;
use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
pub fn greeting() -> &'static str {
    "package"
}
struct Provider;
impl ServiceProvider for Provider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.load_views_from(
                app,
                &app.base_path().join("packages/native/views"),
                "native",
            )
            .await?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(Provider)]
}

#[allow(dead_code)]
fn private_function() {}

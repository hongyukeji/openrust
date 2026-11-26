//! 同一主键／认证标识夹具通过原生Fortify控制器消费，避免只验证底层辅助方法。
#[path = "../../openrust-framework/tests/support/email_verification_identity.rs"]
mod fixture;
struct Fortify;
struct EmailModels<const K: u8>;
impl<const K: u8> openrust::support::ServiceProvider for EmailModels<K> {
    fn boot<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            app.make::<openrust_fortify::Fortify>()
                .await?
                .register_email_verification::<fixture::Principal<K>>();
            Ok(())
        })
    }
}
impl fixture::Flow for Fortify {
    fn providers<const K: u8>() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
        vec![
            Box::new(openrust_fortify::FortifyServiceProvider),
            Box::new(EmailModels::<K>),
        ]
    }
    fn register_routes<const K: u8>(
        _: &mut openrust::routing::Router,
    ) -> Result<(), openrust::routing::RoutingError> {
        Ok(())
    }
}
#[tokio::test]
async fn fortify_verification_uses_model_keys_and_preserves_native_idempotence() {
    fixture::verify::<0, Fortify>().await;
    fixture::verify::<1, Fortify>().await;
    fixture::verify::<2, Fortify>().await;
}

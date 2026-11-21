#[path = "../stubs/horizon_service_provider.rs"]
mod installed_provider;

#[test]
fn published_application_provider_implements_the_native_service_provider_contract() {
    fn accepts_provider(_: impl openrust::support::ServiceProvider) {}
    accepts_provider(installed_provider::HorizonServiceProvider);
}

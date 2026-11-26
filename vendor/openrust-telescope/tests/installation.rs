#[path = "../stubs/telescope_service_provider.rs"]
mod installed_provider;
#[test]
fn published_provider_uses_native_application_services() {
    fn accepts_provider(_: impl openrust::support::ServiceProvider) {}
    accepts_provider(installed_provider::TelescopeServiceProvider);
}

use openrust::support::ServiceProvider;

use crate::app::providers::AppServiceProvider;

/// The explicit list of application service providers shared by HTTP and Artisan.
pub(super) fn providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(AppServiceProvider)]
}

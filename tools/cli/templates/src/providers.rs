//! 应用 Provider 清单，对应 Laravel 的 `bootstrap/providers.php`。

use openrust::support::ServiceProvider;

pub(super) fn application_providers(
    _config: &super::Configuration,
) -> Vec<Box<dyn ServiceProvider>> {
    let mut providers: Vec<Box<dyn ServiceProvider>> = Vec::new();
    providers.push(Box::new(crate::app::providers::AppServiceProvider));
{{application_providers}}
    providers
}

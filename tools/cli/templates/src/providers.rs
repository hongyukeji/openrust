//! 应用启动时加载的 Provider 清单。

use openrust::support::ServiceProvider;

pub(super) fn application_providers(
    {{config_argument}}: &super::Configuration,
) -> Vec<Box<dyn ServiceProvider>> {
{{unused_config_guard}}    let mut providers: Vec<Box<dyn ServiceProvider>> = Vec::new();
    providers.push(Box::new(crate::app::providers::AppServiceProvider));
{{application_providers}}
    providers
}

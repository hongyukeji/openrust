//! 展示环境匹配如何在首次解析时选择编译类。

use openrust::container::attributes::Bind;
use openrust::container::{ClassDescriptor, ServiceKey};
use openrust::core::ApiResponse;
use openrust::http::Inject;
use std::sync::Arc;

pub(crate) trait EnvironmentBinding: Send + Sync {
    fn implementation(&self) -> &'static str;
}

struct DefaultBinding;

impl EnvironmentBinding for DefaultBinding {
    fn implementation(&self) -> &'static str {
        "fallback"
    }
}

struct TestingBinding;

impl EnvironmentBinding for TestingBinding {
    fn implementation(&self) -> &'static str {
        "testing"
    }
}

pub(crate) fn descriptors() -> [ClassDescriptor; 3] {
    [
        ClassDescriptor::interface::<dyn EnvironmentBinding>("Modules/Example/EnvironmentBinding")
            .with_bind(Bind::new(ServiceKey::typed::<DefaultBinding>()))
            .with_bind(Bind::for_environments(
                ServiceKey::typed::<TestingBinding>(),
                ["testing"],
            )),
        ClassDescriptor::concrete::<DefaultBinding>(
            "Modules/Example/EnvironmentBindingFallback",
            vec![],
            |_| Ok(Arc::new(DefaultBinding)),
        )
        .with_projection(openrust::class_projection!(DefaultBinding => dyn EnvironmentBinding)),
        ClassDescriptor::concrete::<TestingBinding>(
            "Modules/Example/EnvironmentBindingTesting",
            vec![],
            |_| Ok(Arc::new(TestingBinding)),
        )
        .with_projection(openrust::class_projection!(TestingBinding => dyn EnvironmentBinding)),
    ]
}

#[derive(serde::Serialize)]
pub(crate) struct BindingSelection {
    implementation: &'static str,
}

pub(crate) async fn show(
    Inject(binding): Inject<dyn EnvironmentBinding>,
) -> ApiResponse<BindingSelection> {
    ApiResponse::success(BindingSelection {
        implementation: binding.implementation(),
    })
}

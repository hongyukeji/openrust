//! 展示环境绑定及声明式应用、请求生命周期。

use openrust::config::AppConfig;
use openrust::container::attributes::{Bind, BindWhen};
use openrust::container::{ClassDescriptor, Resolver, ServiceKey};
use openrust::core::ApiResponse;
use openrust::http::Inject;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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

struct ConditionalBinding;

impl EnvironmentBinding for ConditionalBinding {
    fn implementation(&self) -> &'static str {
        "conditional"
    }
}

#[derive(Default)]
pub(crate) struct ApplicationVisits(AtomicUsize);

#[derive(Default)]
pub(crate) struct RequestVisits(AtomicUsize);

pub(crate) fn descriptors() -> [ClassDescriptor; 6] {
    [
        ClassDescriptor::interface::<dyn EnvironmentBinding>("Modules/Example/EnvironmentBinding")
            .with_bind(Bind::new(ServiceKey::typed::<DefaultBinding>()))
            .with_bind_when(BindWhen::new(
                ServiceKey::typed::<ConditionalBinding>(),
                |resolver: Resolver| async move {
                    if resolver.section::<AppConfig>("app")?.env.as_str() != "conditional" {
                        return Ok(false);
                    }
                    resolver
                        .bind::<dyn EnvironmentBinding, _>(|_| {
                            Box::pin(async { Ok(Arc::new(DefaultBinding) as _) })
                        })
                        .await?;
                    Ok(true)
                },
            ))
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
        ClassDescriptor::concrete::<ConditionalBinding>(
            "Modules/Example/EnvironmentBindingConditional",
            vec![],
            |_| Ok(Arc::new(ConditionalBinding)),
        )
        .with_projection(openrust::class_projection!(ConditionalBinding => dyn EnvironmentBinding)),
        ClassDescriptor::concrete::<ApplicationVisits>(
            "Modules/Example/ApplicationVisits",
            vec![],
            |_| Ok(Arc::new(ApplicationVisits::default())),
        )
        .with_singleton(),
        ClassDescriptor::concrete::<RequestVisits>("Modules/Example/RequestVisits", vec![], |_| {
            Ok(Arc::new(RequestVisits::default()))
        })
        .with_scoped(),
    ]
}

#[derive(serde::Serialize)]
pub(crate) struct BindingSelection {
    implementation: &'static str,
    application_visits: usize,
    request_visits: [usize; 2],
}

pub(crate) async fn show(
    Inject(binding): Inject<dyn EnvironmentBinding>,
    Inject(application): Inject<ApplicationVisits>,
    Inject(first): Inject<RequestVisits>,
    Inject(second): Inject<RequestVisits>,
) -> ApiResponse<BindingSelection> {
    ApiResponse::success(BindingSelection {
        implementation: binding.implementation(),
        application_visits: application.0.fetch_add(1, Ordering::Relaxed) + 1,
        request_visits: [
            first.0.fetch_add(1, Ordering::Relaxed) + 1,
            second.0.fetch_add(1, Ordering::Relaxed) + 1,
        ],
    })
}

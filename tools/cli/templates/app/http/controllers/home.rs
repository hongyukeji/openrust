//! 默认应用控制器；可从 `routes/api.rs` 直接注册。

use openrust::Result;
use openrust::container::attributes::{Config, Give};
use openrust::container::{
    ClassAttribute, ClassDescriptor, ConstructorParam, ContextualAttribute, ParameterKey,
    ParameterOverrides, ParameterValue, PhpBytes, PhpTarget, ResolvedArgs, Resolver, SelfBuilding,
    ServiceKey,
};
use openrust::core::ApiResponse;
use openrust::foundation::ApplicationState;
use openrust::http::axum::extract::State;
use openrust::support::BoxFuture;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) trait HomeResponse: Send + Sync {
    fn response(&self) -> ApiResponse<BTreeMap<&'static str, String>>;
}

pub(crate) struct HomeConstructionInput;

impl ContextualAttribute for HomeConstructionInput {}

pub(crate) struct HomeConstructionMarker;

pub(crate) struct HomeConstruction {
    ready: bool,
    attribute_observed: AtomicBool,
}

impl HomeConstruction {
    const SYMBOL: &'static str = concat!(module_path!(), "::HomeConstruction");
    pub(crate) const INPUT_PARAMETER: &'static str = "ready";

    pub(crate) fn descriptor() -> ClassDescriptor {
        ClassDescriptor::self_building::<Self>(
            Self::SYMBOL,
            Vec::new(),
            |_| Ok(Arc::new(Self::new(false))),
            vec![
                ConstructorParam::scalar::<bool>(Self::INPUT_PARAMETER)
                    .with_attribute(ClassAttribute::contextual(|| Ok(HomeConstructionInput))),
            ],
        )
        .with_attribute(ClassAttribute::marker(|| Ok(HomeConstructionMarker)))
    }

    fn new(ready: bool) -> Self {
        Self {
            ready,
            attribute_observed: AtomicBool::new(false),
        }
    }

    pub(crate) fn observe_attribute(&self) {
        self.attribute_observed.store(true, Ordering::Relaxed);
    }

    pub(crate) fn verify(&self) -> Result<()> {
        if self.ready && self.attribute_observed.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err(openrust::Error::custom(
                "home_construction_incomplete",
                "首页构造依赖未完成属性处理",
            ))
        }
    }
}

impl SelfBuilding for HomeConstruction {
    fn new_instance(
        args: ResolvedArgs<'_>,
        _resolver: Resolver,
    ) -> BoxFuture<'static, Result<Arc<Self>>> {
        let ready = args.value(0).and_then(|value| value.downcast::<bool>());
        Box::pin(async move { Ok(Arc::new(Self::new(*ready?))) })
    }
}

pub(crate) struct HomePresenter {
    name: Arc<String>,
}

impl HomePresenter {
    pub(crate) const SYMBOL: &'static str = concat!(module_path!(), "::HomePresenter");
    pub(crate) const NAME_PARAMETER: &'static str = "name";

    pub(crate) fn new(name: Arc<String>) -> Self {
        Self { name }
    }

    pub(crate) fn descriptor() -> ClassDescriptor {
        ClassDescriptor::concrete::<Self>(
            Self::SYMBOL,
            vec![
                ConstructorParam::php(Self::NAME_PARAMETER, PhpTarget::Bytes).with_attribute(
                    ClassAttribute::contextual(|| {
                        Ok(Config::<String>::new("app.name").with_default(String::new()))
                    }),
                ),
                ConstructorParam::class::<HomeConstruction>("construction").with_attribute(
                    ClassAttribute::contextual(|| {
                        Ok(Give::<HomeConstruction>::new(ServiceKey::named(
                            HomeConstruction::SYMBOL,
                        )))
                    }),
                ),
            ],
            |args| {
                args.service::<HomeConstruction>(1)?.verify()?;
                let name = args.value(0)?.downcast::<PhpBytes>()?;
                let name = std::str::from_utf8(name.as_bytes()).map_err(|_| {
                    openrust::Error::custom("home_name_invalid_utf8", "应用名称需要有效的 UTF-8")
                })?;
                Ok(Arc::new(Self::new(Arc::new(name.to_owned()))))
            },
        )
        .with_projection(openrust::class_projection!(HomePresenter => dyn HomeResponse))
    }
}

impl HomeResponse for HomePresenter {
    fn response(&self) -> ApiResponse<BTreeMap<&'static str, String>> {
        ApiResponse::success(BTreeMap::from([("name", self.name.as_ref().clone())]))
    }
}

/// 返回应用名称。
pub(crate) async fn index(
    State(state): State<ApplicationState>,
) -> Result<ApiResponse<BTreeMap<&'static str, String>>> {
    let mut parameters = ParameterOverrides::new();
    let _ = parameters.insert(
        ParameterKey::name(HomePresenter::NAME_PARAMETER),
        ParameterValue::new(state.config().app.name.clone()),
    );
    let presenter = state.resolve_with::<dyn HomeResponse>(parameters).await?;
    Ok(presenter.response())
}

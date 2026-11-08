//! 默认应用控制器；可从 `routes/api.rs` 直接注册。

use openrust::Result;
use openrust::container::{
    ClassDescriptor, ConstructorParam, ParameterKey, ParameterOverrides, ParameterValue, PhpBytes,
    PhpTarget,
};
use openrust::core::ApiResponse;
use openrust::foundation::ApplicationState;
use openrust::http::axum::extract::State;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) trait HomeResponse: Send + Sync {
    fn response(&self) -> ApiResponse<BTreeMap<&'static str, String>>;
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
                ConstructorParam::php(Self::NAME_PARAMETER, PhpTarget::Bytes)
                    .with_default(|| Ok(ParameterValue::new(String::new()))),
            ],
            |args| {
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

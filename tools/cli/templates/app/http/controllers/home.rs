//! 默认应用控制器；可从 `routes/api.rs` 直接注册。

use openrust::Result;
use openrust::container::{ClassDescriptor, ServiceKey};
use openrust::core::ApiResponse;
use openrust::foundation::ApplicationState;
use openrust::http::axum::extract::State;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) trait HomeResponse: Send + Sync {
    fn response(&self, name: &str) -> ApiResponse<BTreeMap<&'static str, String>>;
}

pub(crate) struct HomePresenter;

impl HomePresenter {
    pub(crate) const SYMBOL: &'static str = concat!(module_path!(), "::HomePresenter");

    pub(crate) fn descriptor() -> ClassDescriptor {
        ClassDescriptor::concrete::<Self>(Self::SYMBOL, vec![], |_| Ok(Arc::new(Self)))
            .with_projection(openrust::class_projection!(HomePresenter => dyn HomeResponse))
    }
}

impl HomeResponse for HomePresenter {
    fn response(&self, name: &str) -> ApiResponse<BTreeMap<&'static str, String>> {
        ApiResponse::success(BTreeMap::from([("name", name.to_owned())]))
    }
}

/// 返回应用名称。
pub(crate) async fn index(
    State(state): State<ApplicationState>,
) -> Result<ApiResponse<BTreeMap<&'static str, String>>> {
    let presenter = state
        .resolve_key::<dyn HomeResponse>(ServiceKey::named(HomePresenter::SYMBOL))
        .await?;
    Ok(presenter.response(&state.config().app.name))
}

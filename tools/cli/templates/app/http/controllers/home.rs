//! 默认应用控制器；可从 `routes/api.rs` 直接注册。

use openrust::container::ClassDescriptor;
use openrust::core::ApiResponse;
use openrust::foundation::ApplicationState;
use openrust::http::Inject;
use openrust::http::axum::extract::State;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct HomePresenter;

impl HomePresenter {
    pub(crate) fn descriptor() -> ClassDescriptor {
        ClassDescriptor::concrete::<Self>(
            concat!(module_path!(), "::HomePresenter"),
            vec![],
            |_| Ok(Arc::new(Self)),
        )
    }

    fn response(&self, name: &str) -> ApiResponse<BTreeMap<&'static str, String>> {
        ApiResponse::success(BTreeMap::from([("name", name.to_owned())]))
    }
}

/// 返回应用名称。
pub(crate) async fn index(
    State(state): State<ApplicationState>,
    Inject(presenter): Inject<HomePresenter>,
) -> ApiResponse<BTreeMap<&'static str, String>> {
    presenter.response(&state.config().app.name)
}

//! 应用自身的 Provider；由 bootstrap/providers.rs 登记。

use crate::app::http::controllers::home::{HomePresenter, HomeResponse};
use openrust::container::ServiceKey;
use openrust::foundation::{BootContext, RegisterContext};
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::prelude::*;
use std::sync::Arc;

/// 默认应用路由与异常呈现的启动钩子。
pub struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.bind_class::<dyn HomeResponse>(
                ServiceKey::typed::<dyn HomeResponse>(),
                ServiceKey::named(HomePresenter::SYMBOL),
            )
            .await?;
            app.resolving(Some(ServiceKey::typed::<HomePresenter>()), |event, _| {
                Box::pin(async move {
                    let Some(concrete) = event.value::<HomePresenter>()? else {
                        return Ok(());
                    };
                    let view = event.value::<dyn HomeResponse>()?.ok_or_else(|| {
                        openrust::Error::custom(
                            "home_response_view_missing",
                            "首页呈现器缺少响应视图",
                        )
                    })?;
                    let expected: Arc<dyn HomeResponse> = concrete;
                    if Arc::ptr_eq(&expected, &view) {
                        Ok(())
                    } else {
                        Err(openrust::Error::custom(
                            "home_response_view_changed",
                            "首页呈现器的响应视图必须保留同一个对象",
                        ))
                    }
                })
            })?;
            app.when(ServiceKey::typed::<HomePresenter>())?
                .needs_parameter(HomePresenter::NAME_PARAMETER)
                .give_config::<String>("app.name", Some(String::new()))?;
{{auth_register}}            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let api = app.section::<openrust::config::ApiConfig>("api")?;
            let prefix = api.normalized_prefix()?;
            app.exceptions(|handler| {
                handler.api_envelope_when(move |request, _| api.matches_path(request.path()));
            })?;
            app.routes(move |routes| crate::bootstrap::routes(routes, &prefix))?;
            Ok(())
        })
    }
}

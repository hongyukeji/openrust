//! 应用自身的 Provider；在 bootstrap 清单中显式登记。

use openrust::core::{BootContext, BoxFuture, ServiceProvider};
use openrust::prelude::*;

/// 根应用路由清单与异常呈现的启动钩子。
pub struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    #[cfg(feature = "auth")]
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<openrust::auth::AuthDrivers>() {
                return Ok(());
            }
            let drivers = app.resolve::<openrust::auth::AuthDrivers>().await?;
            drivers.provider("app-users", |resolver, config| {
                Box::pin(async move {
                    let table = config
                        .options
                        .get("table")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            openrust::Error::custom_with_status(
                                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                "app_user_table_required",
                                "应用账户Provider需要table",
                            )
                        })?;
                    let db = resolver.resolve::<openrust::db::Db>().await?;
                    let passwords = resolver.resolve::<openrust::auth::Passwords>().await?;
                    let provider = openrust::auth::DatabaseUserProvider::new(
                        (*db).clone(),
                        (*passwords).clone(),
                        table,
                    )?
                    .with_user_factory(|attributes| {
                        if attributes
                            .get("disabled_at")
                            .is_some_and(|value| !value.is_null())
                        {
                            return Err(openrust::Error::Unauthorized);
                        }
                        Ok(std::sync::Arc::new(crate::app::models::user::User::new(
                            attributes,
                        )))
                    });
                    Ok(std::sync::Arc::new(provider)
                        as std::sync::Arc<dyn openrust::auth::UserProvider>)
                })
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let api: openrust::config::ApiConfig = app.section("api")?;
            let prefix = api.normalized_prefix()?;
            app.exceptions(move |handler| {
                handler.api_envelope_when(move |request, _| api.matches_path(request.path()));
            })?;
            app.routes(move |routes| crate::bootstrap::routes(routes, &prefix))?;
            Ok(())
        })
    }
}

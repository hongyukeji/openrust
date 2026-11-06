//! 应用自身的 Provider；在 bootstrap 清单中显式登记。

use openrust::foundation::BootContext;
use openrust::prelude::*;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;

/// 根应用路由清单与异常呈现的启动钩子。
pub struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    #[cfg(feature = "auth")]
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
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
                    let db = resolver.resolve::<openrust::database::Db>().await?;
                    let passwords = resolver.resolve::<openrust::hashing::HashManager>().await?;
                    #[cfg(feature = "auth-fortify")]
                    let state_db = (*db).clone();
                    #[cfg(feature = "auth-fortify")]
                    let state_table = table.to_owned();
                    #[cfg(feature = "auth-fortify")]
                    let factor_encrypter = resolver
                        .resolve::<openrust::encryption::Encrypter>()
                        .await?;
                    let model_metadata =
                        std::sync::Arc::new(openrust::database::eloquent::ModelMetadata::new(
                            table,
                            "app::models::user::User",
                        ));
                    let provider = openrust::auth::DatabaseUserProvider::new(
                        (*db).clone(),
                        passwords.clone(),
                        table,
                    )?
                    .with_user_factory(move |attributes| {
                        if attributes
                            .get("disabled_at")
                            .is_some_and(|value| !value.is_null())
                        {
                            return Err(openrust::Error::Unauthorized);
                        }
                        let user = crate::app::models::user::User::new(attributes)
                            .with_model_metadata(model_metadata.clone());
                        #[cfg(feature = "auth-fortify")]
                        let user = {
                            let identifier =
                                openrust::auth::Authenticatable::auth_identifier(&user)?;
                            let state = openrust::auth::fortify::DatabaseTwoFactorState::new(
                                state_db.clone(),
                                state_table.clone(),
                                "id",
                                identifier,
                            )?;
                            user.with_two_factor_state(state.clone())
                                .with_factor_authority(
                                    openrust::auth::fortify::DatabaseFactorAuthority::new(
                                        state,
                                        (*factor_encrypter).clone(),
                                    ),
                                )
                        };
                        Ok(std::sync::Arc::new(user))
                    });
                    Ok(std::sync::Arc::new(provider)
                        as std::sync::Arc<dyn openrust::auth::UserProvider>)
                })
            })?;
            #[cfg(feature = "auth-fortify")]
            super::fortify::register(app).await?;
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

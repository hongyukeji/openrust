use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

pub(crate) struct UserServiceProvider;
impl ServiceProvider for UserServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move { openrust_fortify::starter::configure(app).await })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            openrust_fortify::starter::configure_mail_queue::<crate::app::models::User>(app)?;
            openrust_fortify::starter::bind_password_tokens::<crate::app::models::User>(app)?;
            openrust_fortify::starter::UserResource::use_resource::<
                crate::app::models::User,
                crate::app::http::resources::user_resource::UserResource,
            >(app)?;
            #[cfg(feature = "sanctum")]
            openrust_fortify::starter::deletion::delete_users_using::<crate::app::models::User>(
                app,
                std::sync::Arc::new(crate::app::actions::fortify::delete_user::DeleteUser),
            )?;
            #[cfg(feature = "socialite")]
            openrust_fortify::starter::socialite::create_users_using(
                app,
                std::sync::Arc::new(
                    crate::app::actions::fortify::create_user_from_provider::CreateUserFromProvider,
                ),
            )?;
            // 所有Provider已注册，先选择套件响应，再配置共享的Fortify视图回调。
            openrust_fortify::starter::StarterKitServiceProvider
                .register(app)
                .await?;
            #[cfg(feature = "sanctum")]
            openrust_fortify::starter::client::register_routes::<crate::app::models::User>(app)?;
            openrust_fortify::starter::register_views(app).await
        })
    }
}

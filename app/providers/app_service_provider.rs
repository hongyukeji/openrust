use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

/**
 * 应用服务的注册与启动入口。
 *
 * 应用启动时，先按注册顺序执行各非延迟 Provider 的 register()；
 * 全部注册完成后，再按顺序执行这些 Provider 的 boot()。
 * register() 用于向容器绑定服务；boot() 用于使用已注册的服务完成初始化。
 */
pub(crate) struct AppServiceProvider;

#[rustfmt::skip]
impl ServiceProvider for AppServiceProvider
{
    /**
     * 注册应用服务。
     */
    fn register<'a>(&'a mut self, _app: &'a Application) -> HttpFuture<'a, ProviderResult>
    {
        Box::pin(async {
            //
            Ok(())
        })
    }

    /**
     * 启动应用服务。
     */
    fn boot(&mut self, _app: &Application) -> HttpFuture<'_, ProviderResult>
    {
        Box::pin(async {
            //
            Ok(())
        })
    }
}

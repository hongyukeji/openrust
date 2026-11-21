use openrust::{
    foundation::Application,
    http::{HttpError, HttpFuture, Request},
    support::{ProviderResult, ServiceProvider},
};
use openrust_horizon::Horizon;

/**
 * Horizon 应用服务的启动入口。
 *
 * 所有非延迟 Provider 完成 register() 后，按顺序执行 boot()。
 */
pub(crate) struct HorizonServiceProvider;

impl ServiceProvider for HorizonServiceProvider {
    /**
     * 注册 Horizon 面板的访问授权规则。
     */
    fn boot<'a>(&'a mut self, _app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Horizon::auth(authorize)?;
            Ok(())
        })
    }
}
/**
 * 在此配置应用的运维人员授权规则，默认拒绝访问。
 */
fn authorize(_request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    Box::pin(async { Ok(false) })
}

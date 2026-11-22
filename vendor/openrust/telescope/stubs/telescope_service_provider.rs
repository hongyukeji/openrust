#![allow(
    non_snake_case,
    reason = "Matches Telescope application provider names"
)]
use openrust::{
    contracts::foundation::Application as _,
    foundation::Application,
    http::{HttpError, HttpFuture, Request},
    support::{ProviderResult, ServiceProvider},
};
use openrust_telescope::Telescope;

/**
 * Telescope 应用服务的启动入口。
 *
 * 所有非延迟 Provider 完成 register() 后，按顺序执行 boot()。
 */
pub(crate) struct TelescopeServiceProvider;

impl ServiceProvider for TelescopeServiceProvider {
    /**
     * 配置敏感字段隐藏、记录筛选与面板访问授权。
     */
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let telescope = app.make::<Telescope>().await?;
            let isLocal = app.config()?.get("app.env") == Some("local".into());
            if !isLocal {
                telescope.hideRequestParameters(&["_token"]);
                telescope.hideRequestHeaders(&[
                    "cookie",
                    "set-cookie",
                    "x-csrf-token",
                    "x-xsrf-token",
                ]);
            }
            telescope.filter(move |entry| {
                isLocal
                    || entry.isReportableException()
                    || entry.isFailedRequest()
                    || entry.isFailedJob()
                    || entry.isScheduledTask()
                    || entry.hasMonitoredTag()
            });
            telescope.auth(authorize);
            Ok(())
        })
    }
}
/**
 * 默认仅允许本地环境访问，其他环境需在此配置应用的 viewTelescope 授权规则。
 */
fn authorize(_: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    Box::pin(async {
        Ok(openrust::support::facades::Config::get("app.env")? == Some("local".into()))
    })
}

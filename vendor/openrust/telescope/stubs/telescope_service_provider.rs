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

pub(crate) struct TelescopeServiceProvider;
impl ServiceProvider for TelescopeServiceProvider {
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
fn authorize(_: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    Box::pin(async {
        // Add your application's viewTelescope authorization for non-local users here.
        Ok(
            openrust::support::facades::Config::get("app.env").map_err(|source| {
                HttpError::Internal {
                    source: Box::new(source),
                }
            })? == Some("local".into()),
        )
    })
}

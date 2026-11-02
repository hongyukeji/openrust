//! 显式具名 reporting 连接的连通性演示；不暴露部署地址或凭据。

use openrust::core::ApiResponse;
use openrust::database::{Databases, DbBackend};
use openrust::http::Inject;

#[derive(serde::Serialize)]
pub(crate) struct ReportingConnection {
    reachable: bool,
    backend: &'static str,
}

pub(crate) async fn show(
    Inject(databases): Inject<Databases>,
) -> openrust::Result<ApiResponse<ReportingConnection>> {
    let db = databases.get("reporting")?;
    db.primary_connection()?
        .ping()
        .await
        .map_err(openrust::Error::internal)?;
    let backend = match db.backend()? {
        DbBackend::Postgres => "postgres",
        DbBackend::MySql => "mysql",
        DbBackend::Sqlite => "sqlite",
        _ => {
            return Err(openrust::Error::custom(
                "unsupported_reporting_backend",
                "此连接后端尚未提供探测响应",
            ));
        }
    };
    Ok(ApiResponse::success(ReportingConnection {
        reachable: true,
        backend,
    }))
}

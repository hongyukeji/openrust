//! 显式具名 reporting 连接的连通性演示；不暴露部署地址或凭据。

use openrust::container::attributes::Database;
use openrust::container::{ClassAttribute, ClassDescriptor, ConstructorParam};
use openrust::core::ApiResponse;
use openrust::database::{Db, DbBackend};
use openrust::http::Inject;
use std::sync::Arc;

pub(crate) struct ReportingDatabase(Arc<Db>);

pub(crate) fn reporting_descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete::<ReportingDatabase>(
        "Modules/Example/ReportingDatabase",
        vec![
            ConstructorParam::class::<Db>("database").with_attribute(ClassAttribute::contextual(
                || Ok(Database::new("reporting")),
            )),
        ],
        |args| Ok(Arc::new(ReportingDatabase(args.service::<Db>(0)?))),
    )
}

#[derive(serde::Serialize)]
pub(crate) struct ReportingConnection {
    reachable: bool,
    backend: &'static str,
}

pub(crate) async fn show(
    Inject(reporting): Inject<ReportingDatabase>,
) -> openrust::Result<ApiResponse<ReportingConnection>> {
    let db = &reporting.0;
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

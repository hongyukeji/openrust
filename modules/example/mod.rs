//! 普通业务模块，通过应用注册清单接入框架；全部业务资源在本目录内。

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub(crate) mod database;
#[cfg(test)]
mod extensions;
#[cfg(feature = "grpc")]
#[allow(dead_code)]
mod grpc;
mod http;
#[cfg(feature = "queue")]
pub(crate) mod jobs;
pub(crate) mod providers;
mod routes;
mod services;
#[cfg(test)]
#[path = "tests/registration.rs"]
mod tests;

/// 本模块的编译描述；启用状态由应用 `ModuleManifest` 决定。
pub(crate) fn descriptor(
    uses_database: bool,
    application_root: std::path::PathBuf,
) -> openrust::core::ModuleDescriptor {
    let descriptor = openrust::core::ModuleDescriptor::new(
        "example",
        application_root.join("modules/example"),
        0,
    )
    .provider(providers::ExampleServiceProvider::new(application_root));
    #[cfg(feature = "queue")]
    let descriptor = if uses_database {
        descriptor.provider(jobs::JobsServiceProvider)
    } else {
        descriptor
    };
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    let descriptor = if uses_database {
        descriptor
            .provider(
                openrust::db::MigrationServiceProvider::<database::migrations::Migrator>::new()
                    .for_module("example"),
            )
            .provider(openrust::db::SeedServiceProvider::new(
                database::seeders::run,
            ))
    } else {
        descriptor
    };
    #[cfg(not(any(feature = "queue", feature = "db-postgres", feature = "db-mysql")))]
    let _ = uses_database;
    descriptor
}

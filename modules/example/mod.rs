//! 普通业务模块，通过应用注册清单接入框架；全部业务资源在本目录内。

mod config;

#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/account_security_flows.rs"]
mod account_security_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/attachment_flows.rs"]
mod attachment_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/browser_websocket_flows.rs"]
mod browser_websocket_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "queue",
    feature = "cache-redis",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/cache_behavior_flows.rs"]
mod cache_behavior_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "queue",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/cache_flows.rs"]
mod cache_flows;
#[cfg(all(
    test,
    feature = "otel",
    feature = "metrics",
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/collector_flows.rs"]
mod collector_flows;
#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/cookie_auth_flows.rs"]
mod cookie_auth_flows;
#[cfg(test)]
#[path = "tests/cookie_flows.rs"]
mod cookie_flows;
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub(crate) mod database;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
mod events;
#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/example_record_flows.rs"]
mod example_record_flows;
#[cfg(test)]
#[path = "tests/extension_flows.rs"]
mod extension_flows;
mod extensions;
#[cfg(feature = "grpc")]
#[allow(dead_code)]
mod grpc;
#[cfg(all(test, feature = "grpc"))]
#[path = "tests/grpc_flows.rs"]
mod grpc_flows;
#[cfg(all(test, feature = "grpc"))]
#[path = "tests/grpc_tls_flows.rs"]
mod grpc_tls_flows;
mod http;
#[cfg(feature = "queue")]
pub(crate) mod jobs;
#[cfg(test)]
#[path = "tests/localization_flows.rs"]
mod localization_flows;
#[cfg(test)]
#[path = "tests/message_bag_flows.rs"]
mod message_bag_flows;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
mod models;
#[cfg(all(
    test,
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/oidc_flows.rs"]
mod oidc_flows;
#[cfg(all(
    test,
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/oidc_lifecycle_flows.rs"]
mod oidc_lifecycle_flows;
pub(crate) mod providers;
#[cfg(all(
    test,
    feature = "auth",
    feature = "queue-redis",
    feature = "cache-redis",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/queue_retry_flows.rs"]
mod queue_retry_flows;
#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/readiness_flows.rs"]
mod readiness_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "realtime-redis",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/realtime_cluster.rs"]
mod realtime_cluster;
#[cfg(all(
    test,
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/realtime_flows.rs"]
mod realtime_flows;
#[cfg(all(test, feature = "session"))]
#[path = "tests/redirect_flows.rs"]
mod redirect_flows;
mod routes;
#[cfg(all(
    test,
    feature = "auth",
    feature = "storage-s3",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/s3_flows.rs"]
mod s3_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "queue",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/scheduler_flows.rs"]
mod scheduler_flows;
#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/seed_flows.rs"]
mod seed_flows;
mod services;
#[cfg(test)]
#[path = "tests/telemetry_flows.rs"]
mod telemetry_flows;
#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/tenant_flows.rs"]
mod tenant_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "queue",
    feature = "cache-redis",
    feature = "storage-s3",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/tenant_s3_flows.rs"]
mod tenant_s3_flows;
#[cfg(test)]
#[path = "tests/registration.rs"]
mod tests;
#[cfg(all(
    test,
    feature = "auth",
    feature = "data-exchange",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/transfer_flows.rs"]
mod transfer_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "http-client",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/upstream_timeout_flows.rs"]
mod upstream_timeout_flows;
#[cfg(all(test, feature = "view"))]
#[path = "tests/view_flows.rs"]
mod view_flows;
#[cfg(all(
    test,
    feature = "auth",
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#[path = "tests/websocket_flows.rs"]
mod websocket_flows;

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
    .provider(providers::ExampleServiceProvider::new(application_root))
    .provider(extensions::PrefixCacheServiceProvider);
    #[cfg(feature = "grpc")]
    let descriptor = descriptor
        .provider(openrust::grpc::GrpcServiceProvider::new())
        .provider(grpc::ExampleGrpcServiceProvider);
    #[cfg(feature = "realtime")]
    let descriptor = descriptor.provider(openrust::realtime::RealtimeServiceProvider::new());
    #[cfg(any(
        test,
        all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))
    ))]
    let descriptor = descriptor.provider(services::welcome::WelcomeServiceProvider);
    #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
    let descriptor = if uses_database {
        descriptor.provider(events::article::provider())
    } else {
        descriptor
    };
    #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
    let descriptor = if uses_database {
        descriptor.provider(services::account_security::AccountSecurityProvider)
    } else {
        descriptor
    };
    #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
    let descriptor = if uses_database {
        descriptor.provider(services::tenant::TenantServiceProvider)
    } else {
        descriptor
    };
    #[cfg(all(
        feature = "auth-oidc",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    let descriptor = if uses_database {
        descriptor.provider(services::oidc::ExampleOidcServiceProvider)
    } else {
        descriptor
    };
    #[cfg(all(
        feature = "auth",
        feature = "data-exchange",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    let descriptor = if uses_database {
        descriptor.provider(services::article_transfer::ArticleTransferProvider)
    } else {
        descriptor
    };
    #[cfg(all(
        feature = "auth",
        feature = "storage-local",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    let descriptor = if uses_database {
        descriptor.provider(providers::ArticleAttachmentsProvider)
    } else {
        descriptor
    };
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

#[cfg(all(test, feature = "session"))]
#[path = "tests/session_flows.rs"]
mod session_flows;

#[cfg(test)]
#[path = "tests/config_sources.rs"]
mod config_sources;
#[cfg(test)]
pub(crate) use config_sources::application_sources as test_sources;

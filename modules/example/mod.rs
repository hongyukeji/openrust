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
#[cfg(test)]
#[path = "tests/module_activation_flows.rs"]
mod module_activation_flows;
mod module_init;
#[cfg(test)]
#[path = "tests/module_provider_flows.rs"]
mod module_provider_flows;
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

/// 将运行声明绑定到已编译实现；普通启动不再硬编码声明为已发现模块。
pub(crate) fn manifest(
    uses_database: bool,
    application_root: &std::path::Path,
    config: &openrust::config::ModulesConfig,
) -> openrust::core::ModuleManifest {
    use openrust::core::ServiceProvider;
    let possible =
        providers::ExampleModuleServiceProvider::new(uses_database, application_root.to_path_buf())
            .nested_providers();
    openrust::core::ModuleManifest::discover_from_config(application_root, config)
        .compiled_provider::<providers::ExampleModuleServiceProvider, _>(
            "ExampleModuleServiceProvider",
            move |context| {
                Ok(providers::ExampleModuleServiceProvider::new(
                    uses_database,
                    context.application_root().to_path_buf(),
                )
                .with_module_repository())
            },
        )
        .possible_providers(possible)
        .compiled_module_file("example", "module_init.rs", module_init::ExampleModuleInit)
}

/// 程序化测试仍可显式选择编译描述；生产启动使用 metadata 发现清单。
#[cfg(test)]
pub(crate) fn descriptor(
    uses_database: bool,
    application_root: std::path::PathBuf,
) -> openrust::core::ModuleDescriptor {
    openrust::core::ModuleDescriptor::new("example", application_root.join("modules/example"), 0)
        .provider(providers::ExampleModuleServiceProvider::new(
            uses_database,
            application_root,
        ))
}

#[cfg(all(test, feature = "session"))]
#[path = "tests/session_flows.rs"]
mod session_flows;

#[cfg(test)]
#[path = "tests/discovery_fixture.rs"]
mod discovery_fixture;
#[cfg(test)]
pub(crate) use discovery_fixture::write as write_discovery_fixture;
#[cfg(test)]
#[path = "tests/config_sources.rs"]
mod config_sources;
#[cfg(test)]
#[path = "tests/module_discovery_flows.rs"]
mod module_discovery_flows;
#[cfg(test)]
pub(crate) use config_sources::application_sources as test_sources;

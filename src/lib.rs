#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::process::ExitCode;

pub mod audit;
pub mod cache;
pub mod config;
pub mod core;
#[cfg(feature = "data-exchange")]
pub mod data;
/// 数据交换能力的兼容入口；新代码可使用 [`data::exchange`]。
#[cfg(feature = "data-exchange")]
pub use data::exchange as data_exchange;
pub mod db;
pub mod events;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod http;
pub mod i18n;
pub mod mail;
#[cfg(feature = "queue")]
pub mod notifications;
#[cfg(feature = "queue")]
pub mod queue;
#[cfg(feature = "realtime")]
pub mod realtime;
pub mod routes;
#[cfg(feature = "grpc")]
pub mod runtime;
#[cfg(feature = "queue")]
pub mod schedule;
#[cfg(feature = "encryption")]
pub mod security;
#[cfg(feature = "signing")]
pub mod signing;
pub mod sms;
pub mod storage;
pub mod tenant;
pub mod testing;
pub mod validation;
#[cfg(feature = "view")]
pub mod view;

#[cfg(feature = "auth")]
pub mod auth;

pub use core::{
    ApiResponse, App, AppState, Application, BoxFuture, Cancellation, Command, Error,
    ExecutionContext, ExecutionMetadata, MissingServiceError, NoConfig, PageMeta, Result,
    ValidationErrors, lifecycle, with_execution_context,
};
pub use macros::resource;

#[cfg(test)]
static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 数据库模块，由 facade 的 `db-postgres` 或 `db-mysql` feature 启用。
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use db as database;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
pub use db::{DatabaseConfig, DatabaseInstanceConfig, Databases, Db};

/// 扩展方法，用于在不绑定监听器的情况下测试 HTTP router。
pub use tower::ServiceExt;

/// Tokio runtime，供仅依赖 facade 的应用使用。
pub use tokio;

/// 常见 `app.rs`、controller 或 plugin 所需的类型。
pub mod prelude {
    pub use crate::audit::{Audit, AuditEvent, AuditOutcome, AuditPolicy, AuditSubject};
    #[cfg(feature = "auth")]
    pub use crate::auth::{
        AuthCredential, AuthLayer, AuthMigration, AuthMode, AuthPrincipal, AuthSource,
        Authenticator, CurrentUser, JwtConfig, JwtTokens, OptionalUser,
    };
    pub use crate::cache::{
        Cache, CacheBackend, CacheDriver, CacheDriverConfig, CacheDrivers, CacheScope, CacheStore,
    };
    #[cfg(feature = "queue")]
    pub use crate::config::QueueConfig;
    #[cfg(feature = "realtime")]
    pub use crate::config::RealtimeConfig;
    #[cfg(feature = "signing")]
    pub use crate::config::SignedLinksConfig;
    pub use crate::config::SmsConfig;
    pub use crate::config::{CacheConfig, CacheStoreConfig};
    pub use crate::events::{EventContext, Events};
    pub use crate::http::axum::extract::State;
    pub use crate::http::axum::http::StatusCode;
    pub use crate::http::axum::response::IntoResponse;
    pub use crate::http::{
        Action, Created, DomainParams, Inject, Json, NoContent, PageParams, Paginated, Path, Query,
        Resource, ResourceActions, RouteGroup, RouteUrlGenerator, Routes, Validated,
    };
    pub use crate::i18n::{
        DateFormatStyle, DateTimeFormatStyle, I18n, I18nConfig, I18nFormatError, Locale,
        MessageArgs, MessageCatalog, PluralCategory, RequestLocale,
    };
    pub use crate::mail::{Email, LocalizedEmail, MailConfig, Mailer};
    #[cfg(feature = "queue")]
    pub use crate::queue::{
        DatabaseQueue, DispatchOptions, GroupFailurePolicy, Job, JobContext, JobRegistry, Queue,
        QueueDriver, QueueMigration, QueueTask, TaskGroup,
    };
    #[cfg(feature = "realtime")]
    pub use crate::realtime::{Realtime, RealtimeMessage, RealtimeSession, RealtimeTransport};
    #[cfg(feature = "queue")]
    pub use crate::schedule::{
        CronOptions, MissedRunPolicy, ScheduleDefinition, ScheduleFrequency, ScheduleReport,
        Scheduler,
    };
    #[cfg(feature = "encryption")]
    pub use crate::security::Encryptor;
    #[cfg(feature = "signing")]
    pub use crate::signing::{
        DatabaseSignedLinkRevocationStore, SignedLink, SignedLinkClaims,
        SignedLinkRevocationMigration, SignedLinkRevocationStore, SignedLinks,
    };
    pub use crate::sms::{SmsContent, SmsDriver, SmsMessage, SmsReceipt, SmsSender};
    pub use crate::storage::{Storage, StorageDisk, StoragePath};
    pub use crate::tenant::{TenantContext, TenantId, TenantLayer};
    pub use crate::validation::{Validate, validate};
    pub use crate::{
        App, Application, BoxFuture, Cancellation, Command, Error, ExecutionContext,
        ExecutionMetadata, NoConfig, Result,
    };
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    pub use crate::{DatabaseConfig, DatabaseInstanceConfig, Databases, Db};
}

/// 启动 `A`，并运行第一个命令行参数指定的命令（默认为 `serve`）。
///
/// 返回进程退出码而不触发 panic，使 `main` 保持为单行调用。
pub fn run<A: Application>() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("cannot start tokio runtime: {error}")),
    };
    runtime.block_on(async {
        let app = match lifecycle::bootstrap::<A>().await {
            Ok(app) => app,
            Err(error) => return fail(&format!("bootstrap failed: {error}")),
        };
        let mut args = std::env::args().skip(1);
        let name = args.next().unwrap_or_else(|| "serve".to_owned());
        let result = app.run_command(&name, args.collect()).await;
        let shutdown = app
            .shutdown(
                tokio::time::Instant::now()
                    + std::time::Duration::from_secs(app.config().server.shutdown_timeout_secs),
            )
            .await;
        lifecycle::flush_telemetry().await;
        match (result, shutdown) {
            (Err(error), Err(cleanup_error)) => {
                tracing::error!(code = cleanup_error.code(), "command cleanup also failed");
                fail(&format!("{name}: {error}"))
            }
            (Err(error), Ok(())) => fail(&format!("{name}: {error}")),
            (Ok(()), Err(error)) => fail(&format!("{name} cleanup: {error}")),
            (Ok(()), Ok(())) => ExitCode::SUCCESS,
        }
    })
}

fn fail(message: &str) -> ExitCode {
    // 配置加载失败时 tracing 可能尚未安装，因此始终写入 stderr。
    eprintln!("error: {message}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::prelude::*;

    #[test]
    fn prelude_json_is_a_response() {
        let response = Json("ok").into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
    }
}

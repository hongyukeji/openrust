#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::process::ExitCode;

pub mod audit;
pub mod cache;
pub mod config;
pub mod core;
#[cfg(feature = "data-exchange")]
pub mod data_exchange;
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
#[cfg(feature = "queue")]
pub mod schedule;
#[cfg(feature = "encryption")]
pub mod security;
#[cfg(feature = "signing")]
pub mod signing;
pub mod sms;
pub mod storage;
pub mod tenant;
pub mod validation;

#[cfg(feature = "auth")]
pub mod auth;

pub use core::{
    ApiResponse, App, AppBuilder, AppState, Application, BoxFuture, Cancellation, Command, Error,
    ExecutionContext, ExecutionMetadata, MissingServiceError, NoConfig, PageMeta, Plugin, Result,
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
pub use db::{
    DatabaseConfig, DatabaseInstanceConfig, DatabasePlugin, Databases, Db, MigrationPlugin,
    SeedPlugin,
};

/// 扩展方法，用于在不绑定监听器的情况下测试 HTTP router。
pub use tower::ServiceExt;

/// Tokio runtime，供仅依赖 facade 的应用使用。
pub use tokio;

/// 常见 `app.rs`、controller 或 plugin 所需的类型。
pub mod prelude {
    pub use crate::audit::{
        Audit, AuditEvent, AuditOutcome, AuditPlugin, AuditPolicy, AuditSubject,
    };
    #[cfg(feature = "auth")]
    pub use crate::auth::{
        AuthCredential, AuthLayer, AuthMigration, AuthMode, AuthPlugin, AuthPrincipal, AuthSource,
        Authenticator, CurrentUser, JwtConfig, JwtPlugin, JwtTokens, OptionalUser,
    };
    pub use crate::cache::{
        Cache, CacheBackend, CacheDriver, CacheDriverConfig, CacheDrivers, CachePlugin, CacheScope,
        CacheStore,
    };
    #[cfg(feature = "queue")]
    pub use crate::config::QueueConfig;
    #[cfg(feature = "signing")]
    pub use crate::config::SignedLinksConfig;
    pub use crate::config::SmsConfig;
    pub use crate::config::{CacheConfig, CacheStoreConfig};
    pub use crate::events::{EventContext, Events, EventsPlugin};
    pub use crate::http::axum::extract::State;
    pub use crate::http::axum::http::StatusCode;
    pub use crate::http::axum::response::IntoResponse;
    pub use crate::http::{
        Action, AppBuilderHttpExt, Created, Inject, Json, NoContent, PageParams, Paginated, Path,
        Query, Resource, ResourceActions, Routes, Validated,
    };
    pub use crate::i18n::{
        DateFormatStyle, DateTimeFormatStyle, I18n, I18nConfig, I18nFormatError, I18nPlugin,
        Locale, MessageArgs, MessageCatalog, PluralCategory, RequestLocale,
    };
    pub use crate::mail::{Email, LocalizedEmail, MailConfig, Mailer, MailerPlugin};
    #[cfg(feature = "queue")]
    pub use crate::queue::{
        DatabaseQueue, DispatchOptions, GroupFailurePolicy, Job, JobContext, JobRegistry, Queue,
        QueueDriver, QueueMigration, QueuePlugin, QueueTask, TaskGroup,
    };
    #[cfg(feature = "queue")]
    pub use crate::schedule::{
        CronOptions, MissedRunPolicy, ScheduleDefinition, ScheduleFrequency, ScheduleReport,
        Scheduler,
    };
    #[cfg(feature = "encryption")]
    pub use crate::security::{EncryptionPlugin, Encryptor};
    #[cfg(feature = "signing")]
    pub use crate::signing::{
        DatabaseSignedLinkRevocationStore, SignedLink, SignedLinkClaims,
        SignedLinkRevocationMigration, SignedLinkRevocationStore, SignedLinks, SignedLinksPlugin,
    };
    pub use crate::sms::{SmsContent, SmsDriver, SmsMessage, SmsPlugin, SmsReceipt, SmsSender};
    pub use crate::storage::{Storage, StorageDisk, StoragePath, StoragePlugin};
    pub use crate::tenant::{TenantContext, TenantId, TenantLayer};
    pub use crate::validation::{Validate, validate};
    pub use crate::{
        App, AppBuilder, Application, BoxFuture, Cancellation, Command, Error, ExecutionContext,
        ExecutionMetadata, NoConfig, Plugin, Result,
    };
    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    pub use crate::{
        DatabaseConfig, DatabaseInstanceConfig, DatabasePlugin, Databases, Db, MigrationPlugin,
        SeedPlugin,
    };
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
        match app.run_command(&name, args.collect()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(&format!("{name}: {error}")),
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

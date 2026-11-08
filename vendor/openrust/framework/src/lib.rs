#![doc = include_str!("../README.md")]
#![doc = include_str!("doc_examples.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::process::ExitCode;

pub mod audit;
pub mod cache;
pub mod config;
pub mod console;
/// 类型化服务容器及请求作用域；应用与扩展共用同一内核。
pub mod container;
pub mod core;
#[cfg(feature = "data-exchange")]
pub mod data;
pub mod database;
#[cfg(feature = "encryption")]
pub mod encryption;
pub mod events;
pub mod filesystem;
pub mod foundation;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "hashing")]
pub mod hashing;
pub mod http;
pub mod logging;
pub mod mail;
#[cfg(feature = "queue")]
pub mod notifications;
#[cfg(feature = "queue")]
pub mod queue;
#[cfg(feature = "realtime")]
pub mod realtime;
pub mod routing;
#[cfg(feature = "grpc")]
pub mod runtime;
#[cfg(feature = "queue")]
pub mod schedule;
#[cfg(feature = "session")]
pub mod session;
#[cfg(feature = "signing")]
pub mod signing;
pub mod sms;
pub mod support;
pub mod tenant;
pub mod testing;
pub mod translation;
pub mod validation;
#[cfg(feature = "view")]
pub mod view;

#[cfg(feature = "auth")]
pub mod auth;

pub use crate::console::Command;
pub use crate::support::BoxFuture;
pub use core::ApiResponse;
pub use core::Cancellation;
pub use core::Error;
pub use core::ExecutionContext;
pub use core::ExecutionMetadata;
pub use core::NoConfig;
pub use core::PageMeta;
pub use core::Result;
pub use core::ValidationErrors;
pub use core::with_execution_context;
pub use foundation::Application;
pub use foundation::ApplicationDefinition;
pub use foundation::ApplicationState;
pub use foundation::MissingServiceError;
pub use foundation::lifecycle;
pub use macros::resource;

#[cfg(test)]
mod test_database_environment;

#[cfg(test)]
static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    let guard = ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // 必须早于Jail构造：构造自身已修改cwd，clear_env随后还会删除数据库输入。
    test_database_environment::seed();
    guard
}

/// 通用数据库模块；具体连接器由 `db-postgres` / `db-mysql` feature 选择。
pub use database::{DatabaseConfig, DatabaseInstanceConfig, Databases, Db};

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
    #[cfg(feature = "encryption")]
    pub use crate::encryption::Encrypter;
    pub use crate::events::{Event, EventContext, Events, PayloadRef};
    pub use crate::filesystem::FilesystemAdapter;
    pub use crate::filesystem::FilesystemManager;
    pub use crate::filesystem::StoragePath;
    pub use crate::http::Cookie;
    pub use crate::http::CookieFactory;
    pub use crate::http::CookieJar;
    pub use crate::http::CookieOptions;
    pub use crate::http::CookieResponseExt;
    pub use crate::http::CookieServiceProvider;
    pub use crate::http::Created;
    pub use crate::http::DomainParams;
    pub use crate::http::Inject;
    pub use crate::http::InputNormalization;
    pub use crate::http::InputOutcome;
    pub use crate::http::InputOutcomeMut;
    pub use crate::http::InputTruthy;
    pub use crate::http::Json;
    pub use crate::http::NoContent;
    pub use crate::http::PageParams;
    pub use crate::http::Paginated;
    pub use crate::http::Path;
    pub use crate::http::Query;
    pub use crate::http::Request;
    pub use crate::http::RequestInfo;
    pub use crate::http::Truthy;
    pub use crate::http::Validated;
    pub use crate::http::axum::extract::State;
    pub use crate::http::axum::http::StatusCode;
    pub use crate::http::axum::response::IntoResponse;
    pub use crate::http::handle_precognitive_requests;
    pub use crate::mail::{Email, LocalizedEmail, MailConfig, Mailer};
    #[cfg(feature = "queue")]
    pub use crate::queue::{
        DatabaseQueue, DispatchOptions, GroupFailurePolicy, Job, JobContext, JobRegistry, Queue,
        QueueDriver, QueueMigration, QueueTask, TaskGroup,
    };
    #[cfg(feature = "realtime")]
    pub use crate::realtime::{Realtime, RealtimeMessage, RealtimeSession, RealtimeTransport};
    pub use crate::routing::Action;
    pub use crate::routing::ManagedHandler;
    pub use crate::routing::Resource;
    pub use crate::routing::ResourceActions;
    pub use crate::routing::Route;
    pub use crate::routing::RouteAction;
    pub use crate::routing::RouteGroup;
    pub use crate::routing::RouteMethods;
    pub use crate::routing::RouteResolver;
    pub use crate::routing::Router;
    pub use crate::routing::UrlGenerator;
    #[cfg(feature = "queue")]
    pub use crate::schedule::{
        CronOptions, MissedRunPolicy, ScheduleDefinition, ScheduleFrequency, ScheduleReport,
        Scheduler,
    };
    #[cfg(feature = "signing")]
    pub use crate::signing::{
        DatabaseSignedLinkRevocationStore, SignedLink, SignedLinkClaims,
        SignedLinkRevocationMigration, SignedLinkRevocationStore, SignedLinks,
    };
    pub use crate::sms::{SmsContent, SmsDriver, SmsMessage, SmsReceipt, SmsSender};
    pub use crate::support::{
        ArrayKey, BackedEnum, Collection, EnumBacking, Fluent, InputNumber, MessageBag,
        MessageBagContract, MessageInput, MessageJsonOptions, MessageKeys, MessageLookup,
        MessageProvider, MessageValue, Stringable, ViewErrorBag,
    };
    pub use crate::tenant::{TenantContext, TenantId, TenantLayer};
    pub use crate::translation::DateFormatStyle;
    pub use crate::translation::DateTimeFormatStyle;
    pub use crate::translation::Locale;
    pub use crate::translation::MessageArgs;
    pub use crate::translation::MessageCatalog;
    pub use crate::translation::PluralCategory;
    pub use crate::translation::RequestLocale;
    pub use crate::translation::TranslationConfig;
    pub use crate::translation::TranslationFormatError;
    pub use crate::translation::Translator;
    pub use crate::validation::{Field, InputContext, Rule, RuleError, Rules, Validate, validate};
    pub use crate::{
        Application, ApplicationDefinition, BoxFuture, Cancellation, Command, Error,
        ExecutionContext, ExecutionMetadata, NoConfig, Result,
    };
    pub use crate::{DatabaseConfig, DatabaseInstanceConfig, Databases, Db};
}

/// 启动 `A`，并运行第一个命令行参数指定的命令（默认为 `serve`）。
///
/// 返回进程退出码而不触发 panic，使 `main` 保持为单行调用。
pub fn run<A: ApplicationDefinition>() -> ExitCode {
    run_with_args::<A>(std::env::args().skip(1))
}

/// 启动应用。未指定命令时，同进程运行 HTTP 与已配置的后台角色。
/// 显式指定 `serve`、队列或调度命令时，只运行对应入口。
/// 通过 `./artisan` 启动时，默认显示应用命令列表。
pub fn run_application<A: ApplicationDefinition>() -> ExitCode {
    let default = if std::env::var_os("OPENRUST_ARTISAN").is_some_and(|value| value == "1") {
        "list"
    } else {
        "start"
    };
    run_with_default::<A>(std::env::args().skip(1), default)
}

/// 使用显式控制台参数启动应用；`--env name` 和 `--env=name` 只选择存在的环境文件。
///
/// 嵌入式调用方传入自己的参数，普通 [`lifecycle::bootstrap`] 不观察进程参数。
pub fn run_with_args<A: ApplicationDefinition>(args: impl IntoIterator<Item = String>) -> ExitCode {
    run_with_default::<A>(args, "serve")
}

fn run_with_default<A: ApplicationDefinition>(
    args: impl IntoIterator<Item = String>,
    default_command: &str,
) -> ExitCode {
    let (name, command_args, console_environment) = parse_console_args(args, default_command);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("cannot start tokio runtime: {error}")),
    };
    runtime.block_on(async {
        let app = match lifecycle::bootstrap_for_console::<A>(&name, console_environment.as_deref())
            .await
        {
            Ok(app) => app,
            Err(error) => return fail(&format!("bootstrap failed: {error}")),
        };
        let result = app.run_command(&name, command_args).await;
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

fn parse_console_args(
    args: impl IntoIterator<Item = String>,
    default_command: &str,
) -> (String, Vec<String>, Option<String>) {
    let mut command = Vec::new();
    let mut environment = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--env" {
            if let Some(value) = args.next()
                && environment.is_none()
            {
                environment = Some(value);
            }
        } else if let Some(value) = arg.strip_prefix("--env=") {
            if environment.is_none() {
                environment = Some(value.to_owned());
            }
        } else {
            command.push(arg);
        }
    }
    let mut command = command.into_iter();
    let name = match command.next() {
        Some(name) if matches!(name.as_str(), "--help" | "-h") => "help".to_owned(),
        Some(name) => name,
        None => default_command.to_owned(),
    };
    (name, command.collect(), environment)
}

fn fail(message: &str) -> ExitCode {
    // 配置加载失败时 tracing 可能尚未安装，因此始终写入 stderr。
    eprintln!("error: {message}");
    ExitCode::FAILURE
}

#[cfg(test)]
#[path = "../tests/unit/application_entry.rs"]
mod tests;

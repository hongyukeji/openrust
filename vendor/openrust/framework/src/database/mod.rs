//! Database connections, query construction, schema management, and migrations.

mod lost_connection_detector;
pub use lost_connection_detector::LostConnectionDetector;
mod bindings;
pub(crate) mod configuration_url_parser;
mod connection;
mod connection_interface;
mod connection_pools;
mod connection_resolver_interface;
pub mod connectors;
pub(crate) mod console;
mod database_manager;
mod database_service_provider;
mod database_transactions_manager;
pub mod eloquent;
pub mod events;
mod facade;
mod migration_service_provider;
pub mod migrations;
pub mod query;
mod query_log;
pub mod schema;
pub mod seeder;
mod sql_server;
pub(crate) mod test_events;
pub(crate) mod test_mysql;
pub(crate) mod test_query_counts;
pub(crate) mod test_transaction;
mod transaction;
mod value;

pub(crate) use connection::{ConnectionScope, ReadRouting};
pub use transaction::Transaction;
pub use transaction::TransactionalService;

use std::{future::Future, pin::Pin};

pub use bindings::Bindings;
pub use connection::Connection;
pub use connection_interface::ConnectionInterface;
pub use connection_resolver_interface::ConnectionResolverInterface;
pub use database_manager::DatabaseManager;
pub use database_service_provider::DatabaseServiceProvider;
pub use database_transactions_manager::DatabaseTransactionsManager;
pub use migration_service_provider::MigrationServiceProvider;

/// An asynchronous database operation that may borrow its connection.
pub type DatabaseFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A fallible asynchronous database row stream.
pub type DatabaseCursor<'a, T = Row> =
    Pin<Box<dyn futures_util::Stream<Item = DatabaseResult<T>> + Send + 'a>>;

/// The result of a database operation.
pub type DatabaseResult<T> = Result<T, DatabaseError>;

pub use facade::Db;
pub use query_log::{QueryLogEntry, RawQueryLogEntry};
pub use value::{Row, Value};

/// A configuration, connection, or SQL execution failure.
#[non_exhaustive]
pub enum DatabaseError {
    UnknownConnection(String),
    UnsupportedDriver,
    UnsupportedOperation(&'static str),
    InvalidConfiguration(&'static str),
    UnknownSeeder(String),
    UnknownFakerFormatter(String),
    UnknownFakerExtension(String),
    FakerIo(std::io::Error),
    InvalidSeeder(&'static str),
    SeederFailed {
        seeder: String,
        source: Box<DatabaseError>,
    },
    InvalidMigration(&'static str),
    MigrationEvent(Box<crate::events::EventError>),
    MigrationFailed {
        migration: String,
        source: Box<DatabaseError>,
    },
    InvalidScalar,
    Pagination(crate::pagination::PaginationError),
    MassAssignment(String),
    MissingAttribute {
        model: &'static str,
        attribute: String,
    },
    ModelNotFound(&'static str),
    MultipleModelsFound {
        table: &'static str,
        count: usize,
    },
    ModelEvent(Box<crate::events::EventError>),
    ConnectionEvent(Box<crate::events::EventError>),
    LazyLoadingViolation {
        model: &'static str,
        relation: String,
    },
    InvalidQuery(&'static str),
    Embedding(Box<dyn std::error::Error + Send + Sync>),
    AttributeEncryption(crate::encryption::EncryptException),
    AttributeDecryption(crate::encryption::DecryptException),
    AttributeHashing(crate::hashing::HashError),
    InvalidSchema(&'static str),
    SchemaIo(std::io::Error),
    Translation(Box<crate::translation::TranslationError>),
    DatabaseCreationDeclined,
    SchemaProcess {
        program: &'static str,
        status: std::process::ExitStatus,
    },
    UnsupportedSchema(&'static str),
    TransactionClosed,
    AfterCommit {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    AfterRollback {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    InvalidTransactionAttempts,
    TransactionInterrupted,
    TransactionLevelChanged,
    InvalidBindings {
        expected: usize,
        actual: usize,
    },
    InvalidNamedBindings(&'static str),
    UnsupportedColumnType(String),
    Unavailable(crate::contracts::container::BindingResolutionError),
    Driver {
        operation: &'static str,
        source: sqlx::Error,
    },
    SqlServer {
        operation: &'static str,
        source: tiberius::error::Error,
    },
}

impl DatabaseError {
    fn driver(operation: &'static str) -> impl FnOnce(sqlx::Error) -> Self {
        move |source| Self::Driver { operation, source }
    }
}

impl std::fmt::Debug for DatabaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownConnection(name) => {
                write!(formatter, "database connection {name:?} is not configured")
            }
            Self::UnsupportedDriver => formatter.write_str("database driver is unavailable"),
            Self::UnsupportedOperation(operation) => {
                write!(
                    formatter,
                    "operation is not supported by this database driver: {operation}"
                )
            }
            Self::InvalidConfiguration(key) => write!(formatter, "invalid database option {key}"),
            Self::SchemaIo(_) => formatter.write_str("database schema file operation failed"),
            Self::DatabaseCreationDeclined => {
                formatter.write_str("Database was not created. Aborting migration.")
            }
            Self::SchemaProcess { program, status } => {
                write!(
                    formatter,
                    "database schema command {program} failed ({status})"
                )
            }
            Self::InvalidSchema(part) => write!(formatter, "invalid schema {part}"),
            Self::UnsupportedSchema(operation) => {
                write!(formatter, "unsupported schema operation: {operation}")
            }
            Self::UnknownFakerExtension(id) => {
                write!(formatter, "No Faker extension with id {id:?} was loaded.")
            }
            Self::FakerIo(error) => write!(formatter, "Faker file operation failed: {error}"),
            Self::UnknownFakerFormatter(name) => write!(formatter, "Unknown format {name:?}"),
            Self::InvalidQuery(part) => write!(formatter, "invalid query {part}"),
            Self::Embedding(_) => formatter.write_str("could not resolve a text embedding"),
            Self::AttributeEncryption(_) => {
                formatter.write_str("could not encrypt model attribute")
            }
            Self::AttributeDecryption(_) => {
                formatter.write_str("could not decrypt model attribute")
            }
            Self::AttributeHashing(_) => formatter.write_str("could not hash model attribute"),
            Self::UnknownSeeder(name) => write!(formatter, "seeder {name:?} is not registered"),
            Self::InvalidSeeder(part) => write!(formatter, "invalid seeder {part}"),
            Self::SeederFailed { seeder, source } => write!(formatter, "{seeder}: {source}"),
            Self::InvalidMigration(part) => write!(formatter, "invalid migration {part}"),
            Self::MigrationEvent(_) => formatter.write_str("migration event listener failed"),
            Self::MigrationFailed { migration, source } => {
                write!(formatter, "{migration}: {source}")
            }
            Self::MassAssignment(name) => write!(formatter, "字段 {name:?} 不允许批量赋值"),
            Self::MissingAttribute { model, attribute } => {
                write!(formatter, "模型 {model} 未加载属性 {attribute:?}")
            }
            Self::ModelNotFound(table) => {
                write!(formatter, "model for table {table:?} was not found")
            }
            Self::MultipleModelsFound { table, count } => {
                write!(
                    formatter,
                    "expected one model for table {table:?}, found {count}"
                )
            }
            Self::ConnectionEvent(_) => formatter.write_str("database event listener failed"),
            Self::ModelEvent(_) => formatter.write_str("model event dispatch failed"),
            Self::LazyLoadingViolation { model, relation } => {
                write!(
                    formatter,
                    "lazy loading {relation:?} on model {model} is prohibited"
                )
            }
            Self::Pagination(error) => std::fmt::Display::fmt(error, formatter),
            Self::Translation(error) => std::fmt::Display::fmt(error, formatter),
            Self::InvalidScalar => {
                formatter.write_str("a scalar query must return exactly one column")
            }
            Self::InvalidTransactionAttempts => {
                formatter.write_str("database transaction requires at least one attempt")
            }
            Self::AfterCommit { .. } => {
                formatter.write_str("database committed but a commit callback failed")
            }
            Self::AfterRollback { .. } => {
                formatter.write_str("database rolled back but a rollback callback failed")
            }
            Self::TransactionClosed => formatter.write_str("database transaction has ended"),
            Self::TransactionInterrupted => {
                formatter.write_str("database transaction was interrupted")
            }
            Self::TransactionLevelChanged => {
                formatter.write_str("database callback changed its transaction level")
            }
            Self::InvalidBindings { expected, actual } => write!(
                formatter,
                "expected {expected} database bindings, received {actual}"
            ),
            Self::InvalidNamedBindings(reason) => {
                write!(formatter, "invalid named database bindings: {reason}")
            }
            Self::UnsupportedColumnType(kind) => {
                write!(formatter, "unsupported database column type {kind}")
            }
            Self::Unavailable(_) => {
                formatter.write_str("application database manager is unavailable")
            }
            Self::Driver { operation, .. } | Self::SqlServer { operation, .. } => {
                write!(formatter, "database {operation} failed")
            }
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SchemaIo(source) => Some(source),
            Self::Translation(source) => Some(source.as_ref()),
            Self::MigrationFailed { source, .. } | Self::SeederFailed { source, .. } => {
                Some(source.as_ref())
            }
            Self::Driver { source, .. } => Some(source),
            Self::SqlServer { source, .. } => Some(source),
            Self::FakerIo(source) => Some(source),
            Self::AfterCommit { source } | Self::AfterRollback { source } => Some(source.as_ref()),
            Self::ModelEvent(source)
            | Self::ConnectionEvent(source)
            | Self::MigrationEvent(source) => Some(source.as_ref()),
            Self::Unavailable(source) => Some(source),
            Self::Embedding(source) => Some(source.as_ref()),
            Self::AttributeEncryption(source) => Some(source),
            Self::AttributeDecryption(source) => Some(source),
            Self::AttributeHashing(source) => Some(source),
            Self::Pagination(source) => Some(source),
            _ => None,
        }
    }
}

impl From<crate::pagination::PaginationError> for DatabaseError {
    fn from(error: crate::pagination::PaginationError) -> Self {
        Self::Pagination(error)
    }
}

impl From<crate::translation::TranslationError> for DatabaseError {
    fn from(error: crate::translation::TranslationError) -> Self {
        Self::Translation(Box::new(error))
    }
}

impl From<crate::contracts::container::BindingResolutionError> for DatabaseError {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::Unavailable(source)
    }
}

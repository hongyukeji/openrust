//! Database connections, query construction, schema management, and migrations.

mod configuration_url_parser;
mod connection;
mod connection_interface;
mod connection_resolver_interface;
pub mod connectors;
mod database_manager;
mod database_service_provider;
mod database_transactions_manager;
pub mod eloquent;
mod facade;
mod migration_service_provider;
pub mod migrations;
pub mod query;
pub mod schema;
mod transaction;
mod value;

pub(crate) use connection::{ConnectionScope, ReadRouting};
pub use transaction::Transaction;

use std::{future::Future, pin::Pin};

pub use connection::Connection;
pub use connection_interface::ConnectionInterface;
pub use connection_resolver_interface::ConnectionResolverInterface;
pub use database_manager::DatabaseManager;
pub use database_service_provider::DatabaseServiceProvider;
pub use database_transactions_manager::DatabaseTransactionsManager;
pub use migration_service_provider::MigrationServiceProvider;

/// An asynchronous database operation that may borrow its connection.
pub type DatabaseFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a database operation.
pub type DatabaseResult<T> = Result<T, DatabaseError>;

pub use facade::DB;
pub use value::{Row, Value};

/// A configuration, connection, or SQL execution failure.
#[non_exhaustive]
pub enum DatabaseError {
    UnknownConnection(String),
    UnsupportedDriver,
    InvalidConfiguration(&'static str),
    InvalidScalar,
    InvalidQuery(&'static str),
    TransactionClosed,
    InvalidTransactionAttempts,
    TransactionInterrupted,
    TransactionLevelChanged,
    InvalidBindings {
        expected: usize,
        actual: usize,
    },
    UnsupportedColumnType(String),
    Unavailable(crate::contracts::container::BindingResolutionError),
    Driver {
        operation: &'static str,
        source: sqlx::Error,
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
            Self::InvalidConfiguration(key) => write!(formatter, "invalid database option {key}"),
            Self::InvalidQuery(part) => write!(formatter, "invalid query {part}"),
            Self::InvalidScalar => {
                formatter.write_str("a scalar query must return exactly one column")
            }
            Self::InvalidTransactionAttempts => {
                formatter.write_str("database transaction requires at least one attempt")
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
            Self::UnsupportedColumnType(kind) => {
                write!(formatter, "unsupported database column type {kind}")
            }
            Self::Unavailable(_) => {
                formatter.write_str("application database manager is unavailable")
            }
            Self::Driver { operation, .. } => write!(formatter, "database {operation} failed"),
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Driver { source, .. } => Some(source),
            Self::Unavailable(source) => Some(source),
            _ => None,
        }
    }
}

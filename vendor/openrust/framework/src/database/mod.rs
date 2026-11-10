//! Database connections, query construction, schema management, and migrations.

mod connection;
mod connection_interface;
mod connection_resolver_interface;
pub mod connectors;
mod database_manager;
mod database_service_provider;
mod database_transactions_manager;
pub mod eloquent;
mod migration_service_provider;
pub mod migrations;
pub mod query;
pub mod schema;

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

/// A configuration, compilation, connection, or execution failure.
#[derive(Debug)]
pub struct DatabaseError {
    _private: (),
}

/// A database value that preserves SQL null and the native data type.
pub struct Value {
    _private: (),
}

/// A result row returned by a connection or query.
pub struct Row {
    _private: (),
}

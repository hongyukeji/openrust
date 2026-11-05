//! Session共享存储与请求生命周期；能力通过Session Provider显式安装。

mod attributes;
mod backend;
mod cookie;
mod database_handler;
mod database_store;
mod error_bag;
mod guarded_migration;
mod id;
mod middleware;
mod migration;
mod name;
mod provider;
pub(crate) use middleware::{SessionLayer, handle as handle_session};
pub(crate) use provider::SessionRuntime;
mod request;
mod state;
mod store;

#[cfg(feature = "session-redis")]
pub use backend::RedisSessionStore;
pub use backend::{GuardedDatabaseSessionStore, MemorySessionStore};
pub use cookie::CookieSessionStore;
pub use database_handler::{DatabaseSessionHandler, DatabaseSessionMetadata};
pub use database_store::DatabaseSessionStore;
pub use error_bag::ErrorBagCodecs;
pub use guarded_migration::{
    GuardedSessionGcMigration, GuardedSessionMigration, GuardedSessionTableGcMigration,
    GuardedSessionTableMigration,
};
pub use id::SessionId;
pub use migration::{SessionGcMigration, SessionMigration, SessionTableMigration};
#[cfg(feature = "auth-fortify")]
pub use store::CredentialSessionReader;
pub use store::{
    DEFAULT_MAX_BYTES, SessionRecord, SessionStore, SessionStoreGuarantees, SessionStoreSemantics,
    SessionWriteMode, SessionWriteOutcome,
};

#[cfg(feature = "auth")]
pub(crate) use provider::SessionSettings;
pub use provider::{SessionServiceProvider, SessionStoreHandle};
pub use request::SessionRequestContext;
pub use state::{Expiry, Session};

fn merge_attributes(
    current: serde_json::Value,
    incoming: serde_json::Value,
) -> crate::Result<serde_json::Value> {
    crate::support::data::replace_root(current, incoming)
}

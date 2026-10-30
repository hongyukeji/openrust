//! Session共享存储与请求生命周期；能力通过Session Provider显式安装。

mod attributes;
mod backend;
mod cookie;
mod error_bag;
mod id;
mod middleware;
mod migration;
mod name;
mod provider;
mod request;
mod state;
mod store;

#[cfg(feature = "session-redis")]
pub use backend::RedisSessionStore;
pub use backend::{DatabaseSessionStore, MemorySessionStore};
pub use cookie::CookieSessionStore;
pub use error_bag::ErrorBagCodecs;
pub use id::SessionId;
pub use migration::{SessionGcMigration, SessionMigration};
pub use store::{
    DEFAULT_MAX_BYTES, SessionRecord, SessionStore, SessionStoreGuarantees, SessionWriteMode,
    SessionWriteOutcome,
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

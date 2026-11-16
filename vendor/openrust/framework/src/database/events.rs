//! Events emitted by database connections and transactions.

use crate::contracts::events::Event;

use super::Connection;

/// An application connection has been resolved for use.
pub struct ConnectionEstablished {
    pub connection: Connection,
}

impl ConnectionEstablished {
    pub const NAME: &'static str = r"OpenRust\Database\Events\ConnectionEstablished";
}

impl Event for ConnectionEstablished {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(super) trait TransactionEvent: Event {
    const NAME: &'static str;

    fn new(connection: Connection, level: u32) -> Self;
}

macro_rules! transaction_event {
    ($event:ident, $description:literal) => {
        #[doc = $description]
        pub struct $event {
            pub connection: Connection,
            pub level: u32,
        }

        impl $event {
            pub const NAME: &'static str =
                concat!(r"OpenRust\Database\Events\", stringify!($event));
        }

        impl Event for $event {
            fn name(&self) -> &str {
                Self::NAME
            }
        }

        impl TransactionEvent for $event {
            const NAME: &'static str = Self::NAME;

            fn new(connection: Connection, level: u32) -> Self {
                Self { connection, level }
            }
        }
    };
}

transaction_event!(
    TransactionBeginning,
    "A transaction or savepoint has started."
);
transaction_event!(
    TransactionCommitting,
    "The outer transaction is about to commit."
);
transaction_event!(TransactionCommitted, "The transaction level has committed.");
transaction_event!(
    TransactionRolledBack,
    "A rollback has restored the indicated transaction level."
);

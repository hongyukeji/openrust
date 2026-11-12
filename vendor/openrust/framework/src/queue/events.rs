//! Queue and worker pause transitions.

use crate::{cache::Ttl, contracts::events::Event};

macro_rules! queue_event {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
        pub struct $name { $($(#[$field_meta])* pub $field: $ty),* }

        impl $name {
            pub const NAME: &'static str = stringify!($name);
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}

queue_event!(
    /// An individual queue pause was written.
    QueuePaused { connection: String, queue: String, ttl: Option<Ttl> }
);
queue_event!(
    /// An individual queue pause was removed.
    QueueResumed { connection: String, queue: String }
);
queue_event!(
    /// The global pause was written.
    QueuesPaused {}
);
queue_event!(
    /// The global pause was removed.
    QueuesResumed {}
);
queue_event!(
    /// A worker observed a queue entering a pause.
    WorkerQueuePaused { connection: String, queue: String }
);
queue_event!(
    /// A worker observed a queue leaving a pause.
    WorkerQueueResumed { connection: String, queue: String }
);

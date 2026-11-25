use openrust::{contracts::events::Event, support::facades};

macro_rules! memory_event {
    ($name:ident) => {
        /// Memory usage in MiB immediately before a memory-limit exit.
        pub struct $name {
            pub name: String,
            pub memory_usage: f64,
            pub memory_limit: u64,
        }

        impl $name {
            pub const NAME: &str = stringify!($name);
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}

memory_event!(MasterSupervisorOutOfMemory);
memory_event!(SupervisorOutOfMemory);

impl SupervisorOutOfMemory {
    pub fn get_memory_usage(&self) -> f64 {
        self.memory_usage
    }

    pub fn set_memory_usage(&mut self, memory_usage: f64) -> &mut Self {
        self.memory_usage = memory_usage;
        self
    }
}

pub(crate) async fn dispatch(event: impl Event + 'static) {
    if let Err(error) = facades::Event::dispatch(event).await {
        let _ = facades::Log::warning(
            &format!("Horizon memory-limit listener failed: {error}"),
            &Default::default(),
        );
    }
}

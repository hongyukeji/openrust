use crate::{
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event},
    },
    events::{Dispatcher, EventContext, EventResult},
};
use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// A formatted Rust value observed by application diagnostics.
pub struct Dumped {
    pub dump: String,
    captured: AtomicBool,
}
impl Dumped {
    /// Claims the dump so the helper does not also print it to standard output.
    pub fn capture(&self) {
        self.captured.store(true, Ordering::Release);
    }
}
impl Event for Dumped {
    fn name(&self) -> &str {
        "OpenRust\\Support\\Events\\Dumped"
    }
}

/// Dumps a Rust value, returning it after application listeners have observed it.
pub async fn dump<T: Debug + Sync + ?Sized>(value: &T) -> EventResult<&T> {
    let event = Arc::new(Dumped {
        dump: format!("{value:#?}"),
        captured: AtomicBool::new(false),
    });
    if let Some(container) = super::facades::current_container() {
        let dispatcher = container
            .make::<Dispatcher>()
            .map_err(crate::events::EventError::Unavailable)?;
        dispatcher
            .dispatch(event.clone(), &mut EventContext::new())
            .await?;
    }
    if !event.captured.load(Ordering::Acquire) {
        println!("{}", event.dump);
    }
    Ok(value)
}

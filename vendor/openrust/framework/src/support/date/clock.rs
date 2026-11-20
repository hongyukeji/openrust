use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;

use crate::{
    container::{Container, ServiceKey},
    contracts::container::Container as _,
    support::facades::current_container,
};

type FrozenTime = Mutex<Option<Timestamp>>;

#[derive(Default)]
pub(crate) struct TestClock {
    fixed: FrozenTime,
}

#[derive(Clone)]
struct ClockScope {
    clock: Arc<TestClock>,
    fixed: Arc<FrozenTime>,
}

tokio::task_local! {
    static CLOCK_SCOPES: Vec<ClockScope>;
}

impl TestClock {
    fn scoped(&self) -> Option<Arc<FrozenTime>> {
        CLOCK_SCOPES
            .try_with(|scopes| {
                scopes.iter().rev().find_map(|scope| {
                    std::ptr::eq(self, scope.clock.as_ref()).then(|| scope.fixed.clone())
                })
            })
            .unwrap_or_default()
    }

    fn now(&self) -> Option<Timestamp> {
        let scope = self.scoped();
        *scope
            .as_deref()
            .unwrap_or(&self.fixed)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set(&self, time: Option<Timestamp>) {
        let scope = self.scoped();
        *scope
            .as_deref()
            .unwrap_or(&self.fixed)
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = time;
    }

    pub(crate) async fn scope<F: Future>(self: Arc<Self>, time: Timestamp, future: F) -> F::Output {
        let mut scopes = CLOCK_SCOPES.try_with(Vec::clone).unwrap_or_default();
        scopes.push(ClockScope {
            clock: self,
            fixed: Arc::new(Mutex::new(Some(time))),
        });
        CLOCK_SCOPES.scope(scopes, future).await
    }
}

pub(super) fn utc_now() -> Timestamp {
    utc_now_for(current_container().as_deref())
}

pub(crate) fn utc_now_for(container: Option<&Container>) -> Timestamp {
    frozen_now_for(container).unwrap_or_else(Timestamp::now)
}

pub(crate) fn frozen_utc_now() -> Option<Timestamp> {
    frozen_now_for(current_container().as_deref())
}

fn frozen_now_for(container: Option<&Container>) -> Option<Timestamp> {
    container
        .and_then(|container| {
            container
                .get::<TestClock>(ServiceKey::typed::<TestClock>())
                .ok()
        })
        .and_then(|clock| clock.now())
}

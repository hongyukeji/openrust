use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;

use crate::{
    container::{Container, ServiceKey},
    contracts::container::Container as _,
    support::facades::current_container,
};

type FrozenTime = Mutex<Option<super::Date>>;

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

    pub(crate) fn date(&self) -> Option<super::Date> {
        let scope = self.scoped();
        scope
            .as_deref()
            .unwrap_or(&self.fixed)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub(crate) fn now(&self) -> Option<Timestamp> {
        self.date().map(|date| date.as_date_time().timestamp())
    }
    pub(crate) fn set(&self, time: Option<Timestamp>) {
        self.set_date(time.map(|time| super::Date::from_utc(time, super::DateTimezone::UTC)));
    }
    pub(crate) fn set_date(&self, time: Option<super::Date>) {
        let scope = self.scoped();
        *scope
            .as_deref()
            .unwrap_or(&self.fixed)
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = time;
    }
    pub(crate) fn update_date<E>(
        &self,
        update: impl FnOnce(&super::Date) -> Result<super::Date, E>,
    ) -> Result<bool, E> {
        let scope = self.scoped();
        let mut frozen = scope
            .as_deref()
            .unwrap_or(&self.fixed)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(date) = frozen.as_ref() else {
            return Ok(false);
        };
        *frozen = Some(update(date)?);
        Ok(true)
    }
    pub(crate) async fn scope_date<F: Future>(
        self: Arc<Self>,
        time: Option<super::Date>,
        future: F,
    ) -> F::Output {
        let mut scopes = CLOCK_SCOPES.try_with(Vec::clone).unwrap_or_default();
        scopes.push(ClockScope {
            clock: self,
            fixed: Arc::new(Mutex::new(time)),
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
    if container.is_none() {
        return crate::support::date_factory::standalone_clock().now();
    }
    container
        .and_then(|container| {
            container
                .get::<TestClock>(ServiceKey::typed::<TestClock>())
                .ok()
        })
        .and_then(|clock| clock.now())
}

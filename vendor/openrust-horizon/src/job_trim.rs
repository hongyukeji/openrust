use crate::MasterSupervisorLooped;
use openrust::{
    contracts::events::{Event, Listener},
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::{Config, Facade},
};
use std::time::Duration;
use tokio::{sync::Mutex, time::Instant};

#[derive(Default)]
pub(crate) struct JobTrimSchedule {
    recent: Mutex<TrimState>,
    failed: Mutex<TrimState>,
    monitored: Mutex<TrimState>,
}

#[derive(Default)]
struct TrimState {
    interval: Option<Duration>,
    last_trimmed: Option<Instant>,
}

fn interval_for(key: &str) -> EventResult<Duration> {
    let minutes = Config::get_facade_root()?
        .get(key)
        .and_then(|value| value.as_u64())
        .unwrap_or(10080);
    Ok(Duration::from_secs(
        (minutes / 12).max(1).saturating_mul(60),
    ))
}

macro_rules! trim_listener {
    ($name:ident, $state:ident, $trim:ident, $interval:expr) => {
        pub struct $name;

        impl Listener for $name {
            fn handle<'a>(
                &'a self,
                event: &'a dyn Event,
                _: &'a mut EventContext<'_>,
            ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
                Box::pin(async move {
                    let Some(event) = event.downcast_ref::<MasterSupervisorLooped>() else {
                        return Ok(None);
                    };
                    let mut state = event.job_trim_schedule.$state.lock().await;
                    let interval = match state.interval {
                        Some(interval) => interval,
                        None => *state.interval.insert($interval),
                    };
                    if state
                        .last_trimmed
                        .is_some_and(|last| last.elapsed() < interval)
                    {
                        return Ok(None);
                    }
                    event.repository.jobs().$trim().await.map_err(|source| {
                        EventError::Listener {
                            source: Box::new(source),
                        }
                    })?;
                    state.last_trimmed = Some(Instant::now());
                    Ok(None)
                })
            }
        }
    };
}

trim_listener!(
    TrimRecentJobs,
    recent,
    trim_recent_jobs,
    Duration::from_secs(60)
);
trim_listener!(
    TrimFailedJobs,
    failed,
    trim_failed_jobs,
    interval_for("horizon.trim.failed")?
);
trim_listener!(
    TrimMonitoredJobs,
    monitored,
    trim_monitored_jobs,
    interval_for("horizon.trim.monitored")?
);

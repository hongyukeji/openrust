use std::time::{Duration, SystemTime};

use super::{Delay, DispatchOptions, Payload, QueueError, QueueResult};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct ChainRoute {
    pub(super) connection: Option<String>,
    pub(super) queue: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct JobDispatch {
    connection: Option<String>,
    queue: Option<String>,
    pub(super) delay: Option<Delay>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct ChainDelivery {
    pub(super) connection: String,
    pub(super) queue: Option<String>,
    pub(super) delay: Option<Delay>,
    pub(super) job: Payload,
}

#[allow(non_snake_case, reason = "Matches Laravel Queueable")]
impl Payload {
    pub fn onConnection(mut self, name: impl Into<String>) -> Self {
        self.dispatch_options.get_or_insert_default().connection = Some(name.into());
        self
    }

    pub fn onQueue(mut self, name: impl Into<String>) -> Self {
        self.dispatch_options.get_or_insert_default().queue = Some(name.into());
        self
    }

    pub fn delay(mut self, delay: Delay) -> Self {
        self.dispatch_options.get_or_insert_default().delay = Some(delay);
        self
    }

    pub(crate) fn with_chain_route(mut self, defaults: &DispatchOptions) -> Self {
        self.chain_route = Some(Box::new(ChainRoute {
            connection: defaults.connection.clone(),
            queue: defaults.queue.clone(),
        }));
        self
    }

    pub(crate) fn first_chain_options(&self, defaults: DispatchOptions) -> DispatchOptions {
        let job = self.dispatch_options.as_deref();
        DispatchOptions {
            connection: job
                .and_then(|job| job.connection.clone())
                .or(defaults.connection),
            queue: job.and_then(|job| job.queue.clone()).or(defaults.queue),
            delay: job.and_then(|job| job.delay).or(defaults.delay),
            after_commit: defaults.after_commit,
        }
    }

    pub(super) fn chain_options(&self, source: &str, queue: &str) -> DispatchOptions {
        let inherited = self.chain_route.as_deref().map_or_else(
            || ChainRoute {
                connection: Some(source.to_owned()),
                queue: Some(queue.to_owned()),
            },
            Clone::clone,
        );
        self.first_chain_options(DispatchOptions {
            connection: inherited.connection,
            queue: inherited.queue,
            ..Default::default()
        })
    }

    pub(super) fn chain_delivery(
        self,
        options: DispatchOptions,
        connection: String,
    ) -> QueueResult<Self> {
        let mut relay = Self::new("bus.chain-delivery", ())?;
        let delay = match options.delay {
            Some(Delay::For(duration)) if duration.is_zero() => None,
            Some(Delay::For(duration)) => Some(Delay::Until(
                SystemTime::now()
                    .checked_add(duration)
                    .ok_or(QueueError::InvalidPayload)?,
            )),
            delay => delay,
        };
        relay.chain_delivery = Some(Box::new(ChainDelivery {
            connection,
            queue: options.queue,
            delay,
            job: self,
        }));
        Ok(relay)
    }
}

pub(super) fn no_delay(delay: Option<Delay>) -> bool {
    matches!(delay, None | Some(Delay::For(Duration::ZERO)))
}

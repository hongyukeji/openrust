use ::redis::{Msg, aio::PubSubStream};
use futures_util::StreamExt as _;

/// A Redis Pub/Sub subscription with a dedicated connection.
pub struct Subscription(PubSubStream);

impl Subscription {
    pub(super) fn new(stream: PubSubStream) -> Self {
        Self(stream)
    }

    pub async fn next(&mut self) -> Option<Msg> {
        self.0.next().await
    }
}

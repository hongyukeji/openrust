use ::redis::{Msg, PushInfo, aio::PubSubStream, cluster_async::ClusterConnection};
use futures_util::StreamExt as _;
use tokio::sync::mpsc::UnboundedReceiver;

/// A Redis Pub/Sub subscription with a dedicated connection.
pub struct Subscription(Receiver);

enum Receiver {
    Standalone(PubSubStream),
    Cluster {
        _connection: ClusterConnection,
        messages: UnboundedReceiver<PushInfo>,
    },
}

impl Subscription {
    pub(super) fn new(stream: PubSubStream) -> Self {
        Self(Receiver::Standalone(stream))
    }

    pub(super) fn cluster(
        connection: ClusterConnection,
        messages: UnboundedReceiver<PushInfo>,
    ) -> Self {
        Self(Receiver::Cluster {
            _connection: connection,
            messages,
        })
    }

    pub async fn next(&mut self) -> Option<Msg> {
        match &mut self.0 {
            Receiver::Standalone(stream) => stream.next().await,
            Receiver::Cluster { messages, .. } => loop {
                if let Some(message) = Msg::from_push_info(messages.recv().await?) {
                    return Some(message);
                }
            },
        }
    }
}

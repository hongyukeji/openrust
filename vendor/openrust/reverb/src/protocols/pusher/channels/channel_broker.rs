use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Member {
    pub user_id: String,
    #[serde(default)]
    pub user_info: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Frame {
    pub event: String,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl Frame {
    pub fn new(event: &str, data: String, channel: Option<&str>) -> Self {
        Self {
            event: event.into(),
            data,
            channel: channel.map(str::to_owned),
            user_id: None,
        }
    }

    fn on_channel(event: &str, data: Value, channel: &str) -> Self {
        Self::new(event, data.to_string(), Some(channel))
    }
}

struct Connection {
    send: mpsc::Sender<Frame>,
    disconnect: CancellationToken,
}

struct Subscription {
    member: Option<Member>,
    ready: bool,
    distributed: bool,
}

type Subscribers = HashMap<String, Subscription>;

#[derive(Default)]
struct State {
    connections: HashMap<String, Connection>,
    channels: HashMap<String, Subscribers>,
}

/// Maintains one Reverb application's sockets and channel memberships.
#[derive(Default)]
pub struct ChannelBroker {
    state: Mutex<State>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionError {
    UnknownSocket,
    InvalidChannel,
    InvalidMember,
}

impl ChannelBroker {
    pub fn connect(
        &self,
        socket: String,
        send: mpsc::Sender<Frame>,
        disconnect: CancellationToken,
        limit: Option<usize>,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        if limit.is_some_and(|limit| state.connections.len() >= limit)
            || state.connections.contains_key(&socket)
        {
            return false;
        }
        state
            .connections
            .insert(socket, Connection { send, disconnect });
        true
    }

    pub fn subscribe(
        &self,
        socket: &str,
        channel: &str,
        member: Option<Member>,
    ) -> Result<(), SubscriptionError> {
        if !valid_channel(channel) {
            return Err(SubscriptionError::InvalidChannel);
        }
        let presence = channel.starts_with("presence-");
        if presence != member.is_some()
            || member
                .as_ref()
                .is_some_and(|member| member.user_id.is_empty())
        {
            return Err(SubscriptionError::InvalidMember);
        }

        let mut state = self.state.lock().unwrap();
        if !state.connections.contains_key(socket) {
            return Err(SubscriptionError::UnknownSocket);
        }

        let subscribers = state.channels.entry(channel.into()).or_default();
        let joined = member
            .as_ref()
            .filter(|member| {
                !subscribers.contains_key(socket) && !has_member(subscribers, &member.user_id)
            })
            .cloned();
        subscribers.entry(socket.into()).or_insert(Subscription {
            member,
            ready: true,
            distributed: false,
        });

        let data = if presence {
            let hash: BTreeMap<_, _> = subscribers
                .values()
                .filter_map(|subscription| subscription.member.as_ref())
                .map(|member| (&member.user_id, &member.user_info))
                .collect();
            let ids: Vec<_> = hash.keys().copied().collect();
            json!({"presence": {"count": ids.len(), "ids": ids, "hash": hash}})
        } else {
            json!({})
        };
        let subscribed = Frame::on_channel("pusher_internal:subscription_succeeded", data, channel);
        state.send(socket, &subscribed);
        if let Some(member) = joined {
            let added = Frame::on_channel(
                "pusher_internal:member_added",
                json!({"user_id": member.user_id, "user_info": member.user_info}),
                channel,
            );
            state.publish(channel, &added, Some(socket));
        }
        Ok(())
    }

    pub fn unsubscribe(&self, socket: &str, channel: &str) {
        self.state.lock().unwrap().unsubscribe(socket, channel);
    }

    pub fn disconnect(&self, socket: &str) {
        self.remove_connection(socket);
    }

    pub(crate) fn remove_connection(&self, socket: &str) -> Vec<String> {
        let mut state = self.state.lock().unwrap();
        state.connections.remove(socket);
        let channels: Vec<_> = state
            .channels
            .iter()
            .filter(|(_, subscribers)| subscribers.contains_key(socket))
            .map(|(name, _)| name.clone())
            .collect();
        channels
            .into_iter()
            .filter(|channel| state.unsubscribe(socket, channel))
            .collect()
    }

    pub(crate) fn remove_subscription(&self, socket: &str, channel: &str) -> bool {
        self.state.lock().unwrap().unsubscribe(socket, channel)
    }

    pub(crate) fn begin_presence(
        &self,
        socket: &str,
        channel: &str,
        member: Member,
    ) -> Result<Member, SubscriptionError> {
        if !valid_channel(channel) || !channel.starts_with("presence-") {
            return Err(SubscriptionError::InvalidChannel);
        }
        if member.user_id.is_empty() {
            return Err(SubscriptionError::InvalidMember);
        }
        let mut state = self.state.lock().unwrap();
        if !state.connections.contains_key(socket) {
            return Err(SubscriptionError::UnknownSocket);
        }
        let subscription = state
            .channels
            .entry(channel.into())
            .or_default()
            .entry(socket.into())
            .or_insert(Subscription {
                member: Some(member),
                ready: false,
                distributed: true,
            });
        Ok(subscription
            .member
            .clone()
            .expect("distributed presence member"))
    }

    pub(crate) fn presence_subscribed(&self, socket: &str, channel: &str, frame: &Frame) {
        let mut state = self.state.lock().unwrap();
        if let Some(subscription) = state
            .channels
            .get_mut(channel)
            .and_then(|sockets| sockets.get_mut(socket))
            && subscription.distributed
        {
            subscription.ready = true;
            state.send(socket, frame);
        }
    }

    pub(crate) fn presence_connections(&self) -> Vec<(String, Vec<String>)> {
        self.state
            .lock()
            .unwrap()
            .channels
            .iter()
            .filter_map(|(channel, sockets)| {
                let sockets: Vec<_> = sockets
                    .iter()
                    .filter(|(_, subscription)| subscription.distributed && subscription.ready)
                    .map(|(socket, _)| socket.clone())
                    .collect();
                (!sockets.is_empty()).then(|| (channel.clone(), sockets))
            })
            .collect()
    }

    pub(crate) fn close_presence(&self) {
        let state = self.state.lock().unwrap();
        for sockets in state.channels.values() {
            for (socket, subscription) in sockets {
                if subscription.distributed
                    && let Some(connection) = state.connections.get(socket)
                {
                    connection.disconnect.cancel();
                }
            }
        }
    }

    pub(crate) fn close_stale_presence(&self, channel: &str, socket: &str) {
        let state = self.state.lock().unwrap();
        if state
            .channels
            .get(channel)
            .and_then(|sockets| sockets.get(socket))
            .is_some_and(|subscription| subscription.distributed && subscription.ready)
            && let Some(connection) = state.connections.get(socket)
        {
            connection.disconnect.cancel();
        }
    }

    pub(crate) fn client_frame(
        &self,
        socket: &str,
        channel: &str,
        event: &str,
        data: String,
        members_only: bool,
    ) -> Option<Frame> {
        if !valid_channel(channel) {
            return None;
        }
        let state = self.state.lock().unwrap();
        let subscriber = state
            .channels
            .get(channel)
            .and_then(|sockets| sockets.get(socket))
            .filter(|subscription| subscription.ready);
        if members_only && subscriber.is_none() {
            return None;
        }
        let mut frame = Frame::new(event, data, Some(channel));
        frame.user_id = subscriber
            .and_then(|subscription| subscription.member.as_ref())
            .map(|member| member.user_id.clone());
        Some(frame)
    }

    pub fn publish(&self, channel: &str, event: &str, data: String, excluded: Option<&str>) {
        self.broadcast(channel, &Frame::new(event, data, Some(channel)), excluded);
    }

    pub(crate) fn broadcast(&self, channel: &str, frame: &Frame, excluded: Option<&str>) {
        self.state.lock().unwrap().publish(channel, frame, excluded);
    }
}

impl State {
    fn unsubscribe(&mut self, socket: &str, channel: &str) -> bool {
        let Some(subscribers) = self.channels.get_mut(channel) else {
            return false;
        };
        let subscription = subscribers.remove(socket);
        let distributed = subscription
            .as_ref()
            .is_some_and(|subscription| subscription.distributed);
        let member = subscription
            .filter(|subscription| !subscription.distributed)
            .and_then(|subscription| subscription.member);
        let departed = member.filter(|member| !has_member(subscribers, &member.user_id));
        if subscribers.is_empty() {
            self.channels.remove(channel);
        }
        if let Some(member) = departed {
            let removed = Frame::on_channel(
                "pusher_internal:member_removed",
                json!({"user_id": member.user_id}),
                channel,
            );
            self.publish(channel, &removed, None);
        }
        distributed
    }

    fn send(&self, socket: &str, frame: &Frame) {
        if let Some(connection) = self.connections.get(socket)
            && connection.send.try_send(frame.clone()).is_err()
        {
            connection.disconnect.cancel();
        }
    }

    fn publish(&self, channel: &str, frame: &Frame, excluded: Option<&str>) {
        if let Some(subscribers) = self.channels.get(channel) {
            for (socket, subscription) in subscribers {
                if subscription.ready && Some(socket.as_str()) != excluded {
                    self.send(socket, frame);
                }
            }
        }
    }
}

fn has_member(subscribers: &Subscribers, user_id: &str) -> bool {
    subscribers
        .values()
        .filter_map(|subscription| subscription.member.as_ref())
        .any(|member| member.user_id == user_id)
}

pub(crate) fn valid_channel(channel: &str) -> bool {
    !channel.is_empty()
        && channel.len() <= 200
        && !channel.starts_with("private-encrypted-")
        && channel.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'-' | b'=' | b'@' | b',' | b'.' | b';')
        })
}

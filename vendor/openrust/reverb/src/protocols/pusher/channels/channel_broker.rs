use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    pub user_id: String,
    #[serde(default)]
    pub user_info: Value,
}

#[derive(Clone, Debug, Serialize)]
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

type Subscribers = HashMap<String, Option<Member>>;

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
        subscribers.entry(socket.into()).or_insert(member);

        let data = if presence {
            let hash: BTreeMap<_, _> = subscribers
                .values()
                .flatten()
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
        let mut state = self.state.lock().unwrap();
        state.connections.remove(socket);
        let channels: Vec<_> = state
            .channels
            .iter()
            .filter(|(_, subscribers)| subscribers.contains_key(socket))
            .map(|(name, _)| name.clone())
            .collect();
        for channel in channels {
            state.unsubscribe(socket, &channel);
        }
    }

    pub(crate) fn publish_client(
        &self,
        socket: &str,
        channel: &str,
        event: &str,
        data: String,
        members_only: bool,
    ) -> bool {
        if !valid_channel(channel) {
            return false;
        }
        let state = self.state.lock().unwrap();
        let subscriber = state
            .channels
            .get(channel)
            .and_then(|sockets| sockets.get(socket));
        if members_only && subscriber.is_none() {
            return false;
        }
        let mut frame = Frame::new(event, data, Some(channel));
        frame.user_id = subscriber
            .and_then(Option::as_ref)
            .map(|member| member.user_id.clone());
        state.publish(channel, &frame, Some(socket));
        true
    }

    pub fn publish(&self, channel: &str, event: &str, data: String, excluded: Option<&str>) {
        self.state.lock().unwrap().publish(
            channel,
            &Frame::new(event, data, Some(channel)),
            excluded,
        );
    }
}

impl State {
    fn unsubscribe(&mut self, socket: &str, channel: &str) {
        let Some(subscribers) = self.channels.get_mut(channel) else {
            return;
        };
        let member = subscribers.remove(socket).flatten();
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
            for socket in subscribers.keys() {
                if Some(socket.as_str()) != excluded {
                    self.send(socket, frame);
                }
            }
        }
    }
}

fn has_member(subscribers: &Subscribers, user_id: &str) -> bool {
    subscribers
        .values()
        .flatten()
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

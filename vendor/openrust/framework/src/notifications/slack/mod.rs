//! Slack Bot API messages and Block Kit builders.

mod blocks;
mod slack_message;

pub use blocks::{
    ActionsBlock, ButtonElement, ConfirmObject, ContextBlock, SectionBlock, TextObject,
};
pub use slack_message::SlackMessage;

/// Destination and optional OAuth token for a Slack workspace.
#[derive(Clone, PartialEq)]
pub struct SlackRoute {
    pub(crate) channel: Option<String>,
    pub(crate) token: Option<String>,
}

impl SlackRoute {
    pub fn make(channel: Option<String>, token: Option<String>) -> Self {
        Self { channel, token }
    }
}

impl From<&str> for SlackRoute {
    fn from(channel: &str) -> Self {
        Self::make(Some(channel.into()), None)
    }
}

impl From<String> for SlackRoute {
    fn from(channel: String) -> Self {
        Self::make(Some(channel), None)
    }
}

//! Slack notification messages, routes and Block Kit builders.

mod blocks;
mod event_metadata;
mod selects;
mod slack_message;

pub use blocks::{
    ActionsBlock, ButtonElement, ConfirmObject, ContextBlock, DividerBlock, HeaderBlock,
    ImageBlock, ImageElement, PlainText, SectionBlock, TextObject,
};
pub use event_metadata::EventMetadata;
pub use selects::{StaticSelectElement, UsersSelectElement};
pub use slack_message::SlackMessage;

/// A Slack workspace destination or incoming webhook.
#[derive(Clone, PartialEq)]
pub struct SlackRoute {
    pub(crate) channel: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) webhook: Option<String>,
}

impl SlackRoute {
    pub fn make(channel: Option<String>, token: Option<String>) -> Self {
        Self {
            channel,
            token,
            webhook: None,
        }
    }

    pub fn for_webhook(url: impl Into<String>, channel: Option<String>) -> Self {
        Self {
            channel,
            token: None,
            webhook: Some(url.into()),
        }
    }
}

impl From<&str> for SlackRoute {
    fn from(channel: &str) -> Self {
        String::from(channel).into()
    }
}

impl From<String> for SlackRoute {
    fn from(channel: String) -> Self {
        if channel.starts_with("http://") || channel.starts_with("https://") {
            Self::for_webhook(channel, None)
        } else {
            Self::make(Some(channel), None)
        }
    }
}

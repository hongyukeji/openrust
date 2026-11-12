/// An unauthenticated subscription channel.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub name: String,
}

/// A channel requiring subscription authorization.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PrivateChannel {
    pub name: String,
}

/// A private channel with an authenticated membership roster.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresenceChannel {
    pub name: String,
}

/// A channel declared by an application event.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BroadcastChannel {
    Public(Channel),
    Private(PrivateChannel),
    Presence(PresenceChannel),
}
impl BroadcastChannel {
    /// Uses Laravel's public, private and presence wire names.
    pub fn wire_name(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Public(channel) => (&channel.name).into(),
            Self::Private(channel) => format!("private-{}", channel.name).into(),
            Self::Presence(channel) => format!("presence-{}", channel.name).into(),
        }
    }
}

use regex::Regex;

use crate::{
    auth::Auth,
    contracts::{auth::Authenticatable, broadcasting::ChannelHandler},
    http::Request,
};

use super::{
    BroadcastException, BroadcastResult, ChannelAuthorization, ChannelOptions, ChannelParameters,
};

/// The application's ordered authorization rules for private and presence channels.
#[derive(Default)]
pub struct ChannelRegistry {
    rules: Vec<Rule>,
}

struct Rule {
    name: String,
    pattern: Regex,
    handler: Box<dyn ChannelHandler>,
    options: ChannelOptions,
}

impl ChannelRegistry {
    pub(crate) fn register(
        &mut self,
        name: &str,
        handler: impl ChannelHandler + 'static,
        options: ChannelOptions,
    ) -> BroadcastResult<()> {
        let mut expression = String::from(r"\A");
        let mut rest = name;
        while let Some((literal, parameter)) = rest.split_once('{') {
            if literal.contains('}') {
                return Err(BroadcastException::InvalidChannel);
            }
            let (parameter, remainder) = parameter
                .split_once('}')
                .ok_or(BroadcastException::InvalidChannel)?;
            if parameter.is_empty()
                || !parameter
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(BroadcastException::InvalidChannel);
            }
            expression.push_str(&regex::escape(literal));
            expression.push_str(&format!("(?P<{parameter}>[^.]+)"));
            rest = remainder;
        }
        if name.is_empty() || rest.contains('}') {
            return Err(BroadcastException::InvalidChannel);
        }
        expression.push_str(&regex::escape(rest));
        expression.push_str(r"\z");
        let pattern = Regex::new(&expression).map_err(|_| BroadcastException::InvalidChannel)?;
        let rule = Rule {
            name: name.to_owned(),
            pattern,
            handler: Box::new(handler),
            options,
        };
        if let Some(previous) = self.rules.iter_mut().find(|rule| rule.name == name) {
            *previous = rule;
        } else {
            self.rules.push(rule);
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Runs the matching rule for a driver-normalized channel name and authenticated user.
    pub async fn authorize(
        &self,
        channel: &str,
        user: &dyn Authenticatable,
    ) -> BroadcastResult<ChannelAuthorization> {
        match self.matched(channel) {
            Some((rule, parameters)) => rule.handler.join(user, &parameters).await,
            None => Ok(ChannelAuthorization::Denied),
        }
    }

    /// Resolves the matching rule's guards against the incoming request.
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's broadcast authorization API"
    )]
    pub async fn authorizeRequest(
        &self,
        channel: &str,
        request: &mut Request,
    ) -> BroadcastResult<ChannelAuthorization> {
        let Some((rule, parameters)) = self.matched(channel) else {
            return Ok(ChannelAuthorization::Denied);
        };
        let guards = rule.options.guards.iter().map(|guard| Some(guard.as_str()));
        let default = rule.options.guards.is_empty().then_some(None);
        for name in default.into_iter().chain(guards) {
            let mut guard = Auth::guard(request, name)
                .map_err(|error| BroadcastException::Backend(Box::new(error)))?;
            if let Some(user) = guard
                .user()
                .await
                .map_err(|error| BroadcastException::Backend(Box::new(error)))?
            {
                return rule.handler.join(user, &parameters).await;
            }
        }
        Ok(ChannelAuthorization::Denied)
    }

    pub fn patterns(&self) -> impl Iterator<Item = &str> {
        self.rules.iter().map(|rule| rule.name.as_str())
    }

    fn matched(&self, channel: &str) -> Option<(&Rule, ChannelParameters)> {
        self.rules.iter().find_map(|rule| {
            let captures = rule.pattern.captures(channel)?;
            let parameters = rule
                .pattern
                .capture_names()
                .flatten()
                .filter_map(|name| {
                    captures
                        .name(name)
                        .map(|value| (name.into(), value.as_str().into()))
                })
                .collect();
            Some((rule, parameters))
        })
    }
}

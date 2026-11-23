#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

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
    /// Uses public, private and presence wire names.
    pub fn wire_name(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Public(channel) => (&channel.name).into(),
            Self::Private(channel) => format!("private-{}", channel.name).into(),
            Self::Presence(channel) => format!("presence-{}", channel.name).into(),
        }
    }
}

impl From<Channel> for BroadcastChannel {
    fn from(channel: Channel) -> Self {
        Self::Public(channel)
    }
}

impl From<PrivateChannel> for BroadcastChannel {
    fn from(channel: PrivateChannel) -> Self {
        Self::Private(channel)
    }
}

impl From<PresenceChannel> for BroadcastChannel {
    fn from(channel: PresenceChannel) -> Self {
        Self::Presence(channel)
    }
}

impl From<String> for BroadcastChannel {
    fn from(name: String) -> Self {
        Self::Public(Channel { name })
    }
}

impl From<&str> for BroadcastChannel {
    fn from(name: &str) -> Self {
        name.to_owned().into()
    }
}

use std::{any::Any, collections::HashMap};

use regex::Regex;

use crate::{
    auth::{Auth, AuthIdentifier},
    contracts::{auth::Authenticatable, broadcasting::ChannelHandler, routing::UrlRoutable},
    http::{HttpError, Request, RequestParts},
    routing::{
        BindingContext,
        binding::{self, Resolver, RouteModels},
    },
};

use super::{
    BroadcastException, BroadcastResult, ChannelAuthorization, ChannelOptions, ChannelParameters,
};

/// The application's ordered authorization rules for private and presence channels.
#[derive(Default)]
pub struct ChannelRegistry {
    rules: Vec<Rule>,
    explicit: HashMap<String, Resolver>,
}

struct Rule {
    name: String,
    pattern: Regex,
    handler: Box<dyn ChannelHandler>,
    options: ChannelOptions,
    bindings: ChannelBindings,
}

/// Models to resolve by their capture names using the model route-key lookup.
#[derive(Default)]
pub struct ChannelBindings(HashMap<String, Resolver>);

impl ChannelBindings {
    pub fn model<T: UrlRoutable>(mut self, name: impl Into<String>) -> Self {
        self.0.insert(name.into(), binding::model::<T>());
        self
    }
}

/// Typed values resolved for a matching channel.
#[derive(Default)]
pub struct ChannelModels(RouteModels);

impl ChannelModels {
    pub fn get<T: Any>(&self, name: &str) -> Option<&T> {
        self.0.get(name)?.downcast_ref()
    }
}

impl ChannelRegistry {
    pub(crate) fn register(
        &mut self,
        name: &str,
        handler: impl ChannelHandler + 'static,
        options: ChannelOptions,
        bindings: ChannelBindings,
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
        if bindings
            .0
            .keys()
            .any(|key| !pattern.capture_names().flatten().any(|name| name == key))
        {
            return Err(BroadcastException::InvalidChannel);
        }
        let rule = Rule {
            name: name.to_owned(),
            pattern,
            handler: Box::new(handler),
            options,
            bindings,
        };
        if let Some(previous) = self.rules.iter_mut().find(|rule| rule.name == name) {
            *previous = rule;
        } else {
            self.rules.push(rule);
        }
        Ok(())
    }

    pub(crate) fn use_route_bindings(&mut self, bindings: &HashMap<String, Resolver>) {
        self.explicit.clone_from(bindings);
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
        self.authorize_matching(channel, user, None).await
    }

    /// Resolves the matching rule's guards against the incoming request.
    pub async fn authorize_request(
        &self,
        channel: &str,
        request: &mut Request,
    ) -> BroadcastResult<ChannelAuthorization> {
        Ok(self
            .authorize_request_with_identity(channel, request)
            .await?
            .1)
    }

    /// Returns the identity selected by the channel's guards together with its authorization.
    pub async fn authorize_request_with_identity(
        &self,
        channel: &str,
        request: &mut Request,
    ) -> BroadcastResult<(Option<AuthIdentifier>, ChannelAuthorization)> {
        let Some(rule) = self
            .rules
            .iter()
            .find(|rule| rule.pattern.is_match(channel))
        else {
            return Ok((None, ChannelAuthorization::Denied));
        };
        let parts = request.parts().clone();
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
                let authorization = self.authorize_matching(channel, user, Some(&parts)).await?;
                return Ok((Some(user.get_auth_identifier()), authorization));
            }
        }
        Ok((None, ChannelAuthorization::Denied))
    }

    pub fn patterns(&self) -> impl Iterator<Item = &str> {
        self.rules.iter().map(|rule| rule.name.as_str())
    }

    async fn authorize_matching(
        &self,
        channel: &str,
        user: &dyn Authenticatable,
        request: Option<&RequestParts>,
    ) -> BroadcastResult<ChannelAuthorization> {
        for rule in &self.rules {
            let Some(captures) = rule.pattern.captures(channel) else {
                continue;
            };
            let mut parameters = ChannelParameters::new();
            let mut models = ChannelModels::default();
            for name in rule.pattern.capture_names().flatten() {
                let value = &captures[name];
                parameters.insert(name.into(), value.into());
                let explicit = self.explicit.get(name);
                let Some(resolve) = explicit.or_else(|| rule.bindings.0.get(name)) else {
                    continue;
                };
                let request = request.ok_or(BroadcastException::AccessDenied)?;
                let model = resolve(BindingContext {
                    request,
                    value,
                    field: None,
                    parent: None,
                    with_trashed: false,
                    models: &models.0,
                })
                .await
                .map_err(BroadcastException::Binding)?;
                let model = model.ok_or(if explicit.is_some() {
                    BroadcastException::Binding(HttpError::NotFound)
                } else {
                    BroadcastException::AccessDenied
                })?;
                models.0.insert(name.into(), model);
            }
            let decision = rule.handler.join_bound(user, &parameters, &models).await?;
            if !matches!(decision, ChannelAuthorization::Abstain) {
                return Ok(decision);
            }
        }
        Ok(ChannelAuthorization::Denied)
    }
}

use std::{
    collections::BTreeMap,
    io,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use openrust::{
    auth::AuthIdentifier,
    broadcasting::{
        BroadcastChannel, BroadcastException, BroadcastFuture, BroadcastManager, BroadcastPayload,
        BroadcastResult, ChannelAuthorization,
    },
    contracts::{broadcasting::Broadcaster, foundation::Application as _},
    foundation::{Application, Runner},
    http::{Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider, facades::Broadcast},
};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::protocols::pusher::signatures;

pub struct ReverbServiceProvider;

impl ServiceProvider for ReverbServiceProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> BroadcastFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<BroadcastManager>()
                .await?
                .extend("reverb", |configuration| {
                    let broadcaster = ReverbBroadcaster::new(configuration)?;
                    Ok(Arc::new(broadcaster))
                })?;
            self.commands(app, crate::servers::reverb::console::commands::register);
            Ok(())
        })
    }

    fn runners(&self, app: &Application) -> Vec<Arc<dyn Runner>> {
        vec![Arc::new(crate::servers::reverb::server::ReverbRunner(
            app.config().expect("booted application configuration"),
        ))]
    }
}

#[derive(Deserialize)]
struct Settings {
    key: String,
    secret: String,
    app_id: String,
    options: Options,
}

#[derive(Deserialize)]
struct Options {
    host: String,
    scheme: String,
    port: u16,
    #[serde(default)]
    path: String,
}

pub struct ReverbBroadcaster {
    key: String,
    secret: String,
    url: Url,
    signing_path: String,
    client: reqwest::Client,
}

impl ReverbBroadcaster {
    pub fn new(configuration: &Value) -> BroadcastResult<Self> {
        let settings: Settings =
            serde_json::from_value(configuration.clone()).map_err(|_| configuration_error())?;
        let options = settings.options;
        if !matches!(options.scheme.as_str(), "http" | "https")
            || settings.key.is_empty()
            || settings.key.contains(':')
            || settings.secret.is_empty()
            || settings.app_id.is_empty()
            || !settings
                .app_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || options.host.is_empty()
        {
            return Err(configuration_error());
        }
        let mut url = Url::parse(&format!("{}://localhost", options.scheme)).map_err(backend)?;
        url.set_host(Some(&options.host))
            .map_err(|_| configuration_error())?;
        url.set_port(Some(options.port))
            .map_err(|_| configuration_error())?;
        let signing_path = format!("/apps/{}/events", settings.app_id);
        let prefix = options.path.trim_matches('/');
        if prefix.is_empty() {
            url.set_path(&signing_path);
        } else {
            url.set_path(&format!("/{prefix}{signing_path}"));
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(backend)?;
        Ok(Self {
            key: settings.key,
            secret: settings.secret,
            url,
            signing_path,
            client,
        })
    }

    fn signed_response(
        &self,
        channel: &str,
        socket: &str,
        identity: Option<AuthIdentifier>,
        authorization: ChannelAuthorization,
    ) -> BroadcastResult<Response> {
        let data = match (authorization, identity) {
            (ChannelAuthorization::Allowed, _) if channel.starts_with("private-") => None,
            (ChannelAuthorization::Presence(user_info), Some(identity))
                if channel.starts_with("presence-") =>
            {
                let id = match identity {
                    AuthIdentifier::Integer(id) => id.to_string(),
                    AuthIdentifier::UnsignedInteger(id) => id.to_string(),
                    AuthIdentifier::String(id) => id,
                };
                Some(json!({"user_id":id,"user_info":user_info}).to_string())
            }
            _ => return Err(BroadcastException::AccessDenied),
        };
        let signature =
            signatures::sign_channel(self.secret.as_bytes(), socket, channel, data.as_deref());
        let mut response = json!({"auth": format!("{}:{signature}", self.key)});
        if let Some(data) = data {
            response["channel_data"] = data.into();
        }
        Response::json(StatusCode::OK, response).map_err(backend)
    }

    async fn authorize(
        &self,
        request: &mut Request,
    ) -> BroadcastResult<(String, String, Option<AuthIdentifier>, ChannelAuthorization)> {
        let channel = input(request, "channel_name").await?;
        let socket = input(request, "socket_id").await?;
        let normalized = channel
            .strip_prefix("private-")
            .or_else(|| channel.strip_prefix("presence-"))
            .ok_or(BroadcastException::AccessDenied)?;
        if !crate::protocols::pusher::channels::valid_channel(&channel) {
            return Err(BroadcastException::InvalidChannel);
        }
        let valid_socket = socket.split_once('.').is_some_and(|(first, second)| {
            [first, second].iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 20
                    && part.bytes().all(|byte| byte.is_ascii_digit())
            })
        });
        if !valid_socket {
            return Err(BroadcastException::InvalidChannel);
        }
        let (identity, result) = Broadcast::channelRegistry()?
            .authorizeRequestWithIdentity(normalized, request)
            .await?;
        Ok((channel, socket, identity, result))
    }
}

impl Broadcaster for ReverbBroadcaster {
    fn auth<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>> {
        Box::pin(async move {
            let (channel, socket, identity, result) = self.authorize(request).await?;
            self.signed_response(&channel, &socket, identity, result)
        })
    }

    fn validAuthenticationResponse<'a>(
        &'a self,
        request: &'a mut Request,
        result: ChannelAuthorization,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>> {
        Box::pin(async move {
            let (channel, socket, identity, verified) = self.authorize(request).await?;
            if matches!(result, ChannelAuthorization::Denied)
                || matches!(verified, ChannelAuthorization::Denied)
            {
                return Err(BroadcastException::AccessDenied);
            }
            self.signed_response(&channel, &socket, identity, verified)
        })
    }

    fn broadcast<'a>(
        &'a self,
        channels: &'a [BroadcastChannel],
        event: &'a str,
        payload: &'a BroadcastPayload,
        socket: Option<&'a str>,
    ) -> BroadcastFuture<'a, BroadcastResult<()>> {
        Box::pin(async move {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(backend)?
                .as_secs();
            for batch in channels.chunks(100) {
                let channels: Vec<_> = batch.iter().map(BroadcastChannel::wire_name).collect();
                let mut body = json!({"name": event, "data": Value::Object(payload.clone()).to_string(), "channels": channels});
                if let Some(socket) = socket {
                    body["socket_id"] = socket.into();
                }
                let body = serde_json::to_vec(&body).map_err(backend)?;
                let mut query = BTreeMap::from([
                    ("auth_key".into(), self.key.clone()),
                    ("auth_timestamp".into(), timestamp.to_string()),
                    ("auth_version".into(), "1.0".to_owned()),
                    ("body_md5".into(), signatures::body_checksum(&body)),
                ]);
                let signature = signatures::sign_request(
                    self.secret.as_bytes(),
                    "POST",
                    &self.signing_path,
                    &query,
                    &body,
                );
                query.insert("auth_signature".into(), signature);
                self.client
                    .post(self.url.clone())
                    .query(&query)
                    .header("content-type", "application/json")
                    .body(body)
                    .send()
                    .await
                    .map_err(|error| backend(error.without_url()))?
                    .error_for_status()
                    .map_err(|error| backend(error.without_url()))?;
            }
            Ok(())
        })
    }
}

async fn input(request: &mut Request, key: &str) -> BroadcastResult<String> {
    request
        .input(Some(key))
        .await
        .map_err(|_| BroadcastException::InvalidChannel)?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(BroadcastException::InvalidChannel)
}

fn configuration_error() -> BroadcastException {
    backend(io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid Reverb broadcast connection.",
    ))
}

fn backend(error: impl std::error::Error + Send + Sync + 'static) -> BroadcastException {
    BroadcastException::Backend(Box::new(error))
}

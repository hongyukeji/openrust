use std::{io, time::Duration};

use openrust::{
    contracts::config::Repository,
    foundation::{RunningService, RuntimeResult, RuntimeService},
    http::HttpFuture,
};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::protocols::pusher::server::{Application, ClientEvents, Server};

#[derive(Clone, Deserialize)]
pub(crate) struct Settings {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
    pub(crate) max_request_size: usize,
    scaling: Option<Scaling>,
}

#[derive(Clone, Deserialize)]
struct Scaling {
    enabled: bool,
    channel: String,
    server: RedisSettings,
}

#[derive(Clone, Deserialize)]
struct RedisSettings {
    url: Option<String>,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    database: u32,
    timeout: u64,
}

impl RedisSettings {
    fn connection_url(&self) -> io::Result<String> {
        if let Some(url) = &self.url
            && !url.is_empty()
        {
            return Ok(url.clone());
        }
        let invalid = |_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid Reverb Redis server configuration.",
            )
        };
        let mut url = url::Url::parse("redis://localhost").expect("valid Redis base URL");
        url.set_host(Some(&self.host)).map_err(invalid)?;
        let invalid = |_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid Reverb Redis server configuration.",
            )
        };
        url.set_port(Some(self.port)).map_err(invalid)?;
        url.set_username(self.username.as_deref().unwrap_or(""))
            .map_err(invalid)?;
        url.set_password(self.password.as_deref())
            .map_err(invalid)?;
        url.set_path(&format!("/{}", self.database));
        Ok(url.into())
    }
}

#[derive(Deserialize)]
struct AppSettings {
    app_id: String,
    key: String,
    secret: String,
    allowed_origins: Vec<String>,
    activity_timeout: u64,
    ping_interval: u64,
    max_connections: Option<usize>,
    max_message_size: usize,
    accept_client_events_from: String,
}

pub(crate) struct ReverbRuntime {
    pub(crate) settings: Settings,
    applications: Vec<AppSettings>,
}

impl ReverbRuntime {
    pub(crate) fn from_config(config: &dyn Repository) -> RuntimeResult<Self> {
        let name = config
            .get("reverb.default")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "reverb".into());
        let settings = config
            .get(&format!("reverb.servers.{name}"))
            .and_then(|value| serde_json::from_value(value).ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid Reverb server configuration.",
                )
            })?;
        let applications = config
            .get("reverb.apps.apps")
            .and_then(|value| serde_json::from_value(value).ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid Reverb application configuration.",
                )
            })?;
        Ok(Self {
            settings,
            applications,
        })
    }
}

impl RuntimeService for ReverbRuntime {
    fn prepare(
        &self,
        shutdown: CancellationToken,
    ) -> HttpFuture<'_, RuntimeResult<RunningService>> {
        Box::pin(async move {
            let applications = self
                .applications
                .iter()
                .map(|app| {
                    Ok(Application {
                        id: app.app_id.clone(),
                        key: app.key.clone(),
                        secret: app.secret.clone(),
                        allowed_origins: app.allowed_origins.clone(),
                        activity_timeout: Duration::from_secs(app.activity_timeout),
                        ping_interval: Duration::from_secs(app.ping_interval),
                        max_connections: app.max_connections,
                        max_message_size: app.max_message_size,
                        client_events: match app.accept_client_events_from.as_str() {
                            "members" => ClientEvents::Members,
                            "all" => ClientEvents::All,
                            "none" => ClientEvents::Disabled,
                            _ => {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "Invalid Reverb client events mode.",
                                ));
                            }
                        },
                    })
                })
                .collect::<Result<Vec<_>, io::Error>>()?;
            let mut server = Server::new(
                applications,
                &self.settings.path,
                self.settings.max_request_size,
            )?;
            if let Some(scaling) = self
                .settings
                .scaling
                .as_ref()
                .filter(|scaling| scaling.enabled)
            {
                server = server
                    .with_redis(
                        &scaling.server.connection_url()?,
                        scaling.channel.clone(),
                        Duration::from_secs(scaling.server.timeout),
                    )
                    .await?;
            }
            let listener = TcpListener::bind((self.settings.host.as_str(), self.settings.port))
                .await
                .map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!("Cannot bind Reverb listener: {error}"),
                    )
                })?;
            let address = listener.local_addr()?;
            Ok(Box::pin(async move {
                println!("Reverb listening on ws://{address}");
                server.serve(listener, shutdown).await?;
                Ok(())
            }) as RunningService)
        })
    }
}

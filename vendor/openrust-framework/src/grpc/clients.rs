use std::{
    collections::HashMap,
    fmt,
    net::IpAddr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use serde::Deserialize;
use tonic::{
    Request, Status,
    service::interceptor::InterceptedService,
    transport::{Channel, ClientTlsConfig, Endpoint},
};

use crate::{
    config::Value,
    contracts::{config::Repository, container::Container as _},
    support::facades::current_container,
};

use super::{GrpcInterceptor, GrpcServerOptions, options::Callback};

/// The service accepted by generated Tonic clients.
pub type GrpcChannel = InterceptedService<Channel, GrpcInterceptor>;

/// Client connection configuration and lazy, application-local Tonic channels.
pub struct GrpcClients {
    config: Arc<dyn Repository>,
    channels: Mutex<HashMap<String, GrpcChannel>>,
    credentials: RwLock<HashMap<String, ClientTlsConfig>>,
    interceptors: RwLock<HashMap<String, Vec<Callback>>>,
}

#[derive(Debug)]
pub enum GrpcClientError {
    Unavailable,
    UnknownClient(String),
    InvalidConfig { name: String, field: &'static str },
    NoRuntime,
}

impl fmt::Display for GrpcClientError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => {
                output.write_str("The application gRPC client registry is unavailable")
            }
            Self::UnknownClient(name) => {
                write!(output, "The gRPC client {name:?} is not configured")
            }
            Self::InvalidConfig { name, field } => write!(
                output,
                "Invalid gRPC client {name:?} configuration: {field}"
            ),
            Self::NoRuntime => {
                output.write_str("A Tokio runtime is required to create a gRPC channel")
            }
        }
    }
}

impl std::error::Error for GrpcClientError {}

#[derive(Deserialize)]
struct Configuration {
    endpoint: Option<String>,
    host: Option<String>,
    port: Option<Value>,
    credentials: Option<String>,
    #[serde(default)]
    interceptors: Vec<String>,
    connect_timeout_ms: Option<u64>,
    timeout_ms: Option<u64>,
}

impl GrpcClients {
    pub(crate) fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            channels: Mutex::default(),
            credentials: RwLock::default(),
            interceptors: RwLock::default(),
        }
    }

    /// Registers TLS credentials by the name referenced in the client configuration.
    pub fn register_credentials(&self, name: impl Into<String>, credentials: ClientTlsConfig) {
        self.credentials
            .write()
            .unwrap()
            .insert(name.into(), credentials);
    }

    /// Appends a request interceptor to a named group.
    pub fn register_interceptor(
        &self,
        group: impl Into<String>,
        interceptor: impl Fn(Request<()>) -> Result<Request<()>, Status> + Send + Sync + 'static,
    ) {
        self.interceptors
            .write()
            .unwrap()
            .entry(group.into())
            .or_default()
            .push(Arc::new(interceptor));
    }

    /// Returns a clone of the named Tonic service. The first RPC opens the connection.
    pub fn connect(&self, name: &str) -> Result<GrpcChannel, GrpcClientError> {
        let mut channels = self.channels.lock().unwrap();
        if let Some(channel) = channels.get(name) {
            return Ok(channel.clone());
        }
        let invalid = |field| GrpcClientError::InvalidConfig {
            name: name.into(),
            field,
        };
        let clients = match self.config.get("grpc.clients") {
            None | Some(Value::Null) => return Err(GrpcClientError::UnknownClient(name.into())),
            Some(Value::Object(clients)) => clients,
            _ => return Err(invalid("grpc.clients must be an object")),
        };
        let value = clients
            .get(name)
            .ok_or_else(|| GrpcClientError::UnknownClient(name.into()))?;
        let config: Configuration = serde_json::from_value(value.clone())
            .map_err(|_| invalid("invalid client settings"))?;
        let credentials = config
            .credentials
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(|name| {
                self.credentials
                    .read()
                    .unwrap()
                    .get(name)
                    .cloned()
                    .ok_or_else(|| invalid("credentials group is not registered"))
            })
            .transpose()?;
        let mut endpoint = config.endpoint(credentials.is_some()).map_err(invalid)?;
        if let Some(credentials) = credentials {
            endpoint = endpoint
                .tls_config(credentials)
                .map_err(|_| invalid("invalid TLS credentials"))?;
        }
        let groups = self.interceptors.read().unwrap();
        let interceptors = config
            .interceptors
            .iter()
            .filter_map(|name| groups.get(name))
            .flatten()
            .cloned()
            .collect();
        drop(groups);
        tokio::runtime::Handle::try_current().map_err(|_| GrpcClientError::NoRuntime)?;
        let channel =
            InterceptedService::new(endpoint.connect_lazy(), GrpcInterceptor(interceptors));
        channels.insert(name.into(), channel.clone());
        Ok(channel)
    }
}

impl Configuration {
    fn endpoint(&self, tls: bool) -> Result<Endpoint, &'static str> {
        let scheme = if tls { "https" } else { "http" };
        let address = match (&self.endpoint, &self.host, &self.port) {
            (Some(endpoint), None, None) => endpoint.clone(),
            (None, Some(host), port) => {
                let host = host.trim();
                let authority = match host.parse::<IpAddr>() {
                    Ok(IpAddr::V6(ip)) => format!("[{ip}]"),
                    _ => host.into(),
                };
                let authority = authority
                    .parse::<http::uri::Authority>()
                    .map_err(|_| "host must be a DNS name or IP address")?;
                match (authority.port_u16(), port) {
                    (Some(port), _) if port != 0 => format!("{scheme}://{authority}"),
                    (None, Some(value)) if !authority.as_str().ends_with(':') => {
                        let port = value
                            .as_u64()
                            .and_then(|port| u16::try_from(port).ok())
                            .or_else(|| value.as_str()?.parse().ok())
                            .filter(|port| *port != 0)
                            .ok_or("port must be from 1 to 65535")?;
                        format!("{scheme}://{authority}:{port}")
                    }
                    _ => return Err("port must be from 1 to 65535"),
                }
            }
            _ => return Err("set endpoint or host and port"),
        };
        let uri = address
            .parse::<http::Uri>()
            .map_err(|_| "invalid endpoint URI")?;
        if uri.scheme_str() != Some(scheme)
            || uri.authority().is_none_or(|authority| {
                authority.as_str().contains('@')
                    || (authority.as_str() != authority.host()
                        && authority.port_u16().is_none_or(|port| port == 0))
            })
            || uri.query().is_some()
            || !matches!(uri.path(), "" | "/")
        {
            return Err(
                "endpoint must be an authority with a scheme matching the configured credentials",
            );
        }
        let mut endpoint = Endpoint::from(uri);
        for (duration, setting) in [
            (self.connect_timeout_ms, "connect_timeout_ms"),
            (self.timeout_ms, "timeout_ms"),
        ] {
            if duration == Some(0) {
                return Err(setting);
            }
        }
        if let Some(ms) = self.connect_timeout_ms {
            endpoint = endpoint.connect_timeout(Duration::from_millis(ms));
        }
        if let Some(ms) = self.timeout_ms {
            endpoint = endpoint.timeout(Duration::from_millis(ms));
        }
        Ok(endpoint)
    }
}

/// Resolves the named client from the current application scope.
pub struct Grpc;

impl Grpc {
    pub fn connect(name: &str) -> Result<GrpcChannel, GrpcClientError> {
        Self::clients()?.connect(name)
    }

    pub fn clients() -> Result<Arc<GrpcClients>, GrpcClientError> {
        resolve()
    }

    pub fn server() -> Result<Arc<GrpcServerOptions>, GrpcClientError> {
        resolve()
    }
}

fn resolve<T: Send + Sync + 'static>() -> Result<Arc<T>, GrpcClientError> {
    current_container()
        .and_then(|container| container.make::<T>().ok())
        .ok_or(GrpcClientError::Unavailable)
}

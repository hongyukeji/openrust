use std::{
    collections::HashMap,
    fmt,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Deserialize;
use tonic::{
    Request, Status,
    service::{Interceptor, interceptor::InterceptedService},
    transport::{Channel, Endpoint},
};

use crate::{
    config::Value,
    contracts::{config::Repository, container::Container as _},
    support::facades::current_container,
};

/// The service accepted by generated Tonic clients.
pub type GrpcChannel = InterceptedService<Channel, GrpcInterceptor>;

#[derive(Clone)]
pub struct GrpcInterceptor;

impl Interceptor for GrpcInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        Ok(request)
    }
}

/// Client connection configuration and lazy, application-local Tonic channels.
pub struct GrpcClients {
    config: Arc<dyn Repository>,
    channels: Mutex<HashMap<String, GrpcChannel>>,
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
    connect_timeout_ms: Option<u64>,
    timeout_ms: Option<u64>,
}

impl GrpcClients {
    pub(crate) fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            channels: Mutex::default(),
        }
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
        let endpoint = config.endpoint().map_err(invalid)?;
        tokio::runtime::Handle::try_current().map_err(|_| GrpcClientError::NoRuntime)?;
        let channel = InterceptedService::new(endpoint.connect_lazy(), GrpcInterceptor);
        channels.insert(name.into(), channel.clone());
        Ok(channel)
    }
}

impl Configuration {
    fn endpoint(&self) -> Result<Endpoint, &'static str> {
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
                    (Some(port), _) if port != 0 => format!("http://{authority}"),
                    (None, Some(value)) if !authority.as_str().ends_with(':') => {
                        let port = value
                            .as_u64()
                            .and_then(|port| u16::try_from(port).ok())
                            .or_else(|| value.as_str()?.parse().ok())
                            .filter(|port| *port != 0)
                            .ok_or("port must be from 1 to 65535")?;
                        format!("http://{authority}:{port}")
                    }
                    _ => return Err("port must be from 1 to 65535"),
                }
            }
            _ => return Err("set endpoint or host and port"),
        };
        let uri = address
            .parse::<http::Uri>()
            .map_err(|_| "invalid endpoint URI")?;
        if uri.scheme_str() != Some("http")
            || uri
                .authority()
                .is_none_or(|authority| authority.as_str().contains('@'))
            || uri.query().is_some()
            || !matches!(uri.path(), "" | "/")
        {
            return Err("endpoint must be an http:// authority");
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
        current_container()
            .and_then(|container| container.make::<GrpcClients>().ok())
            .ok_or(GrpcClientError::Unavailable)?
            .connect(name)
    }
}

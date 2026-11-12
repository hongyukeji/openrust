use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    config::Value,
    contracts::{
        broadcasting::{Broadcaster, Factory},
        config::Repository,
    },
    http::{Request, Response},
    log::{Level, LogManager},
};

use super::{
    BroadcastChannel, BroadcastException, BroadcastFuture, BroadcastPayload, BroadcastResult,
    ChannelAuthorization,
};

type Driver = dyn Fn(&Value) -> BroadcastResult<Arc<dyn Broadcaster>> + Send + Sync;

#[derive(Default)]
struct Connections {
    drivers: HashMap<String, Arc<Driver>>,
    resolved: HashMap<String, Arc<dyn Broadcaster>>,
}

/// Resolves named broadcast connections for one application.
pub struct BroadcastManager {
    config: Arc<dyn Repository>,
    log: Arc<LogManager>,
    connections: Mutex<Connections>,
}

impl BroadcastManager {
    pub(crate) fn new(config: Arc<dyn Repository>, log: Arc<LogManager>) -> Self {
        Self {
            config,
            log,
            connections: Mutex::default(),
        }
    }

    /// Registers a driver constructor before resolving its connections.
    pub fn extend(
        &self,
        name: &str,
        driver: impl Fn(&Value) -> BroadcastResult<Arc<dyn Broadcaster>> + Send + Sync + 'static,
    ) -> BroadcastResult<()> {
        if name.trim().is_empty() {
            return Err(BroadcastException::InvalidConnection(name.into()));
        }
        let mut connections = self.connections.lock().unwrap();
        if connections.drivers.contains_key(name) {
            return Err(BroadcastException::DuplicateDriver(name.into()));
        }
        connections.drivers.insert(name.into(), Arc::new(driver));
        Ok(())
    }
}

impl Factory for BroadcastManager {
    fn connection(&self, name: Option<&str>) -> BroadcastResult<Arc<dyn Broadcaster>> {
        let default;
        let name = if let Some(name) = name {
            name
        } else {
            default = match self.config.get("broadcasting.default") {
                Some(Value::String(name)) => name,
                None => "null".into(),
                Some(_) => return Err(BroadcastException::InvalidConnection("default".into())),
            };
            &default
        };

        if let Some(connection) = self.connections.lock().unwrap().resolved.get(name) {
            return Ok(connection.clone());
        }
        let config = self.config.get(&format!("broadcasting.connections.{name}"));
        let fallback = match (name, &config) {
            ("null" | "log", None) => Some(serde_json::json!({ "driver": name })),
            _ => None,
        };
        let config = config
            .or(fallback)
            .ok_or_else(|| BroadcastException::UnknownConnection(name.into()))?;
        let driver = config
            .get("driver")
            .and_then(Value::as_str)
            .ok_or_else(|| BroadcastException::InvalidConnection(name.into()))?;
        let connection: Arc<dyn Broadcaster> = match driver {
            "null" => Arc::new(LocalBroadcaster::Null),
            "log" => Arc::new(LocalBroadcaster::Log(self.log.clone())),
            _ => {
                let constructor = self
                    .connections
                    .lock()
                    .unwrap()
                    .drivers
                    .get(driver)
                    .cloned()
                    .ok_or_else(|| BroadcastException::UnknownDriver(driver.into()))?;
                constructor(&config)?
            }
        };
        Ok(self
            .connections
            .lock()
            .unwrap()
            .resolved
            .entry(name.into())
            .or_insert(connection)
            .clone())
    }
}

enum LocalBroadcaster {
    Null,
    Log(Arc<LogManager>),
}

impl Broadcaster for LocalBroadcaster {
    fn auth<'a>(&'a self, _: &'a mut Request) -> BroadcastFuture<'a, BroadcastResult<Response>> {
        Box::pin(async { Err(BroadcastException::AccessDenied) })
    }

    fn validAuthenticationResponse<'a>(
        &'a self,
        _: &'a mut Request,
        _: ChannelAuthorization,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>> {
        Box::pin(async { Err(BroadcastException::AccessDenied) })
    }

    fn broadcast<'a>(
        &'a self,
        channels: &'a [BroadcastChannel],
        event: &'a str,
        payload: &'a BroadcastPayload,
        socket: Option<&'a str>,
    ) -> BroadcastFuture<'a, BroadcastResult<()>> {
        Box::pin(async move {
            if let Self::Log(log) = self {
                let channels: Vec<_> = channels.iter().map(BroadcastChannel::wire_name).collect();
                let context = [
                    ("channels".into(), serde_json::json!(channels)),
                    ("payload".into(), Value::Object(payload.clone())),
                    ("socket".into(), serde_json::json!(socket)),
                ]
                .into_iter()
                .collect();
                log.log(Level::Info, event, &context)
                    .map_err(|error| BroadcastException::Backend(Box::new(error)))?;
            }
            Ok(())
        })
    }
}

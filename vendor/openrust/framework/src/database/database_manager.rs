use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use serde_json::Value;

use crate::{container::Container, contracts::config::Repository};

use super::{
    Connection, ConnectionResolverInterface, DatabaseError, DatabaseResult,
    connectors::ConnectionFactory, events::QueryRole,
};

tokio::task_local! {
    static CONNECTION_OVERRIDES: Vec<(usize, String)>;
}

/// Resolves configured connections and manages application-local pools.
pub struct DatabaseManager {
    config: Arc<dyn Repository>,
    root: PathBuf,
    connections: Mutex<HashMap<String, Connection>>,
    container: Weak<Container>,
}

#[allow(non_snake_case, reason = "Matches Laravel's DatabaseManager API")]
impl DatabaseManager {
    pub fn new(config: Arc<dyn Repository>, root: impl Into<PathBuf>) -> Self {
        Self {
            config,
            root: root.into(),
            connections: Mutex::default(),
            container: Weak::new(),
        }
    }

    pub(super) fn with_container(mut self, container: Weak<Container>) -> Self {
        self.container = container;
        self
    }

    /// Uses the selected default connection while polling this future.
    pub async fn usingConnection<T>(&self, name: &str, future: impl Future<Output = T>) -> T {
        let mut overrides = CONNECTION_OVERRIDES
            .try_with(Clone::clone)
            .unwrap_or_default();
        overrides.push((std::ptr::from_ref(self).addr(), name.to_owned()));
        CONNECTION_OVERRIDES.scope(overrides, future).await
    }

    pub fn getDefaultConnection(&self) -> DatabaseResult<String> {
        let identity = std::ptr::from_ref(self).addr();
        if let Ok(Some(name)) = CONNECTION_OVERRIDES.try_with(|overrides| {
            overrides
                .iter()
                .rev()
                .find(|(manager, _)| *manager == identity)
                .map(|(_, name)| name.clone())
        }) {
            return Ok(name);
        }
        match self.config.get("database.default") {
            None => Ok("sqlite".to_owned()),
            Some(Value::String(name)) if !name.is_empty() => Ok(name),
            _ => Err(DatabaseError::InvalidConfiguration("database.default")),
        }
    }

    pub fn setDefaultConnection(&self, name: &str) -> DatabaseResult<()> {
        if name.is_empty() {
            return Err(DatabaseError::InvalidConfiguration("database.default"));
        }
        self.config
            .set("database.default", name.into())
            .map_err(|_| DatabaseError::InvalidConfiguration("database.default"))
    }

    fn connections(&self) -> std::sync::MutexGuard<'_, HashMap<String, Connection>> {
        self.connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn resolve_name<'a>(&self, name: Option<&'a str>) -> DatabaseResult<Cow<'a, str>> {
        match name {
            Some(name) => Ok(Cow::Borrowed(name)),
            None => self.getDefaultConnection().map(Cow::Owned),
        }
    }

    pub fn connection(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        let name = self.resolve_name(name)?;
        self.connection_named(&name)
    }

    fn connection_named(&self, name: &str) -> DatabaseResult<Connection> {
        if let Some(connection) = self.connections().get(name).cloned() {
            return Ok(connection);
        }
        let (base, role) = if let Some(base) = name.strip_suffix("::read") {
            (base, Some(QueryRole::Read))
        } else if let Some(base) = name.strip_suffix("::write") {
            (base, Some(QueryRole::Write))
        } else if let Some(base) = name.strip_suffix("::direct") {
            (base, Some(QueryRole::Direct))
        } else {
            (name, None)
        };
        let connection_name = if role == Some(QueryRole::Direct) {
            name
        } else {
            base
        };
        let config = self.config.clone();
        let base_name = base.to_owned();
        let resolved_name = name.to_owned();
        let mut connection =
            ConnectionFactory.make_for_role(connection_name, &self.root, role, move || {
                match config.get("database.connections") {
                    Some(Value::Object(connections)) => connections.get(&base_name).cloned(),
                    None if base_name == "sqlite" => Some(serde_json::json!({"driver":"sqlite"})),
                    None => None,
                    _ => return Err(DatabaseError::InvalidConfiguration("database.connections")),
                }
                .ok_or_else(|| DatabaseError::UnknownConnection(resolved_name.clone()))
            })?;
        connection.model_container.clone_from(&self.container);
        connection.model_timezone = match self.config.get("app.timezone") {
            None => chrono_tz::UTC,
            Some(Value::String(zone)) => zone
                .parse()
                .map_err(|_| DatabaseError::InvalidConfiguration("app.timezone"))?,
            _ => return Err(DatabaseError::InvalidConfiguration("app.timezone")),
        };
        Ok(self
            .connections()
            .entry(name.to_owned())
            .or_insert(connection)
            .clone())
    }

    /// Retires the cached connection's current physical pools.
    pub fn disconnect(&self, name: Option<&str>) -> DatabaseResult<()> {
        let name = self.resolve_name(name)?;
        let connection = self.connections().get(name.as_ref()).cloned();
        if let Some(connection) = connection {
            connection.disconnect();
        }
        Ok(())
    }

    /// Retires the current pools and removes the connection from the manager's cache.
    pub fn purge(&self, name: Option<&str>) -> DatabaseResult<()> {
        let name = self.resolve_name(name)?;
        let connection = self.connections().remove(name.as_ref());
        if let Some(connection) = connection {
            connection.disconnect();
        }
        Ok(())
    }

    /// Rebuilds the named connection's physical pools with the current configuration.
    pub async fn reconnect(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        let name = self.resolve_name(name)?;
        self.disconnect(Some(&name))?;
        let connection = self.connection_named(&name)?;
        connection.establish().await?;
        Ok(connection)
    }
}

impl ConnectionResolverInterface for DatabaseManager {
    fn connection(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        Self::connection(self, name)
    }

    fn get_default_connection(&self) -> DatabaseResult<String> {
        self.getDefaultConnection()
    }

    fn set_default_connection(&self, name: &str) -> DatabaseResult<()> {
        self.setDefaultConnection(name)
    }
}

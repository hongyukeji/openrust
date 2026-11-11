use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde_json::Value;

use crate::contracts::config::Repository;

use super::{
    Connection, ConnectionResolverInterface, DatabaseError, DatabaseResult,
    connectors::ConnectionFactory,
};

tokio::task_local! {
    static CONNECTION_OVERRIDES: Vec<(usize, String)>;
}

/// Resolves configured connections and manages application-local pools.
pub struct DatabaseManager {
    config: Arc<dyn Repository>,
    root: PathBuf,
    connections: Mutex<HashMap<String, Connection>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's DatabaseManager API")]
impl DatabaseManager {
    pub fn new(config: Arc<dyn Repository>, root: impl Into<PathBuf>) -> Self {
        Self {
            config,
            root: root.into(),
            connections: Mutex::default(),
        }
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

    pub fn connection(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        let default;
        let name = match name {
            Some(name) => name,
            None => {
                default = self.getDefaultConnection()?;
                &default
            }
        };
        let lock = || {
            self.connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        };
        if let Some(connection) = lock().get(name).cloned() {
            return Ok(connection);
        }
        let (base, role) = if let Some(base) = name.strip_suffix("::read") {
            (base, Some("read"))
        } else if let Some(base) = name.strip_suffix("::write") {
            (base, Some("write"))
        } else {
            (name, None)
        };
        let config = match self.config.get("database.connections") {
            Some(Value::Object(connections)) => connections.get(base).cloned(),
            None if base == "sqlite" => Some(serde_json::json!({"driver":"sqlite"})),
            None => None,
            _ => return Err(DatabaseError::InvalidConfiguration("database.connections")),
        }
        .ok_or_else(|| DatabaseError::UnknownConnection(name.to_owned()))?;
        let mut connections = lock();
        if let Some(connection) = connections.get(name) {
            return Ok(connection.clone());
        }
        let mut connection = ConnectionFactory.make(base, config, &self.root)?;
        match role {
            Some("read") => {
                if let Some((database, read)) = connection.read_pool.take() {
                    connection.database = database;
                    connection.pool = read;
                }
            }
            Some("write") => connection.read_pool = None,
            _ => {}
        }
        connections.insert(name.to_owned(), connection.clone());
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

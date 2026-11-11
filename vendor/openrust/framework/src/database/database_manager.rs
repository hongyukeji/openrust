use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde_json::Value;

use crate::contracts::config::Repository;

use super::{
    Connection, ConnectionResolverInterface, DatabaseError, DatabaseResult,
    connectors::ConnectionFactory,
};

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

    pub fn getDefaultConnection(&self) -> DatabaseResult<String> {
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
        let config = match self.config.get("database.connections") {
            Some(Value::Object(connections)) => connections.get(name).cloned(),
            None if name == "sqlite" => Some(serde_json::json!({"driver":"sqlite"})),
            None => None,
            _ => return Err(DatabaseError::InvalidConfiguration("database.connections")),
        }
        .ok_or_else(|| DatabaseError::UnknownConnection(name.to_owned()))?;
        let mut connections = lock();
        if let Some(connection) = connections.get(name) {
            return Ok(connection.clone());
        }
        let connection = ConnectionFactory.make(name, config, &self.root)?;
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

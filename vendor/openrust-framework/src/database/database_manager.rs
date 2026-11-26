use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::Value;

use crate::{
    container::Container,
    contracts::{
        config::Repository,
        container::Container as _,
        events::{Dispatcher as _, Event},
    },
    events::{Dispatcher, EventContext},
    support::facades::application_scope,
};

use super::{
    Connection, ConnectionResolverInterface, DatabaseError, DatabaseResult,
    connectors::ConnectionFactory,
    events::QueryRole,
    schema::{MorphKeyType, SchemaDefaults},
    test_mysql::MySqlTestOwners,
    test_query_counts::TestQueryCounts,
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
    schema_defaults: Arc<RwLock<SchemaDefaults>>,
    destructive_commands_prohibited: AtomicBool,
    mysql_test_owners: Option<Arc<MySqlTestOwners>>,
    test_query_counts: Option<Weak<TestQueryCounts>>,
}

impl DatabaseManager {
    pub fn new(config: Arc<dyn Repository>, root: impl Into<PathBuf>) -> Self {
        Self {
            config,
            root: root.into(),
            connections: Mutex::default(),
            container: Weak::new(),
            schema_defaults: Arc::default(),
            destructive_commands_prohibited: AtomicBool::new(false),
            mysql_test_owners: None,
            test_query_counts: None,
        }
    }

    pub(super) fn with_container(mut self, container: Weak<Container>) -> Self {
        self.container = container;
        self
    }

    pub(super) fn localizer(&self) -> DatabaseResult<Arc<crate::localization::Localizer>> {
        if let Some(container) = self.container.upgrade() {
            return container.get("lang").map_err(Into::into);
        }
        let locale = |key| match self.config.get(key) {
            None => Ok("en".to_owned()),
            Some(Value::String(value)) => Ok(value),
            _ => Err(DatabaseError::InvalidConfiguration(key)),
        };
        Ok(Arc::new(crate::localization::Localizer::new(
            crate::localization::FileLoader::new(crate::localization::language_path(&self.root)),
            locale("app.locale")?,
            locale("app.fallback_locale")?,
        )?))
    }

    pub(super) fn configuration(&self) -> &Arc<dyn Repository> {
        &self.config
    }

    pub(super) fn with_test_services(
        mut self,
        owners: Option<Arc<MySqlTestOwners>>,
        query_counts: Option<Weak<TestQueryCounts>>,
    ) -> Self {
        self.mysql_test_owners = owners;
        self.test_query_counts = query_counts;
        self
    }

    pub(super) async fn dispatch_migration_event(&self, event: impl Event) -> DatabaseResult<()> {
        self.dispatch_migration_event_in(event, None).await
    }

    pub(super) async fn dispatch_migration_event_in(
        &self,
        event: impl Event,
        transaction: Option<&mut super::Transaction>,
    ) -> DatabaseResult<()> {
        let Some(owner) = self.container.upgrade() else {
            return Ok(());
        };
        let dispatcher = owner.get::<Dispatcher>("events")?;
        let mut context = transaction.map_or_else(EventContext::new, EventContext::in_transaction);
        application_scope(owner, dispatcher.dispatch(Arc::new(event), &mut context))
            .await
            .map_err(|error| DatabaseError::MigrationEvent(Box::new(error)))?;
        Ok(())
    }

    pub(super) fn set_morph_key_type(&self, kind: MorphKeyType) {
        self.update_schema_defaults(|defaults| defaults.morph_key_type = kind);
    }

    pub(super) fn set_destructive_commands_prohibited(&self, prohibited: bool) {
        self.destructive_commands_prohibited
            .store(prohibited, Ordering::Relaxed);
    }

    pub(super) fn destructive_commands_prohibited(&self) -> bool {
        self.destructive_commands_prohibited.load(Ordering::Relaxed)
    }

    pub(super) fn update_schema_defaults(&self, update: impl FnOnce(&mut SchemaDefaults)) {
        let mut defaults = self
            .schema_defaults
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        update(&mut defaults);
    }

    /// Uses the selected default connection while polling this future.
    pub async fn using_connection<T>(&self, name: &str, future: impl Future<Output = T>) -> T {
        let mut overrides = CONNECTION_OVERRIDES
            .try_with(Clone::clone)
            .unwrap_or_default();
        overrides.push((std::ptr::from_ref(self).addr(), name.to_owned()));
        CONNECTION_OVERRIDES.scope(overrides, future).await
    }

    pub fn get_default_connection(&self) -> DatabaseResult<String> {
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

    pub fn set_default_connection(&self, name: &str) -> DatabaseResult<()> {
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
            None => self.get_default_connection().map(Cow::Owned),
        }
    }

    pub fn connection(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        let name = self.resolve_name(name)?;
        self.connection_named(&name)
    }

    pub(super) fn migration_connection(&self, name: Option<&str>) -> DatabaseResult<Connection> {
        let connection = self.connection(name)?;
        if connection.query_role.is_none() && connection.has_direct_connection() {
            self.connection_named(&format!("{}::direct", connection.get_name()))
        } else {
            Ok(connection)
        }
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
        let mysql_test_owner = self
            .mysql_test_owners
            .as_ref()
            .map(|owners| {
                owners
                    .0
                    .get(base)
                    .cloned()
                    .ok_or_else(|| DatabaseError::UnknownConnection(name.into()))
            })
            .transpose()?;
        let mut connection = ConnectionFactory.make_for_role(
            connection_name,
            &self.root,
            role,
            mysql_test_owner,
            move || {
                match config.get("database.connections") {
                    Some(Value::Object(connections)) => connections.get(&base_name).cloned(),
                    None if base_name == "sqlite" => Some(serde_json::json!({"driver":"sqlite"})),
                    None => None,
                    _ => return Err(DatabaseError::InvalidConfiguration("database.connections")),
                }
                .ok_or_else(|| DatabaseError::UnknownConnection(resolved_name.clone()))
            },
        )?;
        connection.model_container.clone_from(&self.container);
        connection
            .test_query_counts
            .clone_from(&self.test_query_counts);
        connection.schema_defaults.clone_from(&self.schema_defaults);
        connection.model_timezone = match self.config.get("app.timezone") {
            None => jiff::tz::TimeZone::UTC,
            Some(Value::String(zone)) => crate::support::Date::timezone(&zone)
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
        self.get_default_connection()
    }

    fn set_default_connection(&self, name: &str) -> DatabaseResult<()> {
        self.set_default_connection(name)
    }
}

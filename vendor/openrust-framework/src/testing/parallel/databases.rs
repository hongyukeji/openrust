use super::{ParallelTestError, ParallelTesting, isolation::scalar_token, truthy};
use crate::{
    contracts::foundation::Application as _,
    database::{
        Connection, DatabaseManager, configuration_url_parser, connectors::ConnectionFactory,
    },
    foundation::Application,
};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

#[derive(Default)]
pub(super) struct Databases {
    enabled: AtomicBool,
    state: Mutex<Option<State>>,
}
struct State {
    original: Connection,
    configuration: Value,
    connection_name: String,
    name: String,
}
fn error(message: &str) -> ParallelTestError {
    std::io::Error::other(message).into()
}
/*
 * 复用已有URL解析器，先展开URL再更换库名；读、写、直连及候选端点一起处理，
 * 保留凭据、查询参数和连接选项，避免迁移经直连端点误回原数据库。
 */
fn retarget(value: Value, database: &str) -> Result<Value, ParallelTestError> {
    let mut config = configuration_url_parser::parse(value)?;
    if let Some(Value::String(url)) = config.remove("sqlite_url") {
        let query = url
            .split_once('?')
            .map_or(String::new(), |(_, query)| format!("?{query}"));
        config.insert(
            "sqlite_url".into(),
            format!("sqlite://{database}{query}").into(),
        );
    }
    config.insert("database".into(), database.into());
    for role in ["read", "write", "direct"] {
        if let Some(value) = config.remove(role) {
            let value = match value {
                Value::Null | Value::Bool(false) => value,
                Value::Object(ref map) if map.is_empty() => value,
                Value::Array(values) => Value::Array(
                    values
                        .into_iter()
                        .map(|v| retarget(v, database))
                        .collect::<Result<_, _>>()?,
                ),
                Value::String(ref value) if value.is_empty() => Value::String(String::new()),
                value => retarget(value, database)?,
            };
            config.insert(role.into(), value);
        }
    }
    Ok(Value::Object(config))
}
impl Databases {
    pub(super) fn enable(&self) {
        self.enabled.store(true, Ordering::Release);
    }
    fn active(&self, parallel: &ParallelTesting) -> Result<bool, ParallelTestError> {
        Ok(self.enabled.load(Ordering::Acquire) && !truthy(&parallel.option("without_databases")?))
    }
    async fn initialize(
        &self,
        app: &Application,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<State>>, ParallelTestError> {
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            let manager = app.make::<DatabaseManager>().await?;
            let connection = manager.connection(None)?;
            let name = connection.get_database_name().to_owned();
            let connection_name = connection.get_name().to_owned();
            let configuration = app
                .config()?
                .get("database.connections")
                .and_then(|map| map.get(&connection_name).cloned())
                .ok_or_else(|| error("并行数据库缺少具名连接配置"))?;
            // 独立不可变连接快照用于建库/删库，不能复用会随应用配置重连的旧连接。
            let original = if connection.has_direct_connection() {
                let repository = crate::config::Repository::new([("database".into(), serde_json::json!({
                    "default": connection_name, "connections": { &connection_name: configuration.clone() }
                }))].into());
                let manager =
                    DatabaseManager::new(std::sync::Arc::new(repository), app.base_path());
                manager.connection(Some(&format!("{connection_name}::direct")))?
            } else {
                ConnectionFactory.make(
                    "parallel_original",
                    configuration.clone(),
                    app.base_path(),
                )?
            };
            *guard = Some(State {
                original,
                configuration,
                connection_name,
                name,
            });
        }
        Ok(guard)
    }
    fn target(
        state: &State,
        parallel: &ParallelTesting,
    ) -> Result<Option<String>, ParallelTestError> {
        if state.name == ":memory:" {
            return Ok(None);
        }
        let token = scalar_token(parallel.token()?)?;
        if token
            .chars()
            .any(|c| c.is_control() || "/\\<>:\"|?*".contains(c))
        {
            return Err(error("并行数据库token不能包含路径分隔符或保留字符"));
        }
        Ok(Some(format!("{}_test_{token}", state.name)))
    }
    pub(super) async fn prepare_process(
        &self,
        parallel: &ParallelTesting,
        app: &Application,
    ) -> Result<(), ParallelTestError> {
        if !self.active(parallel)? {
            return Ok(());
        }
        let recreate = truthy(&parallel.option("recreate_databases")?);
        let mut guard = self.initialize(app).await?;
        let state = guard.as_mut().expect("已初始化并行数据库");
        if let Some(target) = Self::target(state, parallel)?
            && recreate
        {
            let manager = app.make::<DatabaseManager>().await?;
            for suffix in ["", "::read", "::write", "::direct"] {
                manager.purge(Some(&format!("{}{suffix}", state.connection_name)))?;
            }
            state
                .original
                .get_schema_builder()
                .drop_database_if_exists(&target)
                .await?;
        }
        Ok(())
    }
    pub(super) async fn prepare_case(
        &self,
        parallel: &ParallelTesting,
        app: &Application,
    ) -> Result<Option<(String, bool)>, ParallelTestError> {
        if !self.active(parallel)? {
            return Ok(None);
        }
        let mut guard = self.initialize(app).await?;
        let state = guard.as_mut().expect("已初始化并行数据库");
        let Some(target) = Self::target(state, parallel)? else {
            return Ok(None);
        };
        let config = retarget(state.configuration.clone(), &target)?;
        let probe = ConnectionFactory.make("parallel_probe", config.clone(), app.base_path())?;
        let exists = probe.get_schema_builder().has_table("dummy").await;
        probe.disconnect();
        let created = match exists {
            Ok(_) => false,
            Err(
                crate::database::DatabaseError::Driver { .. }
                | crate::database::DatabaseError::SqlServer { .. },
            ) => {
                let mut schema = state.original.get_schema_builder();
                schema.drop_database_if_exists(&target).await?;
                schema.create_database(&target).await?;
                true
            }
            Err(error) => return Err(error.into()),
        };
        let manager = app.make::<DatabaseManager>().await?;
        for suffix in ["", "::read", "::write", "::direct"] {
            manager.purge(Some(&format!("{}{suffix}", state.connection_name)))?;
        }
        // 用完整连接映射写入，具名连接中的点号不应被当作配置路径分隔符。
        let mut connections = app
            .config()?
            .get("database.connections")
            .ok_or_else(|| error("缺少数据库连接映射"))?;
        connections
            .as_object_mut()
            .ok_or_else(|| error("数据库连接映射必须为对象"))?
            .insert(state.connection_name.clone(), config);
        app.config()?.set("database.connections", connections)?;
        Ok(Some((target, created)))
    }
    pub(super) async fn cleanup_process(
        &self,
        parallel: &ParallelTesting,
        app: &Application,
    ) -> Result<(), ParallelTestError> {
        if !self.active(parallel)? || !truthy(&parallel.option("drop_databases")?) {
            return Ok(());
        }
        let guard = self.initialize(app).await?;
        let state = guard.as_ref().expect("已初始化并行数据库");
        if let Some(target) = Self::target(state, parallel)? {
            let manager = app.make::<DatabaseManager>().await?;
            for suffix in ["", "::read", "::write", "::direct"] {
                manager.purge(Some(&format!("{}{suffix}", state.connection_name)))?;
            }
            state
                .original
                .get_schema_builder()
                .drop_database_if_exists(&target)
                .await?;
        }
        Ok(())
    }
}

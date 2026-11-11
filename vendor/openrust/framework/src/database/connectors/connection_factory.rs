use std::{path::Path, str::FromStr, sync::Arc, time::Duration};

use serde_json::{Map, Value};
use sqlx::{
    Connection as _,
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use super::super::{
    Connection, DatabaseError, DatabaseResult, configuration_url_parser, connection::Pool,
    query::grammars::Grammar,
};

/// Creates logical connections from configuration and driver connectors.
pub struct ConnectionFactory;

impl ConnectionFactory {
    pub fn make(&self, name: &str, config: Value, root: &Path) -> DatabaseResult<Connection> {
        tokio::runtime::Handle::try_current().map_err(|_| invalid("Tokio runtime"))?;
        let mut base = configuration_url_parser::parse(config)?;
        let read = base.remove("read");
        let write = base.remove("write");
        let sticky = Options(base.clone()).boolean("sticky", false)?;
        let write_options = merge(&base, write, "write")?;
        let (database, pool) = make_pool(&write_options, root)?;
        let read_pool = read
            .filter(|value| !value.is_null())
            .map(|read| {
                let options = merge(&base, Some(read), "read")?;
                make_pool(&options, root).map(|(name, pool)| (name.into(), pool))
            })
            .transpose()?;
        let prefix = write_options.string("prefix", "")?;
        let grammar = match &pool {
            Pool::Mysql(_) => {
                Grammar::mysql(prefix, write_options.boolean("use_upsert_alias", false)?)
            }
            Pool::Sqlite(_) => Grammar::sqlite(prefix),
        };
        Ok(Connection {
            name: Arc::from(name),
            prefix_indexes: write_options.boolean("prefix_indexes", false)?,
            grammar,
            database: database.into(),
            pool,
            read_pool,
            sticky,
            routing: Arc::default(),
        })
    }
}

fn merge(
    base: &Map<String, Value>,
    override_config: Option<Value>,
    role: &'static str,
) -> DatabaseResult<Options> {
    let mut options = base.clone();
    let selected = match override_config {
        None | Some(Value::Null) => return Ok(Options(options)),
        Some(Value::Array(configs)) if !configs.is_empty() => {
            let index = fastrand::usize(..configs.len());
            configs
                .into_iter()
                .nth(index)
                .ok_or_else(|| invalid(role))?
        }
        Some(config) => config,
    };
    let selected = configuration_url_parser::parse(selected)?;
    options.extend(selected);
    Ok(Options(options))
}

fn make_pool(config: &Options, root: &Path) -> DatabaseResult<(String, Pool)> {
    let max = u32::try_from(config.integer("max_connections", 10)?)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("max_connections"))?;
    let timeout = config.integer("acquire_timeout", 30)?;
    if timeout == 0 {
        return Err(invalid("acquire_timeout"));
    }
    let timeout = Duration::from_secs(timeout);
    match config.string("driver", "")? {
        "sqlite" => sqlite(config, root, max, timeout),
        "mysql" => mysql(config, root, max, timeout),
        _ => Err(DatabaseError::UnsupportedDriver),
    }
}

struct Options(Map<String, Value>);
impl Options {
    fn string<'a>(&'a self, key: &'static str, default: &'a str) -> DatabaseResult<&'a str> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::String(value)) => Ok(value),
            _ => Err(invalid(key)),
        }
    }

    fn integer(&self, key: &'static str, default: u64) -> DatabaseResult<u64> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(value) => value.as_u64().ok_or_else(|| invalid(key)),
        }
    }

    fn boolean(&self, key: &'static str, default: bool) -> DatabaseResult<bool> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(value) => value.as_bool().ok_or_else(|| invalid(key)),
        }
    }
}

fn invalid(key: &'static str) -> DatabaseError {
    DatabaseError::InvalidConfiguration(key)
}

fn sqlite(
    config: &Options,
    root: &Path,
    max: u32,
    timeout: Duration,
) -> DatabaseResult<(String, Pool)> {
    let url = config.string("sqlite_url", "")?;
    let declared = config.string("database", "database/database.sqlite")?;
    let mut options = if url.is_empty() {
        if declared == ":memory:" {
            SqliteConnectOptions::from_str("sqlite::memory:").map_err(|_| invalid("database"))?
        } else {
            SqliteConnectOptions::new().filename(declared)
        }
    } else {
        SqliteConnectOptions::from_str(url).map_err(|_| invalid("url"))?
    };
    let path = options.get_filename();
    let memory = if url.is_empty() {
        declared == ":memory:"
    } else {
        url.split('?')
            .next()
            .is_some_and(|url| url.ends_with(":memory:"))
            || url.split_once('?').is_some_and(|(_, query)| {
                url::form_urlencoded::parse(query.as_bytes())
                    .any(|(key, value)| key == "mode" && value == "memory")
            })
    };
    let database = if memory {
        let shared = url.split_once('?').is_some_and(|(_, query)| {
            url::form_urlencoded::parse(query.as_bytes())
                .any(|(key, value)| key == "cache" && value == "shared")
        });
        options = options.shared_cache(shared);
        ":memory:".to_owned()
    } else {
        let path = root.join(path);
        if path.to_str().is_none() {
            return Err(invalid("database path encoding"));
        }
        let name = path.to_string_lossy().into_owned();
        options = options.filename(path).create_if_missing(false);
        name
    };
    options = options
        .foreign_keys(config.boolean("foreign_key_constraints", true)?)
        .busy_timeout(Duration::from_millis(
            config.integer("busy_timeout", 60_000)?,
        ));
    let journal = config.string("journal_mode", "")?;
    if !journal.is_empty() {
        options = options.journal_mode(
            journal
                .parse::<SqliteJournalMode>()
                .map_err(|_| invalid("journal_mode"))?,
        );
    }
    let synchronous = config.string("synchronous", "")?;
    if !synchronous.is_empty() {
        options = options.synchronous(
            synchronous
                .parse::<SqliteSynchronous>()
                .map_err(|_| invalid("synchronous"))?,
        );
    }
    match config.0.get("pragmas") {
        Some(Value::Object(pragmas)) => {
            for (key, value) in pragmas {
                if !identifier(key) {
                    return Err(invalid("pragmas"));
                }
                let value = match value {
                    Value::Bool(value) => i32::from(*value).to_string(),
                    Value::Number(value) => value.to_string(),
                    Value::String(value) => format!("'{}'", value.replace('\'', "''")),
                    _ => return Err(invalid("pragmas")),
                };
                options = options.pragma(key.to_owned(), value);
            }
        }
        None | Some(Value::Null) => {}
        _ => return Err(invalid("pragmas")),
    }
    let mut pool = SqlitePoolOptions::new()
        .max_connections(max)
        .acquire_timeout(timeout)
        .before_acquire(|connection, _| {
            Box::pin(async move { Ok(!connection.is_in_transaction()) })
        });
    if memory {
        pool = pool
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None);
    }
    Ok((database, Pool::Sqlite(pool.connect_lazy_with(options))))
}

fn mysql(
    config: &Options,
    root: &Path,
    max: u32,
    timeout: Duration,
) -> DatabaseResult<(String, Pool)> {
    let port = u16::try_from(config.integer("port", 3306)?)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| invalid("port"))?;
    let database = config.string("database", "")?;
    let charset = config.string("charset", "utf8mb4")?;
    let collation = config.string("collation", "")?;
    if !identifier(charset) || (!collation.is_empty() && !identifier(collation)) {
        return Err(invalid("charset or collation"));
    }
    let ca = config.string(
        "ssl_ca",
        config
            .0
            .get("options")
            .and_then(|options| options["ssl_ca"].as_str())
            .unwrap_or(""),
    )?;
    let ssl = config.string(
        "ssl_mode",
        if ca.is_empty() {
            "required"
        } else {
            "verify_identity"
        },
    )?;
    let mode = match ssl {
        "required" => MySqlSslMode::Required,
        "verify_ca" => MySqlSslMode::VerifyCa,
        "verify_identity" => MySqlSslMode::VerifyIdentity,
        _ => return Err(invalid("ssl_mode")),
    };
    let mut options = MySqlConnectOptions::new()
        .port(port)
        .username(config.string("username", "root")?)
        .password(config.string("password", "")?)
        .charset(charset)
        .pipes_as_concat(false)
        .ssl_mode(mode);
    if !database.is_empty() {
        options = options.database(database);
    }
    if !collation.is_empty() {
        options = options.collation(collation);
    }
    if !ca.is_empty() {
        options = options.ssl_ca(root.join(ca));
    }
    let socket = config.string("unix_socket", "")?;
    if !socket.is_empty() {
        options = options.socket(root.join(socket));
    }
    let strict = config.boolean("strict", true)?;
    let mode = if strict {
        "ONLY_FULL_GROUP_BY,STRICT_TRANS_TABLES,NO_ZERO_IN_DATE,NO_ZERO_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION"
    } else {
        "NO_ENGINE_SUBSTITUTION"
    };
    let mut hosts = match config.0.get("host") {
        Some(Value::Array(hosts)) if !hosts.is_empty() => hosts
            .iter()
            .map(|host| {
                host.as_str()
                    .filter(|host| !host.is_empty())
                    .ok_or_else(|| invalid("host"))
            })
            .collect::<DatabaseResult<Vec<_>>>()?,
        _ => vec![config.string("host", "127.0.0.1")?],
    };
    fastrand::shuffle(&mut hosts);
    let pools = hosts
        .into_iter()
        .map(|host| {
            MySqlPoolOptions::new()
                .max_connections(max)
                .acquire_timeout(timeout)
                .after_connect(move |connection, _| {
                    Box::pin(async move {
                        sqlx::query("SET SESSION sql_mode = ?")
                            .bind(mode)
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                })
                .connect_lazy_with(options.clone().host(host))
        })
        .collect();
    Ok((database.to_owned(), Pool::Mysql(pools)))
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

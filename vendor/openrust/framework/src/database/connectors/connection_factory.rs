use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use serde_json::{Map, Value};
use sqlx::{
    Connection as _,
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use super::super::{
    Connection, DatabaseError, DatabaseResult, configuration_url_parser,
    connection::Pool,
    connection_pools::{ConnectionPools, PoolGeneration},
    events::QueryRole,
    query::grammars::Grammar,
    schema::{SqliteJsonTypes, TableOptions},
    test_mysql::MySqlTestOwner,
};

/// Creates logical connections from configuration and driver connectors.
pub struct ConnectionFactory;

impl ConnectionFactory {
    pub fn make(&self, name: &str, config: Value, root: &Path) -> DatabaseResult<Connection> {
        self.make_for_role(name, root, None, None, move || Ok(config.clone()))
    }

    pub(in crate::database) fn make_for_role(
        &self,
        name: &str,
        root: &Path,
        role: Option<QueryRole>,
        mysql_test_owner: Option<Arc<MySqlTestOwner>>,
        reload: impl Fn() -> DatabaseResult<Value> + Send + Sync + 'static,
    ) -> DatabaseResult<Connection> {
        let options = resolve(reload()?, root, role, mysql_test_owner.as_ref())?;
        let sqlite = matches!(options.pools.writer, Pool::Sqlite(_));
        let root = root.to_owned();
        Ok(Connection {
            name: name.into(),
            prefix_indexes: options.prefix_indexes,
            sqlite_json_types: options.sqlite_json_types,
            table_options: options.table_options,
            postgres_dont_drop: options.postgres_dont_drop,
            postgres_schemas: options.postgres_schemas,
            grammar: options.grammar,
            database: options.database,
            pools: Arc::new(ConnectionPools::new(
                options.pools,
                mysql_test_owner.clone(),
                move || {
                    let fresh = resolve(reload()?, &root, role, mysql_test_owner.as_ref())?.pools;
                    if matches!(fresh.writer, Pool::Sqlite(_)) != sqlite {
                        return Err(invalid("reconnecting with a different database driver"));
                    }
                    Ok(fresh)
                },
            )),
            sqlite,
            sticky: options.sticky,
            has_direct_connection: options.has_direct_connection,
            model_timezone: chrono_tz::UTC,
            model_container: std::sync::Weak::new(),
            schema_defaults: Arc::default(),
            routing: Arc::default(),
            query_duration: Arc::default(),
            query_role: role,
            query_log: Arc::default(),
            test_query_counts: None,
            before_executing: Arc::default(),
            before_transaction: Arc::default(),
        })
    }
}

struct ConnectionOptions {
    database: Arc<str>,
    grammar: Grammar,
    prefix_indexes: bool,
    sqlite_json_types: SqliteJsonTypes,
    table_options: Arc<TableOptions>,
    postgres_dont_drop: Arc<[String]>,
    postgres_schemas: Arc<[String]>,
    sticky: bool,
    has_direct_connection: bool,
    pools: PoolGeneration,
}

fn resolve(
    config: Value,
    root: &Path,
    role: Option<QueryRole>,
    mysql_test_owner: Option<&Arc<MySqlTestOwner>>,
) -> DatabaseResult<ConnectionOptions> {
    tokio::runtime::Handle::try_current().map_err(|_| invalid("Tokio runtime"))?;
    let mut base = configuration_url_parser::parse(config)?;
    let read = base.remove("read");
    let write = base.remove("write");
    let direct = base.remove("direct").filter(|value| {
        base.get("driver").and_then(Value::as_str) == Some("pgsql")
            && match value {
                Value::Null | Value::Bool(false) => false,
                Value::Object(config) => !config.is_empty(),
                Value::Array(configs) => !configs.is_empty(),
                Value::String(url) => !url.is_empty(),
                _ => true,
            }
    });
    let has_direct_connection = direct.is_some();
    let use_direct = role == Some(QueryRole::Direct) && has_direct_connection;
    let sticky = Options(base.clone()).boolean("sticky", false)?;
    let write_options = if use_direct {
        let options = merge(&base, direct, "direct")?;
        if options.string("driver", "")? != "pgsql" {
            return Err(invalid("direct driver"));
        }
        options
    } else {
        merge(&base, write, "write")?
    };
    let (mut database, mut writer) = make_pool(&write_options, root, mysql_test_owner)?;
    let read_options = read
        .filter(|value| !value.is_null() && role != Some(QueryRole::Direct))
        .map(|read| merge(&base, Some(read), "read"))
        .transpose()?;
    let mut reader = read_options
        .as_ref()
        .map(|options| make_pool(options, root, mysql_test_owner))
        .transpose()?;
    let prefix = write_options.string("prefix", "")?;
    let grammar = match &writer {
        Pool::Mysql(_) if write_options.string("driver", "")? == "mariadb" => {
            Grammar::mariadb(prefix)
        }
        Pool::Mysql(_) => Grammar::mysql(prefix, write_options.boolean("use_upsert_alias", false)?),
        Pool::Sqlite(_) => Grammar::sqlite(prefix),
        Pool::Postgres(_) => Grammar::postgres(prefix),
        Pool::SqlServer(_) => Grammar::sql_server(prefix),
    };
    match role {
        Some(QueryRole::Read) => {
            if let Some((read_database, read_pool)) = reader.take() {
                database = read_database;
                writer = read_pool;
            }
        }
        Some(QueryRole::Write) => reader = None,
        Some(QueryRole::Direct) | None => {}
    }
    let configured = |key| -> DatabaseResult<Option<String>> {
        let value = write_options.string(key, "")?;
        Ok((!value.is_empty()).then(|| value.to_owned()))
    };
    let table_options = match &writer {
        Pool::Mysql(_) => TableOptions {
            charset: configured("charset")?,
            collation: configured("collation")?,
            engine: configured("engine")?,
        },
        Pool::Postgres(_) => TableOptions {
            charset: configured("charset")?,
            ..TableOptions::default()
        },
        _ => TableOptions::default(),
    };
    let schema_credentials = match (&writer, role) {
        (_, Some(QueryRole::Read)) => None,
        (Pool::Mysql(_), _) => Some(super::super::migrations::SchemaCredentials {
            password: write_options.string("password", "")?.to_owned(),
            ssl_ca: mysql_ssl_ca(&write_options, root)?,
        }),
        (Pool::Postgres(_), _) => Some(super::super::migrations::SchemaCredentials {
            password: write_options.string("password", "")?.to_owned(),
            ssl_ca: postgres_ssl_ca(&write_options, root)?,
        }),
        _ => None,
    };
    let sqlite_json_types = if matches!(writer, Pool::Sqlite(_)) {
        SqliteJsonTypes {
            json: write_options.boolean("use_native_json", false)?,
            jsonb: write_options.boolean("use_native_jsonb", false)?,
        }
    } else {
        SqliteJsonTypes::default()
    };
    let can_create = match &writer {
        Pool::Sqlite(_) if database != ":memory:" => {
            let url = write_options.string("sqlite_url", "")?;
            !url.split_once('?').is_some_and(|(_, query)| {
                url::form_urlencoded::parse(query.as_bytes()).any(|(key, value)| {
                    matches!(
                        (key.as_ref(), value.as_ref()),
                        ("mode", "ro") | ("immutable", "true" | "1")
                    )
                })
            })
        }
        Pool::Mysql(_) | Pool::Postgres(_) => {
            base.get("database").and_then(Value::as_str) == Some(database.as_str())
        }
        _ => false,
    };
    let creation_database = (role != Some(QueryRole::Read) && can_create).then(|| database.clone());
    let postgres_dont_drop = if matches!(writer, Pool::Postgres(_)) {
        match write_options.0.get("dont_drop") {
            None | Some(Value::Null) => vec!["spatial_ref_sys".into()].into(),
            Some(Value::Array(tables)) => tables
                .iter()
                .map(|table| {
                    table
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("dont_drop"))
                })
                .collect::<DatabaseResult<Vec<_>>>()?
                .into(),
            _ => return Err(invalid("dont_drop")),
        }
    } else {
        Arc::default()
    };
    let postgres_schemas = if matches!(writer, Pool::Postgres(_)) {
        let options = if role == Some(QueryRole::Read) {
            read_options.as_ref().unwrap_or(&write_options)
        } else {
            &write_options
        };
        let username = options.string("username", "root")?;
        postgres_search_path(options)?
            .into_iter()
            .map(|schema| {
                if schema == "$user" {
                    username.to_owned()
                } else {
                    schema
                }
            })
            .collect::<Vec<_>>()
            .into()
    } else {
        Arc::default()
    };
    Ok(ConnectionOptions {
        database: database.into(),
        grammar,
        prefix_indexes: write_options.boolean("prefix_indexes", false)?,
        table_options: Arc::new(table_options),
        postgres_dont_drop,
        postgres_schemas,
        sqlite_json_types,
        sticky,
        has_direct_connection,
        pools: PoolGeneration {
            writer,
            schema_credentials,
            creation_database,
            reader: reader.map(|(_, pool)| pool),
            established: Default::default(),
        },
    })
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

fn make_pool(
    config: &Options,
    root: &Path,
    mysql_test_owner: Option<&Arc<MySqlTestOwner>>,
) -> DatabaseResult<(String, Pool)> {
    let driver = config.string("driver", "")?;
    if mysql_test_owner.is_some_and(|owner| driver != "mysql" || !owner.active()) {
        return Err(invalid("inactive or invalid MySQL test schema"));
    }
    let max = u32::try_from(config.integer("max_connections", 10)?)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("max_connections"))?;
    let timeout = config.integer("acquire_timeout", 30)?;
    if timeout == 0 {
        return Err(invalid("acquire_timeout"));
    }
    let timeout = Duration::from_secs(timeout);
    match driver {
        "sqlite" => sqlite(config, root, max, timeout),
        "mysql" | "mariadb" => mysql(config, root, max, timeout, mysql_test_owner),
        "pgsql" => postgres(config, root, max, timeout),
        "sqlsrv" => sql_server(config, root, max, timeout),
        _ => Err(DatabaseError::UnsupportedDriver),
    }
}

struct Options(Map<String, Value>);

fn sql_server(
    options: &Options,
    root: &Path,
    max: u32,
    timeout: Duration,
) -> DatabaseResult<(String, Pool)> {
    let port = u16::try_from(options.integer("port", 1433)?)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| invalid("port"))?;
    let database = options.string("database", "master")?;
    let mut config = tiberius::Config::new();
    config.host(options.string("host", "localhost")?);
    config.port(port);
    config.database(database);
    config.authentication(tiberius::AuthMethod::sql_server(
        options.string("username", "sa")?,
        options.string("password", "")?,
    ));
    config.encryption(tiberius::EncryptionLevel::Required);
    let ca = options.string("ssl_ca", "")?;
    if !ca.is_empty() {
        config.trust_cert_ca(root.join(ca).to_str().ok_or_else(|| invalid("ssl_ca"))?);
    }
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    Ok((
        database.into(),
        Pool::SqlServer(super::super::sql_server::Pool::new(config, max, timeout)),
    ))
}
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

fn postgres(
    config: &Options,
    root: &Path,
    max: u32,
    timeout: Duration,
) -> DatabaseResult<(String, Pool)> {
    let database = config.string("database", "openrust")?;
    let host = config.string("host", "127.0.0.1")?;
    let port = u16::try_from(config.integer("port", 5432)?).map_err(|_| invalid("port"))?;
    let sslmode = config
        .string("sslmode", "prefer")?
        .parse::<PgSslMode>()
        .map_err(|_| invalid("sslmode"))?;
    let path = postgres_search_path(config)?
        .into_iter()
        .map(|schema| Grammar::postgres("").quote(&schema))
        .collect::<DatabaseResult<Vec<_>>>()?
        .join(",");
    let mut options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(config.string("username", "root")?)
        .password(config.string("password", "")?)
        .database(database)
        .ssl_mode(sslmode)
        .options([("search_path", path)]);
    if let Some(certificate) = postgres_ssl_ca(config, root)? {
        options = options.ssl_root_cert(certificate);
    }
    let pool = PgPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(timeout)
        .connect_lazy_with(options);
    Ok((database.to_owned(), Pool::Postgres(pool)))
}

fn postgres_search_path(options: &Options) -> DatabaseResult<Vec<String>> {
    for key in ["search_path", "schema"] {
        let schemas = match options.0.get(key) {
            None | Some(Value::Null) => continue,
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|name| !name.is_empty() && !name.contains('\0'))
                        .map(str::to_owned)
                        .ok_or_else(|| invalid(key))
                })
                .collect::<DatabaseResult<Vec<_>>>()?,
            Some(Value::String(path)) => {
                parse_postgres_search_path(path).ok_or_else(|| invalid(key))?
            }
            _ => return Err(invalid(key)),
        };
        if !schemas.is_empty() {
            return Ok(schemas);
        }
    }
    Ok(vec!["public".into()])
}

fn parse_postgres_search_path(mut path: &str) -> Option<Vec<String>> {
    let mut schemas = Vec::new();
    path = path.trim();
    while !path.is_empty() {
        let (schema, rest) = if let Some(quote @ ('"' | '\'')) = path.chars().next() {
            let mut schema = String::new();
            let mut rest = &path[1..];
            loop {
                let end = rest.find(quote)?;
                schema.push_str(&rest[..end]);
                rest = &rest[end + 1..];
                if let Some(next) = rest.strip_prefix(quote) {
                    schema.push(quote);
                    rest = next;
                } else {
                    break;
                }
            }
            (schema, rest.trim_start())
        } else {
            let end = path.find(',').unwrap_or(path.len());
            let schema = path[..end].trim_end();
            if schema.contains(['"', '\'']) || schema.chars().any(char::is_whitespace) {
                return None;
            }
            (schema.to_owned(), &path[end..])
        };
        if schema.is_empty() || schema.contains('\0') {
            return None;
        }
        schemas.push(schema);
        if rest.is_empty() {
            return Some(schemas);
        }
        path = rest.strip_prefix(',')?.trim_start();
        if path.is_empty() {
            return None;
        }
    }
    Some(schemas)
}

fn postgres_ssl_ca(options: &Options, root: &Path) -> DatabaseResult<Option<PathBuf>> {
    let certificate = options.string("sslrootcert", "")?;
    Ok((!certificate.is_empty()).then(|| root.join(certificate)))
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
    mysql_test_owner: Option<&Arc<MySqlTestOwner>>,
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
    let ca = mysql_ssl_ca(config, root)?;
    let ssl = config.string(
        "ssl_mode",
        if ca.is_none() {
            "required"
        } else {
            "verify_identity"
        },
    )?;
    let ssl_mode = match ssl {
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
        .ssl_mode(ssl_mode);
    if !database.is_empty() {
        options = options.database(database);
    }
    if !collation.is_empty() {
        options = options.collation(collation);
    }
    if let Some(ca) = ca {
        options = options.ssl_ca(ca);
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
    if matches!(ssl_mode, MySqlSslMode::VerifyCa)
        && rustls::crypto::CryptoProvider::get_default().is_none()
    {
        // SQLx's CA-only verifier uses the process provider. Choose the same backend as SQLx.
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    fastrand::shuffle(&mut hosts);
    let pools = hosts
        .into_iter()
        .map(|host| {
            let verify_owner = mysql_test_owner.cloned();
            let active_owner = mysql_test_owner.cloned();
            let pool = MySqlPoolOptions::new()
                .max_connections(max)
                .acquire_timeout(timeout)
                .after_connect(move |connection, _| {
                    let owner = verify_owner.clone();
                    Box::pin(async move {
                        if let Some(owner) = owner {
                            owner.verify_pool(connection).await?;
                        }
                        sqlx::query("SET SESSION sql_mode = ?")
                            .bind(mode)
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                })
                .before_acquire(move |_, _| {
                    let active = active_owner.as_ref().is_none_or(|owner| owner.active());
                    Box::pin(async move { Ok(active) })
                })
                .connect_lazy_with(options.clone().host(host));
            if let Some(owner) = mysql_test_owner {
                owner.attach(&pool);
            }
            pool
        })
        .collect();
    Ok((database.to_owned(), Pool::Mysql(pools)))
}

fn mysql_ssl_ca(config: &Options, root: &Path) -> DatabaseResult<Option<std::path::PathBuf>> {
    let value = config.string(
        "ssl_ca",
        config
            .0
            .get("options")
            .and_then(|options| options["ssl_ca"].as_str())
            .unwrap_or(""),
    )?;
    Ok((!value.is_empty()).then(|| root.join(value)))
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

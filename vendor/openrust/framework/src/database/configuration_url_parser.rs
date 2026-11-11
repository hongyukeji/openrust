use serde_json::{Map, Value};

use super::{DatabaseError, DatabaseResult};

/// Resolves connection URLs before the driver builds its options.
pub(super) fn parse(config: Value) -> DatabaseResult<Map<String, Value>> {
    let mut config = match config {
        Value::Object(config) => config,
        Value::String(url) => Map::from_iter([("url".into(), Value::String(url))]),
        _ => return Err(DatabaseError::InvalidConfiguration("connection")),
    };
    let url = match config.remove("url") {
        None | Some(Value::Null) => return Ok(config),
        Some(Value::String(url)) if url.is_empty() => return Ok(config),
        Some(Value::String(url)) => url,
        _ => return Err(DatabaseError::InvalidConfiguration("url")),
    };
    let (scheme, tail) = url
        .split_once(':')
        .ok_or(DatabaseError::InvalidConfiguration("url"))?;
    let driver = match scheme {
        "mysql2" => "mysql",
        "sqlite3" => "sqlite",
        "postgres" | "postgresql" => "pgsql",
        "mssql" => "sqlsrv",
        driver => driver,
    };
    config.insert("driver".into(), driver.into());
    if driver == "sqlite" {
        config.insert("sqlite_url".into(), format!("sqlite:{tail}").into());
        return Ok(config);
    }
    if driver != "mysql" {
        return Err(DatabaseError::UnsupportedDriver);
    }
    let url = url::Url::parse(&url).map_err(|_| DatabaseError::InvalidConfiguration("url"))?;
    let decode = |value: &str| {
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map(|value| Value::String(value.into_owned()))
            .map_err(|_| DatabaseError::InvalidConfiguration("url"))
    };
    if let Some(host) = url.host_str() {
        config.insert("host".into(), host.trim_matches(['[', ']']).into());
    }
    if let Some(port) = url.port() {
        config.insert("port".into(), port.into());
    }
    if !url.username().is_empty() {
        config.insert("username".into(), decode(url.username())?);
        // An explicit URL username without a password must not inherit another user's secret.
        config.insert(
            "password".into(),
            decode(url.password().unwrap_or_default())?,
        );
    } else if let Some(password) = url.password() {
        config.insert("password".into(), decode(password)?);
    }
    let database = url.path().strip_prefix('/').unwrap_or(url.path());
    if !database.is_empty() {
        config.insert("database".into(), decode(database)?);
    }
    for (key, value) in url.query_pairs() {
        let key = match key.as_ref() {
            "ssl-mode" => "ssl_mode".into(),
            "ssl-ca" => "ssl_ca".into(),
            "socket" => "unix_socket".into(),
            _ => key,
        };
        let value = match key.as_ref() {
            "port" | "max_connections" | "acquire_timeout" => Value::from(
                value
                    .parse::<u64>()
                    .map_err(|_| DatabaseError::InvalidConfiguration("url query"))?,
            ),
            "strict" => Value::from(
                value
                    .parse::<bool>()
                    .map_err(|_| DatabaseError::InvalidConfiguration("url query"))?,
            ),
            _ => Value::String(value.into_owned()),
        };
        config.insert(key.into_owned(), value);
    }
    Ok(config)
}

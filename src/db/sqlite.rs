//! SQLite 连接路径、URI 和每条物理连接的配置。

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};

use super::DatabaseInstanceConfig;
use crate::config::ConfigError;

pub(super) struct SqliteConnectionSettings {
    pub options: SqliteConnectOptions,
    pub url: String,
    pub single_connection: bool,
    pub in_memory: bool,
    pub pragmas: Vec<String>,
}

fn invalid(key: &'static str, code: &'static str) -> crate::Error {
    ConfigError::diagnostic(key, code).into()
}

pub(super) fn connection_settings(
    url: &str,
    config: &DatabaseInstanceConfig,
    application_root: Option<&Path>,
) -> crate::Result<SqliteConnectionSettings> {
    let database = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .ok_or_else(|| invalid("database.url", "invalid_sqlite_url"))?;
    let (path, query) = database.split_once('?').unwrap_or((database, ""));
    let uri = path.starts_with("file:");
    let mut options = if uri {
        // 保留完整 URI 交 SQLite 解析；SQLx 预解码/重包装会改变文件名与 URI 参数。
        SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(true)
    } else {
        SqliteConnectOptions::from_str(url)
            .map_err(|_| invalid("database.url", "invalid_sqlite_url"))?
    };
    // SQLx 对 mode=memory 隐式启用共享缓存，PDO 则保留 SQLite 的私有缓存默认值。
    let mut memory = path == ":memory:" || path == "file::memory:";
    let mut shared = false;
    let parameters = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
    for (key, value) in &parameters {
        match (key.as_ref(), value.as_ref()) {
            ("mode", "memory") => memory = true,
            ("cache", "shared") => shared = true,
            ("cache", "private") => shared = false,
            _ => {}
        }
    }
    // 保留 file: URI；SQLite 自身负责其 URI 参数及命名内存身份。
    let normalized_url;
    if memory || uri {
        options = options.shared_cache(shared);
        // SeaORM 在 options mapper 前仍会用 SQLx 解析 URL；仅供分派的地址不得过滤原生 URI 参数。
        normalized_url = "sqlite::memory:".to_owned();
    } else {
        let filename = options.get_filename();
        let resolved = filename.canonicalize().or_else(|_| {
            application_root.map_or_else(
                || filename.canonicalize(),
                |root| root.join(filename).canonicalize(),
            )
        });
        let resolved =
            resolved.map_err(|_| invalid("database.url", "sqlite_database_does_not_exist"))?;
        if !resolved.is_file() {
            return Err(invalid("database.url", "sqlite_database_is_not_file"));
        }
        // SQLx 0.9 在打开连接前要求 filename.to_str()；不能静默替换路径编码。
        let path = resolved
            .to_str()
            .ok_or_else(|| invalid("database.url", "unsupported_path_encoding"))?;
        normalized_url = format!(
            "sqlite://{}{}{}",
            encode_path(path),
            if query.is_empty() { "" } else { "?" },
            query
        );
        options = options.filename(resolved).create_if_missing(false);
    }
    // PDO SQLite 的默认 timeout 为 60 秒；声明的 PRAGMA 随后逐连接执行。
    options = options
        .foreign_keys(false)
        .busy_timeout(Duration::from_secs(60));
    let pragmas = configured_pragmas(config)?;
    Ok(SqliteConnectionSettings {
        options,
        url: normalized_url,
        single_connection: memory && (path == ":memory:" || path == "file::memory:" || !shared),
        in_memory: memory,
        pragmas,
    })
}

fn configured_pragmas(config: &DatabaseInstanceConfig) -> crate::Result<Vec<String>> {
    let mut pragmas = Vec::new();
    for (name, value) in &config.pragmas {
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphabetic() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
            })
        {
            return Err(invalid("database.pragmas", "invalid_sqlite_pragma_name"));
        }
        let value = pragma_value(value)?;
        pragmas.push(format!("PRAGMA {} = {};", name.to_ascii_lowercase(), value));
    }
    pragmas.push(format!(
        "PRAGMA foreign_keys = {};",
        u8::from(config.foreign_key_constraints)
    ));
    if let Some(timeout) = config.busy_timeout {
        if timeout > i32::MAX as u32 {
            return Err(invalid(
                "database.busy_timeout",
                "invalid_sqlite_busy_timeout",
            ));
        }
        pragmas.push(format!("PRAGMA busy_timeout = {timeout};"));
    }
    if let Some(mode) = &config.journal_mode {
        SqliteJournalMode::from_str(mode)
            .map_err(|_| invalid("database.journal_mode", "invalid_sqlite_journal_mode"))?;
        pragmas.push(format!(
            "PRAGMA journal_mode = {};",
            mode.to_ascii_uppercase()
        ));
    }
    if let Some(mode) = &config.synchronous {
        SqliteSynchronous::from_str(mode)
            .map_err(|_| invalid("database.synchronous", "invalid_sqlite_synchronous"))?;
        pragmas.push(format!(
            "PRAGMA synchronous = {};",
            mode.to_ascii_uppercase()
        ));
    }
    Ok(pragmas)
}

fn pragma_value(value: &Value) -> crate::Result<String> {
    match value {
        Value::Bool(value) => Ok(if *value { "1" } else { "0" }.to_owned()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) if !value.contains('\0') => {
            Ok(format!("'{}'", value.replace('\'', "''")))
        }
        _ => Err(invalid("database.pragmas", "invalid_sqlite_pragma_value")),
    }
}

fn encode_path(path: &str) -> String {
    path.replace('%', "%25")
        .replace('?', "%3F")
        .replace('#', "%23")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pragma_strings_are_single_quoted_and_escape_quotes() -> crate::Result<()> {
        assert_eq!(
            pragma_value(&Value::String("a'; DROP TABLE users;--".into()))?,
            "'a''; DROP TABLE users;--'"
        );
        assert_eq!(pragma_value(&Value::Bool(true))?, "1");
        assert!(pragma_value(&Value::Null).is_err());
        assert!(pragma_value(&serde_json::json!([1])).is_err());
        assert!(pragma_value(&Value::String("\0".into())).is_err());
        Ok(())
    }

    #[test]
    fn pragma_declaration_order_precedes_special_overrides() -> crate::Result<()> {
        let mut config = DatabaseInstanceConfig::default();
        config
            .pragmas
            .insert("synchronous".into(), Value::String("OFF".into()));
        config
            .pragmas
            .insert("foreign_keys".into(), Value::Bool(false));
        config
            .pragmas
            .insert("busy_timeout".into(), Value::from(17));
        config.foreign_key_constraints = true;
        config.busy_timeout = Some(42);
        config.journal_mode = Some("delete".into());
        config.synchronous = Some("full".into());
        let settings = connection_settings("sqlite::memory:", &config, None)?;
        assert_eq!(
            settings.pragmas,
            [
                "PRAGMA synchronous = 'OFF';",
                "PRAGMA foreign_keys = 0;",
                "PRAGMA busy_timeout = 17;",
                "PRAGMA foreign_keys = 1;",
                "PRAGMA busy_timeout = 42;",
                "PRAGMA journal_mode = DELETE;",
                "PRAGMA synchronous = FULL;",
            ]
        );
        Ok(())
    }

    #[test]
    fn defaults_do_not_force_journal_or_synchronous_mode() -> crate::Result<()> {
        let settings =
            connection_settings("sqlite::memory:", &DatabaseInstanceConfig::default(), None)?;
        assert_eq!(settings.pragmas, ["PRAGMA foreign_keys = 1;"]);
        assert!(settings.single_connection);
        Ok(())
    }

    #[test]
    fn uri_encoded_flags_keep_private_and_named_shared_memory_distinct() -> crate::Result<()> {
        let config = DatabaseInstanceConfig::default();
        let shared = connection_settings(
            "sqlite:file:settings-test?%6dode=memory&cache=sha%72ed",
            &config,
            None,
        )?;
        assert!(!shared.single_connection);
        assert_eq!(
            shared.options.get_filename(),
            Path::new("file:settings-test?%6dode=memory&cache=sha%72ed")
        );
        let private = connection_settings("sqlite:file:settings-test?mode=memory", &config, None)?;
        assert!(private.single_connection);
        let anonymous = connection_settings("sqlite://:memory:?cache=shared", &config, None)?;
        assert!(anonymous.single_connection);
        Ok(())
    }

    #[tokio::test]
    async fn physical_connection_defaults_match_pdo_without_forcing_wal() -> crate::Result<()> {
        use sqlx::Connection;

        let settings =
            connection_settings("sqlite::memory:", &DatabaseInstanceConfig::default(), None)?;
        let mut connection = sqlx::SqliteConnection::connect_with(&settings.options)
            .await
            .map_err(|_| invalid("database.url", "sqlite_test_connection_failed"))?;
        for pragma in &settings.pragmas {
            sqlx::query(sqlx::AssertSqlSafe(pragma.as_str()))
                .execute(&mut connection)
                .await
                .map_err(|_| invalid("database.pragmas", "sqlite_test_pragma_failed"))?;
        }
        let timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut connection)
            .await
            .map_err(|_| invalid("database.pragmas", "sqlite_test_query_failed"))?;
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut connection)
            .await
            .map_err(|_| invalid("database.pragmas", "sqlite_test_query_failed"))?;
        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut connection)
            .await
            .map_err(|_| invalid("database.pragmas", "sqlite_test_query_failed"))?;
        assert_eq!(timeout, 60_000);
        assert_eq!(foreign_keys, 1);
        assert_eq!(journal, "memory");
        Ok(())
    }

    #[test]
    fn canonical_url_path_preserves_unicode_and_reserved_characters() {
        assert_eq!(
            encode_path("/临时/100%?#.sqlite"),
            "/临时/100%25%3F%23.sqlite"
        );
    }
}

#[cfg(test)]
mod provider_tests;

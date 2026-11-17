use std::{fs::File, io, path::Path, process::Stdio};

use sqlx::postgres::{PgConnectOptions, PgPool, PgSslMode};
use tokio::process::Command;

use crate::database::{Connection, DatabaseError, DatabaseResult, Value, schema::SchemaFallback};

use super::{SchemaCredentials, run, run_dump};

pub(super) async fn dump(
    connection: &Connection,
    pool: &PgPool,
    credentials: &SchemaCredentials,
    migration_table: Option<&str>,
    output: &mut File,
) -> DatabaseResult<()> {
    let options = pool.connect_options();
    let mut definitions = dump_command(&options, credentials)?;
    definitions.arg("--schema-only");
    let mut streams = vec![definitions];
    if let Some(table) = migration_table {
        let table = ledger_pattern(connection, table).await?;
        let mut ledger = dump_command(&options, credentials)?;
        ledger.args(["--data-only", "--strict-names", "--table", &table]);
        streams.push(ledger);
    }
    for command in streams {
        let raw = tempfile::tempfile().map_err(DatabaseError::SchemaIo)?;
        let mut raw = run_dump(command, "pg_dump", raw).await?;
        io::copy(&mut raw, output).map_err(DatabaseError::SchemaIo)?;
    }
    Ok(())
}

async fn ledger_pattern(connection: &Connection, reference: &str) -> DatabaseResult<String> {
    let builder = connection.getSchemaBuilder();
    let (schema, table) = builder.parseSchemaAndTable(reference, SchemaFallback::None)?;
    let table = connection
        .grammar
        .quote(&format!("{}{table}", connection.getTablePrefix()))?;
    let target = match schema {
        Some(schema) => format!("{}.{table}", connection.grammar.quote(schema)?),
        None => table.clone(),
    };
    let rows = connection.selectFromWriteConnection(
        "select n.nspname::text as name from pg_catalog.pg_class c join pg_catalog.pg_namespace n on n.oid = c.relnamespace where c.oid = pg_catalog.to_regclass(?)",
        &[target.into()],
    ).await?;
    let Some(Value::Text(schema)) = rows.first().and_then(|row| row.get("name")) else {
        return Err(DatabaseError::InvalidSchema("snapshot migration ledger"));
    };
    Ok(format!("{}.{table}", connection.grammar.quote(schema)?))
}

pub(super) async fn load(
    pool: &PgPool,
    credentials: &SchemaCredentials,
    path: &Path,
) -> DatabaseResult<()> {
    let options = pool.connect_options();
    let (program, mut command) = restore_command(&options, credentials, path)?;
    let file = File::open(path).map_err(DatabaseError::SchemaIo)?;
    command.stdin(file).stdout(Stdio::null());
    run(command, program).await
}

fn dump_command(
    options: &PgConnectOptions,
    credentials: &SchemaCredentials,
) -> DatabaseResult<Command> {
    let mut command = command("pg_dump", options, credentials)?;
    command.args(["--no-owner", "--no-acl"]);
    Ok(command)
}

fn restore_command(
    options: &PgConnectOptions,
    credentials: &SchemaCredentials,
    path: &Path,
) -> DatabaseResult<(&'static str, Command)> {
    if path.extension().is_some_and(|extension| extension == "sql") {
        let mut command = command("psql", options, credentials)?;
        command.args([
            "--no-psqlrc",
            "--set=ON_ERROR_STOP=1",
            "--single-transaction",
            "--file=-",
        ]);
        Ok(("psql", command))
    } else {
        let mut command = command("pg_restore", options, credentials)?;
        command.args([
            "--no-owner",
            "--no-acl",
            "--clean",
            "--if-exists",
            "--single-transaction",
        ]);
        Ok(("pg_restore", command))
    }
}

fn command(
    program: &str,
    options: &PgConnectOptions,
    credentials: &SchemaCredentials,
) -> DatabaseResult<Command> {
    let database = options
        .get_database()
        .ok_or(DatabaseError::InvalidSchema("snapshot database"))?;
    let sslmode = match options.get_ssl_mode() {
        PgSslMode::Disable => "disable",
        PgSslMode::Allow => "allow",
        PgSslMode::Prefer => "prefer",
        PgSslMode::Require => "require",
        PgSslMode::VerifyCa => "verify-ca",
        PgSslMode::VerifyFull => "verify-full",
    };
    let host = options
        .get_socket()
        .map(|path| {
            path.to_str()
                .ok_or(DatabaseError::InvalidSchema("PostgreSQL socket"))
        })
        .transpose()?
        .unwrap_or(options.get_host());
    // libpq also accepts connection strings as database names. Quote each selected
    // option inside a single connection string so names remain literal values.
    let target = format!(
        "host={} port={} user={} dbname={} sslmode={sslmode}",
        conninfo_value(host),
        options.get_port(),
        conninfo_value(options.get_username()),
        conninfo_value(database)
    );
    let mut command = Command::new(program);
    command
        .arg("--no-password")
        .arg("--dbname")
        .arg(target)
        .env("PGPASSWORD", &credentials.password)
        .env_remove("PGHOSTADDR")
        .env_remove("PGOPTIONS")
        .env_remove("PGSERVICE")
        .kill_on_drop(true);
    if let Some(ca) = &credentials.ssl_ca {
        command.env("PGSSLROOTCERT", ca);
    }
    Ok(command)
}

fn conninfo_value(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
#[path = "../../../../tests/unit/postgres_schema_state.rs"]
mod tests;

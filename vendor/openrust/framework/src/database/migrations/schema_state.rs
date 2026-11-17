use std::{
    fs::{self, File},
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, LazyLock},
};

use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
use tokio::process::Command;

use crate::database::{Connection, DatabaseError, DatabaseResult, connection::Pool};

mod postgres;

pub(super) const UNSUPPORTED: DatabaseError = DatabaseError::UnsupportedSchema(
    "schema snapshots require a MySQL or PostgreSQL write connection",
);

pub(in crate::database) struct SchemaCredentials {
    pub password: String,
    pub ssl_ca: Option<PathBuf>,
}

/// The selected MySQL write endpoint for the native schema tools.
pub(super) struct SchemaState<'a> {
    options: Arc<MySqlConnectOptions>,
    credentials: &'a SchemaCredentials,
}

pub(super) async fn dump(
    connection: &Connection,
    path: &Path,
    migration_table: Option<&str>,
) -> DatabaseResult<()> {
    let generation = connection.pools.generation()?;
    let credentials = generation.schema_credentials.as_ref().ok_or(UNSUPPORTED)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(DatabaseError::SchemaIo)?;
    let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(DatabaseError::SchemaIo)?;
    if let Pool::Postgres(pool) = &generation.writer {
        postgres::dump(
            connection,
            pool,
            credentials,
            migration_table,
            staging.as_file_mut(),
        )
        .await?;
    } else {
        let state = SchemaState::resolve(&generation.writer, Some(credentials)).await?;
        dump_mysql(connection, &state, migration_table, staging.as_file_mut()).await?;
    }
    staging
        .as_file()
        .sync_all()
        .map_err(DatabaseError::SchemaIo)?;
    staging
        .persist(path)
        .map_err(|failure| DatabaseError::SchemaIo(failure.error))?;
    Ok(())
}

async fn dump_mysql(
    connection: &Connection,
    state: &SchemaState<'_>,
    migration_table: Option<&str>,
    output: &mut File,
) -> DatabaseResult<()> {
    let raw = tempfile::tempfile().map_err(DatabaseError::SchemaIo)?;
    let mut command = state.dump_command()?;
    command.args(["--routines", "--no-data", "--", state.database()?]);
    let mut raw = run_dump(command, "mysqldump", raw).await?;

    remove_auto_increment(&mut raw, output)?;
    if let Some(table) = migration_table {
        let table = format!("{}{table}", connection.getTablePrefix());
        let mut command = state.dump_command()?;
        command.args([
            "--no-create-info",
            "--skip-triggers",
            "--skip-extended-insert",
            "--compact",
            "--complete-insert",
            "--",
            state.database()?,
            &table,
        ]);
        let mut ledger = run_dump(
            command,
            "mysqldump",
            tempfile::tempfile().map_err(DatabaseError::SchemaIo)?,
        )
        .await?;
        io::copy(&mut ledger, output).map_err(DatabaseError::SchemaIo)?;
    }
    Ok(())
}

pub(super) async fn load(connection: &Connection, path: &Path) -> DatabaseResult<()> {
    let generation = connection.pools.generation()?;
    let credentials = generation.schema_credentials.as_ref().ok_or(UNSUPPORTED)?;
    if let Pool::Postgres(pool) = &generation.writer {
        return postgres::load(pool, credentials, path).await;
    }
    let state = SchemaState::resolve(&generation.writer, Some(credentials)).await?;
    let file = File::open(path).map_err(DatabaseError::SchemaIo)?;
    let mut command = state.command("mysql");
    command
        .args(["--binary-mode", "--local-infile=0"])
        .arg(format!("--database={}", state.database()?))
        .stdin(file);
    run(command, "mysql").await
}

impl<'a> SchemaState<'a> {
    async fn resolve(
        pool: &Pool,
        credentials: Option<&'a SchemaCredentials>,
    ) -> DatabaseResult<Self> {
        let (Pool::Mysql(pools), Some(credentials)) = (pool, credentials) else {
            return Err(UNSUPPORTED);
        };
        let (last, others) = pools.split_last().expect("configured MySQL pools");
        for pool in others {
            if pool.acquire().await.is_ok() {
                return Ok(Self {
                    options: pool.connect_options(),
                    credentials,
                });
            }
        }
        last.acquire()
            .await
            .map_err(DatabaseError::driver("connect"))?;
        Ok(Self {
            options: last.connect_options(),
            credentials,
        })
    }

    fn database(&self) -> DatabaseResult<&str> {
        self.options
            .get_database()
            .ok_or(DatabaseError::InvalidSchema("snapshot database"))
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.args(["--no-defaults", "--no-login-paths"]);
        if let Some(socket) = self.options.get_socket() {
            command.args(["--protocol=SOCKET", "--socket"]).arg(socket);
        } else {
            command.args([
                "--protocol=TCP",
                "--host",
                self.options.get_host(),
                "--port",
                &self.options.get_port().to_string(),
            ]);
        }
        let ssl = match self.options.get_ssl_mode() {
            MySqlSslMode::Disabled => "DISABLED",
            MySqlSslMode::Preferred => "PREFERRED",
            MySqlSslMode::Required => "REQUIRED",
            MySqlSslMode::VerifyCa => "VERIFY_CA",
            MySqlSslMode::VerifyIdentity => "VERIFY_IDENTITY",
        };
        command
            .args(["--user", self.options.get_username(), "--ssl-mode", ssl])
            .env("MYSQL_PWD", &self.credentials.password)
            .kill_on_drop(true);
        if let Some(ca) = &self.credentials.ssl_ca {
            command.arg("--ssl-ca").arg(ca);
        }
        command
    }

    fn dump_command(&self) -> DatabaseResult<Command> {
        let mut command = self.command("mysqldump");
        command.args([
            "--no-tablespaces",
            "--skip-add-locks",
            "--skip-comments",
            "--skip-set-charset",
            "--tz-utc",
            "--column-statistics=0",
            "--set-gtid-purged=OFF",
        ]);
        // Validate the target before a subprocess can create anything.
        self.database()?;
        Ok(command)
    }
}

async fn run_dump(
    mut command: Command,
    program: &'static str,
    mut output: File,
) -> DatabaseResult<File> {
    use std::io::Seek as _;
    command
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(DatabaseError::SchemaIo)?);
    run(command, program).await?;
    output.rewind().map_err(DatabaseError::SchemaIo)?;
    Ok(output)
}

async fn run(mut command: Command, program: &'static str) -> DatabaseResult<()> {
    let status = command
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                DatabaseError::UnsupportedSchema(match program {
                    "mysql" => "mysql client is required for schema restore",
                    "psql" => "psql client is required for PostgreSQL schema restore",
                    "pg_restore" => "pg_restore client is required for PostgreSQL archive restore",
                    "pg_dump" => "pg_dump client is required for PostgreSQL schema export",
                    _ => "mysqldump client is required for schema export",
                })
            } else {
                DatabaseError::SchemaIo(error)
            }
        })?;
    if !status.success() {
        return Err(DatabaseError::SchemaProcess { program, status });
    }
    Ok(())
}

fn remove_auto_increment(source: &mut File, output: &mut File) -> DatabaseResult<()> {
    use std::io::{BufRead as _, BufReader};
    static TABLE_OPTIONS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(\) ENGINE=[^ ]+) AUTO_INCREMENT=[0-9]+").expect("valid regex")
    });
    for line in BufReader::new(source).lines() {
        let line = line.map_err(DatabaseError::SchemaIo)?;
        let text = TABLE_OPTIONS.replace(&line, "$1");
        writeln!(output, "{text}").map_err(DatabaseError::SchemaIo)?;
    }
    Ok(())
}

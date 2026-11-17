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
    "schema snapshots require a MySQL, MariaDB, or PostgreSQL write connection",
);

pub(in crate::database) struct SchemaCredentials {
    pub password: String,
    pub ssl_ca: Option<PathBuf>,
}

/// The selected MySQL or MariaDB write endpoint for the native schema tools.
pub(super) struct SchemaState<'a> {
    options: Arc<MySqlConnectOptions>,
    credentials: &'a SchemaCredentials,
    mariadb: bool,
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
        let state =
            SchemaState::resolve(&generation.writer, credentials, connection.is_mariadb()).await?;
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
    let program = if state.mariadb {
        "mariadb-dump"
    } else {
        "mysqldump"
    };
    state.check_client(program).await?;
    let mut command = state.dump_command(program);
    command.args(["--routines", "--no-data", "--", state.database()?]);
    let mut raw = run_dump(command, program, raw).await?;

    remove_auto_increment(&mut raw, output)?;
    if let Some(table) = migration_table {
        let table = format!("{}{table}", connection.getTablePrefix());
        let mut command = state.dump_command(program);
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
            program,
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
    let state =
        SchemaState::resolve(&generation.writer, credentials, connection.is_mariadb()).await?;
    let program = if state.mariadb { "mariadb" } else { "mysql" };
    state.check_client(program).await?;
    let file = File::open(path).map_err(DatabaseError::SchemaIo)?;
    let mut command = state.command(program);
    command
        .args(["--binary-mode", "--local-infile=0"])
        .arg(format!("--database={}", state.database()?))
        .stdin(file);
    run(command, program).await
}

impl<'a> SchemaState<'a> {
    async fn resolve(
        pool: &Pool,
        credentials: &'a SchemaCredentials,
        mariadb: bool,
    ) -> DatabaseResult<Self> {
        let Pool::Mysql(pools) = pool else {
            return Err(UNSUPPORTED);
        };
        let (last, others) = pools.split_last().expect("configured MySQL pools");
        for pool in others {
            if pool.acquire().await.is_ok() {
                return Ok(Self {
                    options: pool.connect_options(),
                    credentials,
                    mariadb,
                });
            }
        }
        last.acquire()
            .await
            .map_err(DatabaseError::driver("connect"))?;
        Ok(Self {
            options: last.connect_options(),
            credentials,
            mariadb,
        })
    }

    fn database(&self) -> DatabaseResult<&str> {
        self.options
            .get_database()
            .ok_or(DatabaseError::InvalidSchema("snapshot database"))
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.arg("--no-defaults");
        if !self.mariadb {
            command.arg("--no-login-paths");
        }
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
        let ssl = self.options.get_ssl_mode();
        if self.mariadb {
            // Without explicit verification, the native client can fall back to plaintext.
            command.args(["--ssl", "--ssl-verify-server-cert"]);
        } else {
            let mode = match ssl {
                MySqlSslMode::Disabled => "DISABLED",
                MySqlSslMode::Preferred => "PREFERRED",
                MySqlSslMode::Required => "REQUIRED",
                MySqlSslMode::VerifyCa => "VERIFY_CA",
                MySqlSslMode::VerifyIdentity => "VERIFY_IDENTITY",
            };
            command.args(["--ssl-mode", mode]);
        }
        command
            .args(["--user", self.options.get_username()])
            .env("MYSQL_PWD", &self.credentials.password)
            .kill_on_drop(true);
        if let Some(ca) = &self.credentials.ssl_ca {
            command.arg("--ssl-ca").arg(ca);
        }
        command
    }

    async fn check_client(&self, program: &'static str) -> DatabaseResult<()> {
        if !self.mariadb {
            return Ok(());
        }
        match self.options.get_ssl_mode() {
            MySqlSslMode::Required => {}
            MySqlSslMode::VerifyIdentity if self.credentials.ssl_ca.is_some() => {}
            _ => {
                return Err(DatabaseError::UnsupportedSchema(
                    "MariaDB schema snapshots require TLS mode required or verify_identity with a CA",
                ));
            }
        }
        let version = Command::new(program)
            .args(["--no-defaults", "--version"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .env_remove("MYSQL_PWD")
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| client_error(program, error))?;
        let version_banner = String::from_utf8_lossy(&version.stdout);
        let supported = version_banner.split_ascii_whitespace().any(|field| {
            let Some((version, _)) = field.split_once("-MariaDB") else {
                return false;
            };
            let mut parts = version.split('.');
            let major = parts.next().and_then(|part| part.parse::<u32>().ok());
            let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
            major.zip(minor).is_some_and(|version| version >= (11, 4))
        });
        if !version.status.success() || !supported {
            return Err(DatabaseError::UnsupportedSchema(
                "MariaDB schema snapshots require MariaDB 11.4 or newer client tools",
            ));
        }
        Ok(())
    }

    fn dump_command(&self, program: &'static str) -> Command {
        let mut command = self.command(program);
        command.args([
            "--no-tablespaces",
            "--skip-add-locks",
            "--skip-comments",
            "--skip-set-charset",
            "--tz-utc",
        ]);
        if !self.mariadb {
            command.args(["--column-statistics=0", "--set-gtid-purged=OFF"]);
        }
        command
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
        .map_err(|error| client_error(program, error))?;
    if !status.success() {
        return Err(DatabaseError::SchemaProcess { program, status });
    }
    Ok(())
}

fn client_error(program: &str, error: io::Error) -> DatabaseError {
    if error.kind() != io::ErrorKind::NotFound {
        return DatabaseError::SchemaIo(error);
    }
    DatabaseError::UnsupportedSchema(match program {
        "mysql" => "mysql client is required for schema restore",
        "psql" => "psql client is required for PostgreSQL schema restore",
        "pg_restore" => "pg_restore client is required for PostgreSQL archive restore",
        "pg_dump" => "pg_dump client is required for PostgreSQL schema export",
        "mariadb" => "mariadb client is required for MariaDB schema restore",
        "mariadb-dump" => "mariadb-dump client is required for MariaDB schema export",
        _ => "mysqldump client is required for schema export",
    })
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

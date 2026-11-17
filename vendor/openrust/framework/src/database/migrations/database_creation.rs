use std::{fs, io, path::PathBuf, sync::Arc};

use sqlx::{
    Connection as _,
    mysql::{MySqlConnectOptions, MySqlConnection, MySqlDatabaseError},
    postgres::{PgConnection, PgDatabaseError},
    sqlite::SqliteError,
};

use crate::database::{
    Connection, DatabaseError, DatabaseResult, connection::Pool, connection_pools::PoolGeneration,
};

const SQLITE_CANTOPEN: u32 = 14;

#[derive(Clone, Copy)]
pub(super) enum MissingDatabase {
    Sqlite,
    Mysql,
    Postgres,
}

impl MissingDatabase {
    pub(super) fn from_error(error: &DatabaseError) -> Option<Self> {
        let DatabaseError::Driver { source, .. } = error else {
            return None;
        };
        let error = source.as_database_error()?;
        if error.try_downcast_ref::<SqliteError>().is_some()
            && error
                .code()
                .and_then(|code| code.parse::<u32>().ok())
                .is_some_and(|code| code & 0xff == SQLITE_CANTOPEN)
        {
            return Some(Self::Sqlite);
        }
        if error
            .try_downcast_ref::<MySqlDatabaseError>()
            .is_some_and(|error| error.number() == 1049)
        {
            return Some(Self::Mysql);
        }
        error
            .try_downcast_ref::<PgDatabaseError>()
            .filter(|error| error.code() == "3D000")
            .map(|_| Self::Postgres)
    }

    pub(super) fn prepare(
        self,
        connection: &Connection,
    ) -> DatabaseResult<Option<DatabaseCreation>> {
        let generation = connection.pools.generation()?;
        if generation.creation_database.as_deref() != Some(connection.getDatabaseName()) {
            return Ok(None);
        }
        let verb = match (self, &generation.writer) {
            (Self::Sqlite, Pool::Sqlite(pool)) => {
                let path = pool.connect_options().get_filename().to_path_buf();
                return Ok(match fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        Some(DatabaseCreation::Sqlite(path))
                    }
                    _ => None,
                });
            }
            (Self::Mysql, Pool::Mysql(_)) => "create database if not exists",
            (Self::Postgres, Pool::Postgres(..)) if connection.getDatabaseName() != "postgres" => {
                "create database"
            }
            _ => return Ok(None),
        };
        let target = connection.grammar.quote(connection.getDatabaseName())?;
        Ok(Some(DatabaseCreation::Server {
            generation,
            sql: format!("{verb} {target}"),
        }))
    }
}

pub(super) enum DatabaseCreation {
    Sqlite(PathBuf),
    Server {
        generation: Arc<PoolGeneration>,
        sql: String,
    },
}

impl DatabaseCreation {
    pub(super) async fn create(self) -> DatabaseResult<()> {
        let (generation, sql) = match self {
            Self::Sqlite(path) => return create_sqlite_database(path).await,
            Self::Server { generation, sql } => (generation, sql),
        };
        if let Pool::Postgres(pool, _) = &generation.writer {
            let options = pool.connect_options().as_ref().clone().database("postgres");
            return tokio::time::timeout(pool.options().get_acquire_timeout(), async {
                let mut session =
                    PgConnection::connect_with(&options)
                        .await
                        .map_err(DatabaseError::driver(
                            "connect to PostgreSQL maintenance database",
                        ))?;
                sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
                    .execute(&mut session)
                    .await
                    .map_err(DatabaseError::driver("create database"))?;
                Ok(())
            })
            .await
            .map_err(|_| DatabaseError::driver("create database")(sqlx::Error::PoolTimedOut))?;
        }
        create_mysql_database(&generation, &sql).await
    }
}

async fn create_sqlite_database(path: PathBuf) -> DatabaseResult<()> {
    let result = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists
                && fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) =>
        {
            Ok(())
        }
        Err(error) => Err(DatabaseError::SchemaIo(error)),
    }
}

async fn create_mysql_database(generation: &PoolGeneration, sql: &str) -> DatabaseResult<()> {
    let (Pool::Mysql(pools), Some(credentials)) =
        (&generation.writer, generation.schema_credentials.as_ref())
    else {
        return Err(DatabaseError::UnsupportedSchema(
            "creating a missing MySQL database",
        ));
    };
    // Pool::acquire reports the last endpoint's error when every attempt fails.
    let pool = pools.last().expect("configured MySQL write pools");
    let selected = pool.connect_options();
    let mut options = MySqlConnectOptions::new()
        .host(selected.get_host())
        .port(selected.get_port())
        .username(selected.get_username())
        .password(&credentials.password)
        .charset(selected.get_charset())
        .ssl_mode(selected.get_ssl_mode());
    if let Some(socket) = selected.get_socket() {
        options = options.socket(socket);
    }
    if let Some(ca) = &credentials.ssl_ca {
        options = options.ssl_ca(ca);
    }
    if let Some(collation) = selected.get_collation() {
        options = options.collation(collation);
    }
    tokio::time::timeout(pool.options().get_acquire_timeout(), async {
        let mut session = MySqlConnection::connect_with(&options)
            .await
            .map_err(DatabaseError::driver("connect without database"))?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut session)
            .await
            .map_err(DatabaseError::driver("create database"))?;
        Ok(())
    })
    .await
    .map_err(|_| DatabaseError::driver("create database")(sqlx::Error::PoolTimedOut))?
}

#[cfg(test)]
#[path = "../../../tests/unit/database_creation.rs"]
mod tests;

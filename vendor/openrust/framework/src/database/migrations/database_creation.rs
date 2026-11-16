use sqlx::{
    Connection as _,
    mysql::{MySqlConnectOptions, MySqlConnection, MySqlDatabaseError},
};

use crate::database::{Connection, DatabaseError, DatabaseResult, connection::Pool};

pub(super) fn missing_mysql_database(error: &DatabaseError) -> bool {
    let DatabaseError::Driver { source, .. } = error else {
        return false;
    };
    source
        .as_database_error()
        .and_then(|error| error.try_downcast_ref::<MySqlDatabaseError>())
        .is_some_and(|error| error.number() == 1049)
}

pub(super) fn can_create(connection: &Connection) -> DatabaseResult<bool> {
    Ok(connection
        .pools
        .generation()?
        .schema_credentials
        .as_ref()
        .is_some_and(|settings| settings.configured_database))
}

pub(super) async fn create_mysql_database(connection: &Connection) -> DatabaseResult<()> {
    let generation = connection.pools.generation()?;
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
    let name = connection.grammar.quote(connection.getDatabaseName())?;
    let sql = format!("create database if not exists {name}");
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

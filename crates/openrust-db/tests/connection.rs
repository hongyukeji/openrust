//! Verifies the configured connection pool against the selected database service.
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust_core::{AppBuilder, Error, config::FrameworkConfig, lifecycle};
use openrust_db::{DatabasePlugin, Db};
use sea_orm::{ConnectionTrait, Statement};

#[tokio::test]
async fn plugin_opens_pool_and_executes_a_query() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database connection: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin);

    let app = lifecycle::finalize(builder).await?;
    let database = app.state().try_get::<Db>()?.connection()?;
    database.execute_unprepared("SELECT 1").await?;

    #[cfg(feature = "db-mysql")]
    if url.starts_with("mysql://") {
        let row = database
            .query_one_raw(Statement::from_string(
                database.get_database_backend(),
                "SELECT @@session.time_zone AS openrust_timezone".to_owned(),
            ))
            .await?
            .ok_or("MySQL timezone query returned no row")?;
        let timezone = row.try_get::<String>("", "openrust_timezone")?;
        assert_eq!(timezone, "+00:00");

        let row = database
            .query_one_raw(Statement::from_string(
                database.get_database_backend(),
                "SHOW SESSION STATUS LIKE 'Ssl_cipher'".to_owned(),
            ))
            .await?
            .ok_or("MySQL TLS query returned no row")?;
        let cipher = row.try_get::<String>("", "Value")?;
        assert!(!cipher.is_empty(), "MySQL TLS must be active");
    }
    Ok(())
}

#[tokio::test]
async fn transaction_commits_success_and_rolls_back_callback_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database transaction: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin);

    let app = lifecycle::finalize(builder).await?;
    let database = app.state().try_get::<Db>()?;
    let connection = database.connection()?;
    let table = format!(
        "openrust_db_transaction_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    connection
        .execute_unprepared(&format!("CREATE TABLE {table} (id BIGINT PRIMARY KEY)"))
        .await?;

    let table_for_commit = table.clone();
    let committed_id = database
        .transaction(|transaction| {
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!("INSERT INTO {table_for_commit} (id) VALUES (1)"))
                    .await
                    .map_err(Error::internal)?;
                Ok(1_i64)
            })
        })
        .await?;
    assert_eq!(committed_id, 1);

    let table_for_rollback = table.clone();
    let rolled_back = database
        .transaction(|transaction| {
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!(
                        "INSERT INTO {table_for_rollback} (id) VALUES (2)"
                    ))
                    .await
                    .map_err(Error::internal)?;
                Err::<(), _>(Error::internal(std::io::Error::other("force rollback")))
            })
        })
        .await;
    assert!(rolled_back.is_err());

    let count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {table}"),
        ))
        .await?
        .ok_or("transaction row count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    connection
        .execute_unprepared(&format!("DROP TABLE {table}"))
        .await?;
    assert_eq!(count, 1, "callback errors must roll back their writes");
    Ok(())
}

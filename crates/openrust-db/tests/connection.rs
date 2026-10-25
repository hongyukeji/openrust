//! Verifies the configured connection pool against the selected database service.
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust_core::{AppBuilder, config::FrameworkConfig, lifecycle};
use openrust_db::{DatabasePlugin, Db};
use sea_orm::ConnectionTrait;
#[cfg(feature = "db-mysql")]
use sea_orm::Statement;

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

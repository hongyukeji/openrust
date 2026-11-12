use std::{error::Error, fs::File};

use serde_json::json;

use crate::{
    contracts::foundation::Application as _,
    database::{Connection, DatabaseManager, Value},
    foundation::{Application, configuration::ApplicationBuilder},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::TestApp;

const FILE: &str = "testing.sqlite";

struct Sqlite;

impl ServiceProvider for Sqlite {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            let config = app.config()?;
            config.set(
                "database.connections",
                json!({
                    "sqlite": {"driver": "sqlite", "database": FILE}
                }),
            )?;
            config.set("database.default", json!("sqlite"))?;
            Ok(())
        })
    }
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's database testing assertions"
)]
impl TestApp {
    /// Creates a test application with a private SQLite file.
    ///
    /// The fixture replaces the configured connections during provider registration.
    /// Migrations and seeders can be registered through the application builder.
    pub fn sqlite(
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let fixture =
            Self::create(|builder| configure(builder.withProviders(vec![Box::new(Sqlite)])))?;
        File::options()
            .write(true)
            .create_new(true)
            .open(fixture.root().join(FILE))?;
        Ok(fixture)
    }

    /// Boots the test application and resolves its default database connection.
    pub async fn database(&mut self) -> Result<Connection, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self
            .app()
            .make::<DatabaseManager>()
            .await?
            .connection(None)?)
    }

    /// Asserts that a table contains a row matching the supplied values.
    pub async fn assertDatabaseHas(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        assert!(
            self.database_contains(table, columns).await?,
            "no matching row in {table}"
        );
        Ok(())
    }

    /// Asserts that a table has no row matching the supplied values.
    pub async fn assertDatabaseMissing(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        assert!(
            !self.database_contains(table, columns).await?,
            "matching row found in {table}"
        );
        Ok(())
    }

    /// Asserts that a table contains exactly the specified number of rows.
    pub async fn assertDatabaseCount(
        &mut self,
        table: &str,
        expected: u64,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        assert_eq!(
            self.database().await?.table(table).count("*").await?,
            expected,
            "row count for {table}"
        );
        Ok(())
    }

    async fn database_contains(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let connection = self.database().await?;
        let query = columns
            .iter()
            .fold(connection.table(table), |query, (column, value)| {
                query.whereNullSafeEquals(column, value.clone())
            });
        Ok(query.exists().await?)
    }
}

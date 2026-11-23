use std::{error::Error, io};

use crate::database::{Value, eloquent::Model, query::Builder};

use super::TestApp;

type AssertionResult = Result<(), Box<dyn Error + Send + Sync>>;

impl TestApp {
    /// Expects exactly this many queries from this point until fixture teardown.
    /// With no connection name, all connections of this application count.
    pub async fn expects_database_query_count(
        &mut self,
        expected: u64,
        connection: Option<&str>,
    ) -> AssertionResult {
        let selected = self.database_on(connection).await?;
        let label = connection.unwrap_or_else(|| selected.get_name()).to_owned();
        self.query_counts
            .expect(expected, label, connection.map(|_| selected));
        Ok(())
    }

    /// Asserts that the default connection has a matching row.
    pub async fn assert_database_has(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> AssertionResult {
        self.assert_database_has_on(table, columns, None).await
    }

    /// Asserts that the selected connection has a matching row.
    pub async fn assert_database_has_on(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
        connection: Option<&str>,
    ) -> AssertionResult {
        let query = self.matching_rows(table, columns, connection).await?;
        assert!(query.exists().await?, "no matching row in {table}");
        Ok(())
    }

    /// Asserts that the default connection has no matching row.
    pub async fn assert_database_missing(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> AssertionResult {
        self.assert_database_missing_on(table, columns, None).await
    }

    /// Asserts that the selected connection has no matching row.
    pub async fn assert_database_missing_on(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
        connection: Option<&str>,
    ) -> AssertionResult {
        let query = self.matching_rows(table, columns, connection).await?;
        assert!(!query.exists().await?, "matching row found in {table}");
        Ok(())
    }

    /// Asserts that the table contains the expected number of rows.
    pub async fn assert_database_count(&mut self, table: &str, expected: u64) -> AssertionResult {
        self.assert_database_count_on(table, expected, None).await
    }

    /// Asserts the row count on a selected connection.
    pub async fn assert_database_count_on(
        &mut self,
        table: &str,
        expected: u64,
        connection: Option<&str>,
    ) -> AssertionResult {
        assert_eq!(
            self.database_on(connection)
                .await?
                .table(table)
                .count("*")
                .await?,
            expected,
            "row count for {table}"
        );
        Ok(())
    }

    /// Asserts that the table is empty on the default connection.
    pub async fn assert_database_empty(&mut self, table: &str) -> AssertionResult {
        self.assert_database_empty_on(table, None).await
    }

    /// Asserts that the table is empty on a selected connection.
    pub async fn assert_database_empty_on(
        &mut self,
        table: &str,
        connection: Option<&str>,
    ) -> AssertionResult {
        self.assert_database_count_on(table, 0, connection).await
    }

    /// Asserts that the table has a matching row with a non-null `deleted_at`.
    pub async fn assert_soft_deleted(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> AssertionResult {
        self.assert_soft_deleted_on(table, columns, None, "deleted_at")
            .await
    }

    /// Asserts soft deletion with a selected connection and column.
    pub async fn assert_soft_deleted_on(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
        connection: Option<&str>,
        deleted_at: &str,
    ) -> AssertionResult {
        let query = self.matching_rows(table, columns, connection).await?;
        assert!(
            query.where_not_null(deleted_at).exists().await?,
            "no matching soft-deleted row in {table}"
        );
        Ok(())
    }

    /// Asserts that the table has a matching row with a null `deleted_at`.
    pub async fn assert_not_soft_deleted(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
    ) -> AssertionResult {
        self.assert_not_soft_deleted_on(table, columns, None, "deleted_at")
            .await
    }

    /// Asserts that a matching live row exists on a selected connection.
    pub async fn assert_not_soft_deleted_on(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
        connection: Option<&str>,
        deleted_at: &str,
    ) -> AssertionResult {
        let query = self.matching_rows(table, columns, connection).await?;
        assert!(
            query.where_null(deleted_at).exists().await?,
            "no matching row with a null {deleted_at} in {table}"
        );
        Ok(())
    }

    /// Checks a model's primary key in its table, including soft-deleted rows.
    pub async fn assert_model_exists(&mut self, model: &impl Model) -> AssertionResult {
        self.assert_database_has_on(
            model.get_table(),
            &model_key(model),
            model.get_connection_name(),
        )
        .await
    }

    /// Asserts that the model's primary key is absent from its table.
    pub async fn assert_model_missing(&mut self, model: &impl Model) -> AssertionResult {
        self.assert_database_missing_on(
            model.get_table(),
            &model_key(model),
            model.get_connection_name(),
        )
        .await
    }

    /// Checks the model's primary key and configured soft-deletion column.
    pub async fn assert_soft_deleted_model(&mut self, model: &impl Model) -> AssertionResult {
        self.assert_soft_deleted_on(
            model.get_table(),
            &model_key(model),
            model.get_connection_name(),
            model_deleted_at(model)?,
        )
        .await
    }

    /// Asserts that the model has a matching live row in its table.
    pub async fn assert_not_soft_deleted_model(&mut self, model: &impl Model) -> AssertionResult {
        self.assert_not_soft_deleted_on(
            model.get_table(),
            &model_key(model),
            model.get_connection_name(),
            model_deleted_at(model)?,
        )
        .await
    }

    async fn matching_rows(
        &mut self,
        table: &str,
        columns: &[(&str, Value)],
        connection: Option<&str>,
    ) -> Result<Builder<'static>, Box<dyn Error + Send + Sync>> {
        let query = self.database_on(connection).await?.table(table);
        Ok(columns.iter().fold(query, |query, (column, value)| {
            query.where_null_safe_equals(column, value.clone())
        }))
    }
}

fn model_key(model: &impl Model) -> [(&'static str, Value); 1] {
    [(
        model.get_key_name(),
        model.get_key().cloned().unwrap_or(Value::Null),
    )]
}

fn model_deleted_at<M: Model>(model: &M) -> io::Result<&'static str> {
    M::metadata().deleted_at_column().ok_or_else(|| {
        io::Error::other(format!(
            "model table {} has no soft-deletion column",
            model.get_table()
        ))
    })
}

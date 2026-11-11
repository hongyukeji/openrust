use crate::database::{DB, DatabaseResult};

use super::{Blueprint, Builder, Column, ForeignKey, Index, TableInfo};

/// Schema operations on the current application's default database connection.
pub struct Schema;

#[allow(non_snake_case, reason = "Matches Laravel's Schema facade")]
impl Schema {
    pub fn connection(name: &str) -> DatabaseResult<Builder<'static>> {
        Ok(DB::connection(Some(name))?.getSchemaBuilder())
    }

    fn default_builder() -> DatabaseResult<Builder<'static>> {
        Ok(DB::connection(None)?.getSchemaBuilder())
    }

    pub async fn create(table: &str, configure: impl FnOnce(&mut Blueprint)) -> DatabaseResult<()> {
        Self::default_builder()?.create(table, configure).await
    }

    pub async fn table(table: &str, configure: impl FnOnce(&mut Blueprint)) -> DatabaseResult<()> {
        Self::default_builder()?.table(table, configure).await
    }

    pub async fn drop(table: &str) -> DatabaseResult<()> {
        Self::default_builder()?.drop(table).await
    }

    pub async fn dropIfExists(table: &str) -> DatabaseResult<()> {
        Self::default_builder()?.dropIfExists(table).await
    }

    pub async fn rename(from: &str, to: &str) -> DatabaseResult<()> {
        Self::default_builder()?.rename(from, to).await
    }

    pub async fn hasTable(table: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.hasTable(table).await
    }

    pub async fn getTables(schema: Option<&str>) -> DatabaseResult<Vec<TableInfo>> {
        Self::default_builder()?.getTables(schema).await
    }

    pub async fn hasColumn(table: &str, column: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.hasColumn(table, column).await
    }

    pub async fn hasColumns(table: &str, columns: &[&str]) -> DatabaseResult<bool> {
        Self::default_builder()?.hasColumns(table, columns).await
    }

    pub async fn getColumns(table: &str) -> DatabaseResult<Vec<Column>> {
        Self::default_builder()?.getColumns(table).await
    }

    pub async fn getColumnListing(table: &str) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?.getColumnListing(table).await
    }

    pub async fn getColumnType(table: &str, column: &str, full: bool) -> DatabaseResult<String> {
        Self::default_builder()?
            .getColumnType(table, column, full)
            .await
    }

    pub async fn getIndexes(table: &str) -> DatabaseResult<Vec<Index>> {
        Self::default_builder()?.getIndexes(table).await
    }

    pub async fn getIndexListing(table: &str) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?.getIndexListing(table).await
    }

    pub async fn hasIndex(table: &str, name: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.hasIndex(table, name).await
    }

    pub async fn getForeignKeys(table: &str) -> DatabaseResult<Vec<ForeignKey>> {
        Self::default_builder()?.getForeignKeys(table).await
    }
}

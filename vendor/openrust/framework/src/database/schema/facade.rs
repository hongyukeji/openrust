use crate::{
    database::{DB, DatabaseError, DatabaseFuture, DatabaseResult},
    support::facades::Facade,
};

use super::{
    Blueprint, Builder, Column, ForeignKey, Index, KeySelector, MorphKeyType, SchemaInfo,
    SchemaSession, TableInfo, ViewInfo,
};

/// Schema operations on the current application's default database connection.
pub struct Schema;

#[allow(non_snake_case, reason = "Matches Laravel's Schema facade")]
impl Schema {
    /// Sets the default for subsequent polymorphic columns in this application.
    pub fn defaultMorphKeyType(kind: MorphKeyType) -> DatabaseResult<()> {
        DB::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .set_morph_key_type(kind);
        Ok(())
    }

    /// Sets the default length of subsequent string and char columns in this application.
    pub fn defaultStringLength(length: u32) -> DatabaseResult<()> {
        if !(1..=65_535).contains(&length) {
            return Err(DatabaseError::InvalidSchema("default string length"));
        }
        DB::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .update_schema_defaults(|defaults| defaults.string_length = length);
        Ok(())
    }

    /// Sets the default fractional seconds precision for subsequent schema columns.
    pub fn defaultTimePrecision(precision: Option<u8>) -> DatabaseResult<()> {
        let precision = precision.unwrap_or(0);
        if precision > 6 {
            return Err(DatabaseError::InvalidSchema("time precision"));
        }
        DB::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .update_schema_defaults(|defaults| defaults.time_precision = precision);
        Ok(())
    }

    pub fn morphUsingUuids() -> DatabaseResult<()> {
        Self::defaultMorphKeyType(MorphKeyType::Uuid)
    }

    pub fn morphUsingUlids() -> DatabaseResult<()> {
        Self::defaultMorphKeyType(MorphKeyType::Ulid)
    }

    pub fn connection(name: &str) -> DatabaseResult<Builder<'static>> {
        Ok(DB::connection(Some(name))?.getSchemaBuilder())
    }

    fn default_builder() -> DatabaseResult<Builder<'static>> {
        Ok(DB::connection(None)?.getSchemaBuilder())
    }

    pub async fn session() -> DatabaseResult<SchemaSession> {
        Self::default_builder()?.session().await
    }

    pub async fn withoutForeignKeyConstraints<T>(
        callback: impl for<'a> FnOnce(&'a mut SchemaSession) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
    ) -> DatabaseResult<T> {
        Self::default_builder()?
            .withoutForeignKeyConstraints(callback)
            .await
    }

    pub async fn createDatabase(name: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.createDatabase(name).await
    }

    pub async fn dropDatabaseIfExists(name: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.dropDatabaseIfExists(name).await
    }

    pub async fn create(table: &str, configure: impl FnOnce(&mut Blueprint)) -> DatabaseResult<()> {
        Self::default_builder()?.create(table, configure).await
    }

    pub async fn table(table: &str, configure: impl FnOnce(&mut Blueprint)) -> DatabaseResult<()> {
        Self::default_builder()?.table(table, configure).await
    }

    pub async fn dropAllTables() -> DatabaseResult<()> {
        Self::default_builder()?.dropAllTables().await
    }

    pub async fn dropAllViews() -> DatabaseResult<()> {
        Self::default_builder()?.dropAllViews().await
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

    pub async fn getSchemas() -> DatabaseResult<Vec<SchemaInfo>> {
        Self::default_builder()?.getSchemas().await
    }

    pub async fn getViews(schema: Option<&str>) -> DatabaseResult<Vec<ViewInfo>> {
        Self::default_builder()?.getViews(schema).await
    }

    pub async fn getViewsIn(schemas: &[&str]) -> DatabaseResult<Vec<ViewInfo>> {
        Self::default_builder()?.getViewsIn(schemas).await
    }

    pub async fn hasView(view: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.hasView(view).await
    }

    pub async fn hasTable(table: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.hasTable(table).await
    }

    pub async fn getTables(schema: Option<&str>) -> DatabaseResult<Vec<TableInfo>> {
        Self::default_builder()?.getTables(schema).await
    }

    pub async fn getTablesIn(schemas: &[&str]) -> DatabaseResult<Vec<TableInfo>> {
        Self::default_builder()?.getTablesIn(schemas).await
    }

    pub async fn getTableListing(schema: Option<&str>) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?.getTableListing(schema).await
    }

    pub async fn getTableListingWith(
        schema: Option<&str>,
        schema_qualified: bool,
    ) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?
            .getTableListingWith(schema, schema_qualified)
            .await
    }

    pub async fn hasColumn(table: &str, column: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.hasColumn(table, column).await
    }

    pub async fn hasColumns(table: &str, columns: &[&str]) -> DatabaseResult<bool> {
        Self::default_builder()?.hasColumns(table, columns).await
    }

    pub async fn whenTableHasColumn(
        table: &str,
        column: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .whenTableHasColumn(table, column, configure)
            .await
    }

    pub async fn whenTableDoesntHaveColumn(
        table: &str,
        column: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .whenTableDoesntHaveColumn(table, column, configure)
            .await
    }

    pub async fn whenTableHasIndex(
        table: &str,
        index: KeySelector<'_>,
        configure: impl FnOnce(&mut Blueprint),
        index_type: Option<&str>,
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .whenTableHasIndex(table, index, configure, index_type)
            .await
    }

    pub async fn whenTableDoesntHaveIndex(
        table: &str,
        index: KeySelector<'_>,
        configure: impl FnOnce(&mut Blueprint),
        index_type: Option<&str>,
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .whenTableDoesntHaveIndex(table, index, configure, index_type)
            .await
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

    pub async fn hasIndexWith(
        table: &str,
        selector: KeySelector<'_>,
        index_type: Option<&str>,
    ) -> DatabaseResult<bool> {
        Self::default_builder()?
            .hasIndexWith(table, selector, index_type)
            .await
    }

    pub async fn hasForeignKey(table: &str, selector: KeySelector<'_>) -> DatabaseResult<bool> {
        Self::default_builder()?
            .hasForeignKey(table, selector)
            .await
    }

    pub async fn getForeignKeys(table: &str) -> DatabaseResult<Vec<ForeignKey>> {
        Self::default_builder()?.getForeignKeys(table).await
    }
}

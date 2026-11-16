use crate::{
    database::{DB, DatabaseError, DatabaseFuture, DatabaseResult},
    support::facades::Facade,
};

use super::{
    Blueprint, Builder, Column, ForeignKey, Index, MorphKeyType, SchemaInfo, SchemaSession,
    TableInfo, ViewInfo,
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

use crate::{
    database::{DatabaseError, DatabaseFuture, DatabaseResult, Db},
    support::facades::Facade,
};

use super::{
    Blueprint, Builder, Column, ForeignKey, Index, KeySelector, MorphKeyType, SchemaFallback,
    SchemaInfo, SchemaSession, TableInfo, TypeInfo, ViewInfo,
};

/// Schema operations on the current application's default database connection.
pub struct Schema;

impl Schema {
    /// Sets the default for subsequent polymorphic columns in this application.
    pub fn default_morph_key_type(kind: MorphKeyType) -> DatabaseResult<()> {
        Db::get_facade_root()?.set_morph_key_type(kind);
        Ok(())
    }

    /// Sets the default length of subsequent string and char columns in this application.
    pub fn default_string_length(length: u32) -> DatabaseResult<()> {
        if !(1..=65_535).contains(&length) {
            return Err(DatabaseError::InvalidSchema("default string length"));
        }
        Db::get_facade_root()?.update_schema_defaults(|defaults| defaults.string_length = length);
        Ok(())
    }

    /// Sets the default fractional seconds precision for subsequent schema columns.
    pub fn default_time_precision(precision: Option<u8>) -> DatabaseResult<()> {
        let precision = precision.unwrap_or(0);
        if precision > 6 {
            return Err(DatabaseError::InvalidSchema("time precision"));
        }
        Db::get_facade_root()?
            .update_schema_defaults(|defaults| defaults.time_precision = precision);
        Ok(())
    }

    pub fn morph_using_uuids() -> DatabaseResult<()> {
        Self::default_morph_key_type(MorphKeyType::Uuid)
    }

    pub fn morph_using_ulids() -> DatabaseResult<()> {
        Self::default_morph_key_type(MorphKeyType::Ulid)
    }

    pub fn connection(name: &str) -> DatabaseResult<Builder<'static>> {
        Ok(Db::connection(Some(name))?.get_schema_builder())
    }

    fn default_builder() -> DatabaseResult<Builder<'static>> {
        Ok(Db::connection(None)?.get_schema_builder())
    }

    pub async fn session() -> DatabaseResult<SchemaSession> {
        Self::default_builder()?.session().await
    }

    pub async fn without_foreign_key_constraints<T>(
        callback: impl for<'a> FnOnce(&'a mut SchemaSession) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
    ) -> DatabaseResult<T> {
        Self::default_builder()?
            .without_foreign_key_constraints(callback)
            .await
    }

    pub async fn create_database(name: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.create_database(name).await
    }

    pub async fn ensure_vector_extension_exists(schema: Option<&str>) -> DatabaseResult<()> {
        Self::default_builder()?
            .ensure_vector_extension_exists(schema)
            .await
    }

    pub async fn ensure_extension_exists(name: &str, schema: Option<&str>) -> DatabaseResult<()> {
        Self::default_builder()?
            .ensure_extension_exists(name, schema)
            .await
    }

    pub async fn drop_database_if_exists(name: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.drop_database_if_exists(name).await
    }

    pub async fn create<R: super::BlueprintCallbackResult>(
        table: &str,
        configure: impl FnOnce(&mut Blueprint) -> R,
    ) -> DatabaseResult<()> {
        Self::default_builder()?.create(table, configure).await
    }

    pub async fn table<R: super::BlueprintCallbackResult>(
        table: &str,
        configure: impl FnOnce(&mut Blueprint) -> R,
    ) -> DatabaseResult<()> {
        Self::default_builder()?.table(table, configure).await
    }

    pub async fn drop_all_tables() -> DatabaseResult<()> {
        Self::default_builder()?.drop_all_tables().await
    }

    pub async fn drop_all_views() -> DatabaseResult<()> {
        Self::default_builder()?.drop_all_views().await
    }

    pub async fn drop_all_types() -> DatabaseResult<()> {
        Self::default_builder()?.drop_all_types().await
    }

    pub async fn drop_columns(table: &str, columns: &[&str]) -> DatabaseResult<()> {
        Self::default_builder()?.drop_columns(table, columns).await
    }

    pub async fn drop(table: &str) -> DatabaseResult<()> {
        Self::default_builder()?.drop(table).await
    }

    pub async fn drop_if_exists(table: &str) -> DatabaseResult<()> {
        Self::default_builder()?.drop_if_exists(table).await
    }

    pub async fn rename(from: &str, to: &str) -> DatabaseResult<()> {
        Self::default_builder()?.rename(from, to).await
    }

    /// Returns the default schema of the application's write connection.
    pub fn get_current_schema_name() -> DatabaseResult<String> {
        Ok(Self::default_builder()?
            .get_current_schema_name()
            .to_owned())
    }

    /// Returns the default connection's schema lookup list.
    pub fn get_current_schema_listing() -> DatabaseResult<Vec<String>> {
        Ok(Self::default_builder()?.get_current_schema_listing())
    }

    /// Splits a logical table reference before the connection's table prefix is applied.
    pub fn parse_schema_and_table(
        reference: &str,
        fallback: SchemaFallback<'_>,
    ) -> DatabaseResult<(Option<String>, String)> {
        let builder = Self::default_builder()?;
        let (schema, table) = builder.parse_schema_and_table(reference, fallback)?;
        Ok((schema.map(str::to_owned), table.to_owned()))
    }

    pub async fn get_schemas() -> DatabaseResult<Vec<SchemaInfo>> {
        Self::default_builder()?.get_schemas().await
    }

    pub async fn get_types(schema: Option<&str>) -> DatabaseResult<Vec<TypeInfo>> {
        Self::default_builder()?.get_types(schema).await
    }

    pub async fn get_types_in(schemas: &[&str]) -> DatabaseResult<Vec<TypeInfo>> {
        Self::default_builder()?.get_types_in(schemas).await
    }

    pub async fn get_views(schema: Option<&str>) -> DatabaseResult<Vec<ViewInfo>> {
        Self::default_builder()?.get_views(schema).await
    }

    pub async fn get_views_in(schemas: &[&str]) -> DatabaseResult<Vec<ViewInfo>> {
        Self::default_builder()?.get_views_in(schemas).await
    }

    pub async fn has_view(view: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.has_view(view).await
    }

    pub async fn has_table(table: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.has_table(table).await
    }

    pub async fn get_tables(schema: Option<&str>) -> DatabaseResult<Vec<TableInfo>> {
        Self::default_builder()?.get_tables(schema).await
    }

    pub async fn get_tables_in(schemas: &[&str]) -> DatabaseResult<Vec<TableInfo>> {
        Self::default_builder()?.get_tables_in(schemas).await
    }

    pub async fn get_table_listing(schema: Option<&str>) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?.get_table_listing(schema).await
    }

    pub async fn get_table_listing_with(
        schema: Option<&str>,
        schema_qualified: bool,
    ) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?
            .get_table_listing_with(schema, schema_qualified)
            .await
    }

    pub async fn has_column(table: &str, column: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.has_column(table, column).await
    }

    pub async fn has_columns(table: &str, columns: &[&str]) -> DatabaseResult<bool> {
        Self::default_builder()?.has_columns(table, columns).await
    }

    pub async fn when_table_has_column(
        table: &str,
        column: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .when_table_has_column(table, column, configure)
            .await
    }

    pub async fn when_table_doesnt_have_column(
        table: &str,
        column: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .when_table_doesnt_have_column(table, column, configure)
            .await
    }

    pub async fn when_table_has_index(
        table: &str,
        index: KeySelector<'_>,
        configure: impl FnOnce(&mut Blueprint),
        index_type: Option<&str>,
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .when_table_has_index(table, index, configure, index_type)
            .await
    }

    pub async fn when_table_doesnt_have_index(
        table: &str,
        index: KeySelector<'_>,
        configure: impl FnOnce(&mut Blueprint),
        index_type: Option<&str>,
    ) -> DatabaseResult<()> {
        Self::default_builder()?
            .when_table_doesnt_have_index(table, index, configure, index_type)
            .await
    }

    /// Returns the metadata for a case-insensitive column name, if present.
    pub async fn get_column(table: &str, name: &str) -> DatabaseResult<Option<Column>> {
        Self::default_builder()?.get_column(table, name).await
    }

    pub async fn get_columns(table: &str) -> DatabaseResult<Vec<Column>> {
        Self::default_builder()?.get_columns(table).await
    }

    pub async fn get_column_listing(table: &str) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?.get_column_listing(table).await
    }

    pub async fn get_column_type(table: &str, column: &str, full: bool) -> DatabaseResult<String> {
        Self::default_builder()?
            .get_column_type(table, column, full)
            .await
    }

    pub async fn get_indexes(table: &str) -> DatabaseResult<Vec<Index>> {
        Self::default_builder()?.get_indexes(table).await
    }

    pub async fn get_index_listing(table: &str) -> DatabaseResult<Vec<String>> {
        Self::default_builder()?.get_index_listing(table).await
    }

    pub async fn has_index(table: &str, name: &str) -> DatabaseResult<bool> {
        Self::default_builder()?.has_index(table, name).await
    }

    pub async fn has_index_with(
        table: &str,
        selector: KeySelector<'_>,
        index_type: Option<&str>,
    ) -> DatabaseResult<bool> {
        Self::default_builder()?
            .has_index_with(table, selector, index_type)
            .await
    }

    pub async fn has_foreign_key(table: &str, selector: KeySelector<'_>) -> DatabaseResult<bool> {
        Self::default_builder()?
            .has_foreign_key(table, selector)
            .await
    }

    pub async fn get_foreign_keys(table: &str) -> DatabaseResult<Vec<ForeignKey>> {
        Self::default_builder()?.get_foreign_keys(table).await
    }
}

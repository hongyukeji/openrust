use std::collections::BTreeMap;

use crate::database::{DatabaseError, DatabaseResult, Row, Value};

use super::{Builder, Executor, postgres};
use crate::database::schema::{
    Column, ColumnGeneration, ForeignKey, GenerationType, Index, KeySelector, SchemaFallback,
    SchemaInfo, TableInfo, TypeInfo, ViewInfo,
    grammars::{Grammar, Table},
};

const MYSQL_SYSTEM_SCHEMAS: &str =
    "('information_schema', 'mysql', 'ndbinfo', 'performance_schema', 'sys')";

#[allow(non_snake_case, reason = "Matches Laravel's Schema Builder API")]
impl Builder<'_> {
    fn catalog_table(&self, name: &str) -> DatabaseResult<Table> {
        let table = self.resolve_table(name)?;
        if self.grammar.mysql()
            && let Executor::Session(session) = &self.executor
            && session.temporary_index(&table).is_some()
        {
            return Err(DatabaseError::UnsupportedSchema(
                "MySQL temporary table metadata",
            ));
        }
        Ok(table)
    }

    fn catalog_parameters(&self, table: &Table) -> [Value; 2] {
        [
            self.grammar.catalog_schema(table).into(),
            table.name.as_str().into(),
        ]
    }

    /// Returns the default schema of the configured write connection.
    pub fn getCurrentSchemaName(&self) -> &str {
        self.grammar.default_schema()
    }

    /// Returns the connection's default schema lookup list.
    pub fn getCurrentSchemaListing(&self) -> Vec<String> {
        if self.grammar.postgres() {
            return self.grammar.postgres_schemas().to_vec();
        }
        vec![self.getCurrentSchemaName().to_owned()]
    }

    /// Splits a logical table reference before the connection's table prefix is applied.
    pub fn parseSchemaAndTable<'a>(
        &'a self,
        reference: &'a str,
        fallback: SchemaFallback<'a>,
    ) -> DatabaseResult<(Option<&'a str>, &'a str)> {
        let (schema, table) = Grammar::parse_schema_and_table(reference)?;
        let schema = schema.or_else(|| match fallback {
            SchemaFallback::None => None,
            SchemaFallback::Current => Some(self.getCurrentSchemaName()),
            SchemaFallback::Named(schema) => Some(schema),
        });
        Ok((schema, table))
    }

    pub async fn getSchemas(&mut self) -> DatabaseResult<Vec<SchemaInfo>> {
        let mysql_sql;
        let sql = if self.grammar.mysql() {
            mysql_sql = format!(
                "select schema_name as name, null as path, schema_name = database() as is_default from information_schema.schemata where schema_name not in {MYSQL_SYSTEM_SCHEMAS} order by schema_name"
            );
            &mysql_sql
        } else if self.grammar.postgres() {
            "select nspname::text as name, null::text as path, coalesce(nspname = pg_catalog.current_schema(), false) as is_default from pg_catalog.pg_namespace where nspname !~ '^pg_' and nspname <> 'information_schema' order by nspname"
        } else if self.grammar.sql_server() {
            "select name, cast(null as nvarchar(255)) as path, cast(case when name=schema_name() then 1 else 0 end as bit) as is_default from sys.schemas where principal_id < 16384 order by name"
        } else {
            "select name, file as path, name = 'main' as is_default from pragma_database_list order by name"
        };
        self.select(sql, &[])
            .await?
            .iter()
            .map(|row| {
                Ok(SchemaInfo {
                    name: text(row, "name")?,
                    path: optional_text(row, "path")?,
                    default: flag(row, "is_default")?,
                })
            })
            .collect()
    }

    /// Lists PostgreSQL types in one schema or across all user schemas.
    pub async fn getTypes(&mut self, schema: Option<&str>) -> DatabaseResult<Vec<TypeInfo>> {
        self.catalog_types(schema.as_ref().map(std::slice::from_ref))
            .await
    }

    pub async fn getTypesIn(&mut self, schemas: &[&str]) -> DatabaseResult<Vec<TypeInfo>> {
        self.catalog_types(Some(schemas)).await
    }

    async fn catalog_types(&mut self, schemas: Option<&[&str]>) -> DatabaseResult<Vec<TypeInfo>> {
        if !self.grammar.postgres() {
            return Err(DatabaseError::UnsupportedSchema(
                "type catalog requires PostgreSQL",
            ));
        }
        if schemas.is_some_and(<[&str]>::is_empty) {
            return Ok(Vec::new());
        }
        let (filter, bindings) = postgres_schema_filter("schema", schemas);
        let sql = format!(
            "select * from ({}) as type_catalog where {filter} order by schema, name",
            postgres::TYPES
        );
        self.select(&sql, &bindings)
            .await?
            .iter()
            .map(type_info)
            .collect()
    }

    pub async fn getViews(&mut self, schema: Option<&str>) -> DatabaseResult<Vec<ViewInfo>> {
        match schema {
            Some(schema) => self.getViewsIn(&[schema]).await,
            None if self.grammar.mysql() => self.mysql_views(None).await,
            None if self.grammar.postgres() => self.postgres_views(None).await,
            None if self.grammar.sql_server() => self.sql_server_views(None).await,
            None if matches!(self.executor, Executor::Connection(_)) => {
                let mut session = self.session().await?;
                let result = session.getSchemaBuilder().all_sqlite_views().await;
                let cleanup = session.close().await;
                let views = result?;
                cleanup?;
                Ok(views)
            }
            None => self.all_sqlite_views().await,
        }
    }

    pub async fn getViewsIn(&mut self, schemas: &[&str]) -> DatabaseResult<Vec<ViewInfo>> {
        if schemas.is_empty() {
            return Ok(Vec::new());
        }
        if self.grammar.mysql() {
            return self.mysql_views(Some(schemas)).await;
        }
        if self.grammar.postgres() {
            return self.postgres_views(Some(schemas)).await;
        }
        if self.grammar.sql_server() {
            return self.sql_server_views(Some(schemas)).await;
        }
        if schemas.len() > 1 && matches!(self.executor, Executor::Connection(_)) {
            let mut session = self.session().await?;
            let result = session.getSchemaBuilder().sqlite_views(schemas).await;
            let cleanup = session.close().await;
            let views = result?;
            cleanup?;
            return Ok(views);
        }
        self.sqlite_views(schemas).await
    }

    pub async fn hasView(&mut self, view: &str) -> DatabaseResult<bool> {
        let view = self.resolve_table(view)?;
        if self.grammar.postgres() {
            let schema = view.schema.as_deref().unwrap_or_default();
            let rows = self.select(
                "select 1 from pg_catalog.pg_views where viewname::text = ? and (schemaname::text = ? or (? = '' and schemaname = any(pg_catalog.current_schemas(false)))) limit 1",
                &[view.name.into(), schema.into(), schema.into()],
            ).await?;
            return Ok(!rows.is_empty());
        }
        let schema = self.grammar.catalog_schema(&view).to_owned();
        Ok(self
            .getViews(Some(&schema))
            .await?
            .iter()
            .any(|item| item.name.eq_ignore_ascii_case(&view.name)))
    }

    async fn mysql_views(&mut self, schemas: Option<&[&str]>) -> DatabaseResult<Vec<ViewInfo>> {
        let (filter, bindings) = match schemas {
            Some(schemas) => schema_filter("table_schema", schemas),
            None => (
                format!("table_schema not in {MYSQL_SYSTEM_SCHEMAS}"),
                Vec::new(),
            ),
        };
        let sql = format!(
            "select table_name as name, table_schema as schema_name, view_definition as definition from information_schema.views where {filter} order by table_schema, table_name"
        );
        self.select(&sql, &bindings)
            .await?
            .iter()
            .map(view_info)
            .collect()
    }

    async fn all_sqlite_views(&mut self) -> DatabaseResult<Vec<ViewInfo>> {
        let schemas = self.getSchemas().await?;
        let names: Vec<_> = schemas.iter().map(|schema| schema.name.as_str()).collect();
        self.sqlite_views(&names).await
    }

    async fn postgres_views(&mut self, schemas: Option<&[&str]>) -> DatabaseResult<Vec<ViewInfo>> {
        let (filter, bindings) = postgres_schema_filter("schemaname", schemas);
        let sql = format!(
            "select viewname::text as name, schemaname::text as schema_name, definition from pg_catalog.pg_views where {filter} order by schemaname, viewname"
        );
        self.select(&sql, &bindings)
            .await?
            .iter()
            .map(view_info)
            .collect()
    }

    async fn sql_server_views(
        &mut self,
        schemas: Option<&[&str]>,
    ) -> DatabaseResult<Vec<ViewInfo>> {
        let (filter, bindings) = schemas.map_or_else(
            || ("1=1".to_owned(), Vec::new()),
            |schemas| schema_filter("s.name", schemas),
        );
        let sql = format!(
            "select v.name, s.name as schema_name, object_definition(v.object_id) as definition from sys.views v join sys.schemas s on s.schema_id=v.schema_id where v.is_ms_shipped=0 and {filter} order by s.name,v.name"
        );
        self.select(&sql, &bindings)
            .await?
            .iter()
            .map(view_info)
            .collect()
    }

    async fn sqlite_views(&mut self, schemas: &[&str]) -> DatabaseResult<Vec<ViewInfo>> {
        let mut views = Vec::new();
        for schema in schemas {
            let catalog = self.grammar.identifier(schema)?;
            let sql = format!(
                "select name, ? as schema_name, sql as definition from {catalog}.sqlite_schema where type = 'view' order by name"
            );
            let rows = self.select(&sql, &[(*schema).into()]).await?;
            for row in rows {
                views.push(view_info(&row)?);
            }
        }
        Ok(views)
    }

    pub async fn hasTable(&mut self, table: &str) -> DatabaseResult<bool> {
        let table = self.resolve_table(table)?;
        if let Executor::Session(session) = &self.executor
            && session.temporary_index(&table).is_some()
        {
            return Ok(true);
        }
        if self.grammar.postgres() {
            let sql = "select 1 from information_schema.tables where (table_schema = ? or (? = '' and table_schema = any(current_schemas(false)))) and table_name = ? and table_type = 'BASE TABLE' limit 1";
            let schema = table.schema.as_deref().unwrap_or_default();
            return Ok(!self
                .select(
                    sql,
                    &[schema.into(), schema.into(), table.name.as_str().into()],
                )
                .await?
                .is_empty());
        }
        let parameters = self.catalog_parameters(&table);
        let sql = if self.grammar.mysql() {
            "select 1 from information_schema.tables where table_schema = coalesce(nullif(?, ''), database()) and table_name = ? and table_type in ('BASE TABLE', 'SYSTEM VERSIONED') limit 1"
        } else if self.grammar.sql_server() {
            "select top (1) 1 from information_schema.tables where table_schema = ? and table_name = ? and table_type = 'BASE TABLE'"
        } else {
            "select 1 from pragma_table_list where schema = ? and name = ? collate nocase and type in ('table', 'virtual') limit 1"
        };
        Ok(!self.select(sql, &parameters).await?.is_empty())
    }

    pub async fn getTables(&mut self, schema: Option<&str>) -> DatabaseResult<Vec<TableInfo>> {
        match schema {
            Some(schema) => self.getTablesIn(&[schema]).await,
            None => self.catalog_tables(None).await,
        }
    }

    pub async fn getTablesIn(&mut self, schemas: &[&str]) -> DatabaseResult<Vec<TableInfo>> {
        if schemas.is_empty() {
            return Ok(Vec::new());
        }
        self.catalog_tables(Some(schemas)).await
    }

    pub async fn getTableListing(&mut self, schema: Option<&str>) -> DatabaseResult<Vec<String>> {
        self.getTableListingWith(schema, true).await
    }

    pub async fn getTableListingWith(
        &mut self,
        schema: Option<&str>,
        schema_qualified: bool,
    ) -> DatabaseResult<Vec<String>> {
        Ok(self
            .getTables(schema)
            .await?
            .into_iter()
            .map(|table| {
                if schema_qualified {
                    table.schema_qualified_name
                } else {
                    table.name
                }
            })
            .collect())
    }

    async fn catalog_tables(&mut self, schemas: Option<&[&str]>) -> DatabaseResult<Vec<TableInfo>> {
        let (sql, bindings) = if self.grammar.mysql() {
            let (filter, bindings) = match schemas {
                Some(schemas) => schema_filter("table_schema", schemas),
                None => (
                    format!("table_schema not in {MYSQL_SYSTEM_SCHEMAS}"),
                    Vec::new(),
                ),
            };
            (
                format!(
                    "select table_name as name, table_schema as schema_name, cast(data_length + index_length as unsigned) as size, engine as engine, table_collation as collation, table_comment as comment from information_schema.tables where table_type in ('BASE TABLE', 'SYSTEM VERSIONED') and {filter} order by table_schema, table_name"
                ),
                bindings,
            )
        } else if self.grammar.postgres() {
            let (filter, bindings) = postgres_schema_filter("n.nspname", schemas);
            (
                format!(
                    "select c.relname::text as name, n.nspname::text as schema_name, pg_catalog.pg_total_relation_size(c.oid) as size, null::text as engine, null::text as collation, pg_catalog.obj_description(c.oid, 'pg_class') as comment from pg_catalog.pg_class c join pg_catalog.pg_namespace n on n.oid = c.relnamespace where c.relkind in ('r','p') and {filter} order by n.nspname, c.relname"
                ),
                bindings,
            )
        } else if self.grammar.sql_server() {
            let (filter, bindings) = schemas.map_or_else(
                || ("1=1".to_owned(), Vec::new()),
                |schemas| schema_filter("s.name", schemas),
            );
            (
                format!(
                    "select t.name, s.name as schema_name, cast(null as bigint) as size, cast(null as nvarchar(255)) as engine, cast(null as nvarchar(255)) as collation, cast(null as nvarchar(255)) as comment from sys.tables t join sys.schemas s on s.schema_id=t.schema_id where t.is_ms_shipped=0 and {filter} order by s.name,t.name"
                ),
                bindings,
            )
        } else {
            let (filter, bindings) = schemas.map_or_else(
                || ("1 = 1".to_owned(), Vec::new()),
                |schemas| schema_filter("schema", schemas),
            );
            (
                format!(
                    "select name, schema as schema_name, null as size, null as engine, null as collation, null as comment from pragma_table_list where type in ('table', 'virtual') and substr(name, 1, 7) <> 'sqlite_' and {filter} order by schema, name"
                ),
                bindings,
            )
        };
        self.select(&sql, &bindings)
            .await?
            .iter()
            .map(|row| {
                let name = text(row, "name")?;
                let schema = text(row, "schema_name")?;
                let size = match row.get("size") {
                    Some(Value::Null) => None,
                    Some(Value::Unsigned(value)) => Some(*value),
                    Some(Value::Integer(value)) => Some(
                        u64::try_from(*value)
                            .map_err(|_| DatabaseError::InvalidSchema("table size"))?,
                    ),
                    _ => return Err(DatabaseError::InvalidSchema("table size")),
                };
                Ok(TableInfo {
                    schema_qualified_name: format!("{schema}.{name}"),
                    name,
                    schema,
                    size,
                    engine: optional_text(row, "engine")?,
                    collation: optional_text(row, "collation")?,
                    comment: optional_text(row, "comment")?,
                })
            })
            .collect()
    }

    pub async fn getColumns(&mut self, table: &str) -> DatabaseResult<Vec<Column>> {
        let table = self.catalog_table(table)?;
        let parameters = self.catalog_parameters(&table);
        let (sql, parameters) = if self.grammar.mysql() {
            (
                "select column_name as name, data_type as type_name, column_type as full_type, is_nullable = 'YES' as nullable, column_default as default_value, extra like '%auto_increment%' as auto_increment, column_key = 'PRI' as primary_key, column_comment as comment, collation_name as collation, case when extra like '%VIRTUAL GENERATED%' then 'virtual' when extra like '%STORED GENERATED%' then 'stored' end as generation_kind, generation_expression as generation_expression from information_schema.columns where table_schema = coalesce(nullif(?, ''), database()) and table_name = ? order by ordinal_position",
                parameters,
            )
        } else if self.grammar.postgres() {
            (postgres::COLUMNS, optional_schema_parameters(&table))
        } else if self.grammar.sql_server() {
            (
                "select c.name, ty.name as type_name, ty.name as full_type, c.is_nullable as nullable, d.definition as default_value, c.is_identity as auto_increment, cast(case when exists (select 1 from sys.index_columns ic join sys.indexes i on i.object_id=ic.object_id and i.index_id=ic.index_id where ic.object_id=c.object_id and ic.column_id=c.column_id and i.is_primary_key=1) then 1 else 0 end as bit) as primary_key, cast(null as nvarchar(255)) as comment, c.collation_name as collation, case when computed.is_persisted=1 then 'stored' when c.is_computed=1 then 'virtual' end as generation_kind, computed.definition as generation_expression from sys.columns c join sys.types ty on ty.user_type_id=c.user_type_id join sys.tables t on t.object_id=c.object_id join sys.schemas s on s.schema_id=t.schema_id left join sys.default_constraints d on d.object_id=c.default_object_id left join sys.computed_columns computed on computed.object_id=c.object_id and computed.column_id=c.column_id where s.name=? and t.name=? order by c.column_id",
                parameters,
            )
        } else {
            let [schema, name] = parameters;
            (
                r#"with columns as (
                       select *, pk > 0 and lower(type) = 'integer'
                           and not exists (select 1 from pragma_index_list(?1, ?2) where origin = 'pk')
                           as rowid_alias
                       from pragma_table_xinfo(?1, ?2)
                   )
                   select name, lower(type) as full_type,
                          lower(substr(type, 1, instr(type || '(', '(') - 1)) as type_name,
                          not "notnull" and not rowid_alias as nullable, dflt_value as default_value,
                          rowid_alias as auto_increment, pk > 0 as primary_key,
                          null as comment, null as collation,
                          case hidden when 2 then 'virtual' when 3 then 'stored' end as generation_kind,
                          null as generation_expression
                   from columns order by cid"#,
                [name, schema],
            )
        };
        let mut columns = self
            .select(sql, &parameters)
            .await?
            .iter()
            .map(|row| {
                let generation = match optional_text(row, "generation_kind")?.as_deref() {
                    Some("virtual") => Some(GenerationType::Virtual),
                    Some("stored") => Some(GenerationType::Stored),
                    _ => None,
                };
                let generation = generation
                    .map(|kind| -> DatabaseResult<ColumnGeneration> {
                        Ok(ColumnGeneration {
                            kind,
                            expression: optional_text(row, "generation_expression")?
                                .unwrap_or_default(),
                        })
                    })
                    .transpose()?;
                Ok(Column {
                    name: text(row, "name")?,
                    type_name: text(row, "type_name")?,
                    full_type: text(row, "full_type")?,
                    nullable: flag(row, "nullable")?,
                    default: optional_text(row, "default_value")?,
                    auto_increment: flag(row, "auto_increment")?,
                    primary: flag(row, "primary_key")?,
                    comment: optional_text(row, "comment")?,
                    generation,
                    collation: optional_text(row, "collation")?,
                })
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        let generated = columns.iter().any(|column| column.generation.is_some());
        if !self.grammar.mysql()
            && !self.grammar.sql_server()
            && !self.grammar.postgres()
            && !columns.is_empty()
        {
            let schema = self
                .grammar
                .identifier(self.grammar.catalog_schema(&table))?;
            let sql = format!(
                "select sql from {schema}.sqlite_schema where type = 'table' and name = ? collate nocase"
            );
            let rows = self.select(&sql, &[table.name.into()]).await?;
            if let Some(Value::Text(sql)) = rows.first().and_then(|row| row.get("sql")) {
                let mut collations = super::sqlite::column_collations(sql)?
                    .into_iter()
                    .collect::<BTreeMap<_, _>>();
                if !collations.is_empty() {
                    for column in &mut columns {
                        column.collation = collations.remove(&column.name);
                    }
                }
                if generated {
                    let mut expressions =
                        super::sqlite::generated_expressions(sql)?.collect::<BTreeMap<_, _>>();
                    for column in &mut columns {
                        if let Some(generation) = &mut column.generation {
                            generation.expression = expressions.remove(&column.name).ok_or(
                                DatabaseError::InvalidSchema("SQLite generated column expression"),
                            )?;
                        }
                    }
                }
            }
        }
        Ok(columns)
    }

    pub async fn getColumnListing(&mut self, table: &str) -> DatabaseResult<Vec<String>> {
        Ok(self
            .getColumns(table)
            .await?
            .into_iter()
            .map(|column| column.name)
            .collect())
    }

    pub async fn hasColumn(&mut self, table: &str, column: &str) -> DatabaseResult<bool> {
        self.hasColumns(table, &[column]).await
    }

    pub async fn hasColumns(&mut self, table: &str, columns: &[&str]) -> DatabaseResult<bool> {
        let existing = self.getColumnListing(table).await?;
        Ok(columns.iter().all(|column| {
            existing
                .iter()
                .any(|name| name.eq_ignore_ascii_case(column))
        }))
    }

    /// Returns the metadata for a case-insensitive column name, if present.
    pub async fn getColumn(&mut self, table: &str, name: &str) -> DatabaseResult<Option<Column>> {
        Ok(self
            .getColumns(table)
            .await?
            .into_iter()
            .find(|column| column.name.eq_ignore_ascii_case(name)))
    }

    pub async fn getColumnType(
        &mut self,
        table: &str,
        name: &str,
        full: bool,
    ) -> DatabaseResult<String> {
        let column = self
            .getColumn(table, name)
            .await?
            .ok_or(DatabaseError::InvalidSchema("unknown column"))?;
        Ok(if full {
            column.full_type
        } else {
            column.type_name
        })
    }

    pub async fn getIndexes(&mut self, table: &str) -> DatabaseResult<Vec<Index>> {
        let identifier = self.catalog_table(table)?;
        let parameters = self.catalog_parameters(&identifier);
        let (sql, parameters) = if self.grammar.mysql() {
            (
                "select index_name as name, column_name as column_name, lower(index_type) as index_type, not non_unique as unique_key, index_name = 'PRIMARY' as primary_key from information_schema.statistics where table_schema = coalesce(nullif(?, ''), database()) and table_name = ? order by index_name, seq_in_index",
                parameters.to_vec(),
            )
        } else if self.grammar.postgres() {
            (
                "select ic.relname as name, a.attname as column_name, am.amname as index_type, \
                 i.indisunique as unique_key, i.indisprimary as primary_key \
                 from pg_index i join pg_class tc on tc.oid = i.indrelid \
                 join pg_namespace tn on tn.oid = tc.relnamespace \
                 join pg_class ic on ic.oid = i.indexrelid join pg_am am on am.oid = ic.relam \
                 join lateral unnest(i.indkey) with ordinality as keys(num, ord) on true \
                 left join pg_attribute a on a.attrelid = tc.oid and a.attnum = keys.num \
                 where tn.nspname = coalesce(nullif(?, ''), current_schema()) and tc.relname = ? \
                 order by ic.relname, keys.ord",
                optional_schema_parameters(&identifier).to_vec(),
            )
        } else if self.grammar.sql_server() {
            (
                "select i.name, c.name as column_name, lower(i.type_desc) as index_type, \
                 i.is_unique as unique_key, i.is_primary_key as primary_key \
                 from sys.indexes i join sys.tables t on t.object_id = i.object_id \
                 join sys.schemas s on s.schema_id = t.schema_id \
                 join sys.index_columns ic on ic.object_id = i.object_id and ic.index_id = i.index_id \
                 join sys.columns c on c.object_id = ic.object_id and c.column_id = ic.column_id \
                 where s.name = coalesce(nullif(?, ''), schema_name()) and t.name = ? and i.name is not null \
                 order by i.name, ic.key_ordinal, ic.index_column_id",
                optional_schema_parameters(&identifier).to_vec(),
            )
        } else {
            let [schema, name] = parameters;
            (
                r#"with indexes as (select name, "unique", origin from pragma_index_list(?1, ?2))
                   select i.name as name, c.name as column_name, null as index_type,
                          i."unique" as unique_key, i.origin = 'pk' as primary_key, c.seqno as column_order
                   from indexes i join pragma_index_info(i.name, ?2) c
                   union all
                   select 'primary', name, null, 1, 1, pk
                   from pragma_table_xinfo(?1, ?2)
                   where pk > 0 and not exists (select 1 from indexes where origin = 'pk')
                   order by name, primary_key, column_order"#,
                vec![name, schema],
            )
        };
        let mut indexes = BTreeMap::<(String, bool), Index>::new();
        for row in self.select(sql, &parameters).await? {
            let name = text(&row, "name")?;
            let primary = flag(&row, "primary_key")?;
            let name = if primary && self.grammar.mysql() {
                "primary".to_owned()
            } else {
                name
            };
            let index = indexes.entry((name.clone(), primary)).or_insert(Index {
                name: if self.grammar.sql_server() {
                    name.to_ascii_lowercase()
                } else {
                    name
                },
                columns: Vec::new(),
                index_type: optional_text(&row, "index_type")?,
                unique: flag(&row, "unique_key")?,
                primary,
            });
            index.columns.push(optional_text(&row, "column_name")?);
        }
        Ok(indexes.into_values().collect())
    }

    pub async fn getIndexListing(&mut self, table: &str) -> DatabaseResult<Vec<String>> {
        Ok(self
            .getIndexes(table)
            .await?
            .into_iter()
            .map(|index| index.name)
            .collect())
    }

    pub async fn hasIndex(&mut self, table: &str, name: &str) -> DatabaseResult<bool> {
        self.hasIndexWith(table, KeySelector::Name(name), None)
            .await
    }

    pub async fn hasIndexWith(
        &mut self,
        table: &str,
        selector: KeySelector<'_>,
        index_type: Option<&str>,
    ) -> DatabaseResult<bool> {
        Ok(self.getIndexes(table).await?.iter().any(|index| {
            let matches_type = index_type.is_none_or(|kind| {
                (kind.eq_ignore_ascii_case("primary") && index.primary)
                    || (kind.eq_ignore_ascii_case("unique") && index.unique)
                    || index
                        .index_type
                        .as_deref()
                        .is_some_and(|actual| actual.eq_ignore_ascii_case(kind))
            });
            matches_type
                && match selector {
                    KeySelector::Name(name) => {
                        index.name.eq_ignore_ascii_case(name)
                            || (index.primary && name.eq_ignore_ascii_case("primary"))
                    }
                    KeySelector::Columns(columns) => index
                        .columns
                        .iter()
                        .map(|column| column.as_deref())
                        .eq(columns.iter().copied().map(Some)),
                }
        }))
    }

    pub async fn hasForeignKey(
        &mut self,
        table: &str,
        selector: KeySelector<'_>,
    ) -> DatabaseResult<bool> {
        Ok(self
            .getForeignKeys(table)
            .await?
            .iter()
            .any(|key| match selector {
                KeySelector::Name(name) => key.name.as_deref() == Some(name),
                KeySelector::Columns(columns) => key.columns == columns,
            }))
    }

    pub async fn getForeignKeys(&mut self, table: &str) -> DatabaseResult<Vec<ForeignKey>> {
        let table = self.catalog_table(table)?;
        let parameters = self.catalog_parameters(&table);
        let (sql, parameters) = if self.grammar.mysql() {
            (
                "select k.constraint_name as key_id, k.column_name as column_name, k.referenced_table_schema as foreign_schema, k.referenced_table_name as foreign_table, k.referenced_column_name as foreign_column, lower(r.delete_rule) as on_delete, lower(r.update_rule) as on_update from information_schema.key_column_usage k join information_schema.referential_constraints r on r.constraint_schema = k.constraint_schema and r.constraint_name = k.constraint_name and r.table_name = k.table_name where k.table_schema = coalesce(nullif(?, ''), database()) and k.table_name = ? and k.referenced_table_name is not null order by k.constraint_name, k.ordinal_position",
                parameters,
            )
        } else if self.grammar.postgres() {
            (postgres::FOREIGN_KEYS, optional_schema_parameters(&table))
        } else if self.grammar.sql_server() {
            (
                "select fk.name as key_id, c.name as column_name, fs.name as foreign_schema, \
                 ft.name as foreign_table, fc.name as foreign_column, \
                 lower(replace(fk.delete_referential_action_desc, '_', ' ')) as on_delete, \
                 lower(replace(fk.update_referential_action_desc, '_', ' ')) as on_update \
                 from sys.foreign_keys fk join sys.foreign_key_columns fkc on fkc.constraint_object_id = fk.object_id \
                 join sys.tables t on t.object_id = fk.parent_object_id \
                 join sys.schemas s on s.schema_id = t.schema_id \
                 join sys.columns c on c.object_id = t.object_id and c.column_id = fkc.parent_column_id \
                 join sys.tables ft on ft.object_id = fk.referenced_object_id \
                 join sys.schemas fs on fs.schema_id = ft.schema_id \
                 join sys.columns fc on fc.object_id = ft.object_id and fc.column_id = fkc.referenced_column_id \
                 where s.name = coalesce(nullif(?, ''), schema_name()) and t.name = ? \
                 order by fk.name, fkc.constraint_column_id",
                optional_schema_parameters(&table),
            )
        } else {
            let [schema, name] = parameters;
            (
                r#"select id as key_id, "from" as column_name, ?2 as foreign_schema,
                          "table" as foreign_table, "to" as foreign_column,
                          lower(on_delete) as on_delete, lower(on_update) as on_update
                   from pragma_foreign_key_list(?1, ?2) order by id, seq"#,
                [name, schema],
            )
        };
        let rows = self.select(sql, &parameters).await?;
        rows.chunk_by(|left, right| left.get("key_id") == right.get("key_id"))
            .map(|parts| {
                let row = &parts[0];
                Ok(ForeignKey {
                    name: (self.grammar.mysql()
                        || self.grammar.postgres()
                        || self.grammar.sql_server())
                    .then(|| text(row, "key_id"))
                    .transpose()?,
                    columns: parts
                        .iter()
                        .map(|part| text(part, "column_name"))
                        .collect::<DatabaseResult<_>>()?,
                    foreign_schema: optional_text(row, "foreign_schema")?,
                    foreign_table: text(row, "foreign_table")?,
                    foreign_columns: parts
                        .iter()
                        .map(|part| optional_text(part, "foreign_column"))
                        .collect::<DatabaseResult<_>>()?,
                    on_delete: text(row, "on_delete")?,
                    on_update: text(row, "on_update")?,
                })
            })
            .collect()
    }
}

fn optional_schema_parameters(table: &Table) -> [Value; 2] {
    [
        table.schema.as_deref().unwrap_or_default().into(),
        table.name.as_str().into(),
    ]
}

fn postgres_schema_filter(column: &str, schemas: Option<&[&str]>) -> (String, Vec<Value>) {
    match schemas {
        Some(schemas) => schema_filter(&format!("{column}::text"), schemas),
        None => (
            format!("{column} !~ '^pg_' and {column} <> 'information_schema'"),
            Vec::new(),
        ),
    }
}

fn schema_filter(column: &str, schemas: &[&str]) -> (String, Vec<Value>) {
    let parameters = vec!["?"; schemas.len()].join(", ");
    let bindings = schemas.iter().copied().map(Value::from).collect();
    (format!("{column} in ({parameters})"), bindings)
}

fn type_info(row: &Row) -> DatabaseResult<TypeInfo> {
    let name = text(row, "name")?;
    let schema = text(row, "schema")?;
    let kind = match text(row, "type_code")?.as_str() {
        "b" => Some("base"),
        "c" => Some("composite"),
        "d" => Some("domain"),
        "e" => Some("enum"),
        "p" => Some("pseudo"),
        "r" => Some("range"),
        "m" => Some("multirange"),
        _ => None,
    };
    let category = match text(row, "category_code")?.as_str() {
        "A" => Some("array"),
        "B" => Some("boolean"),
        "C" => Some("composite"),
        "D" => Some("date_time"),
        "E" => Some("enum"),
        "G" => Some("geometric"),
        "I" => Some("network_address"),
        "N" => Some("numeric"),
        "P" => Some("pseudo"),
        "R" => Some("range"),
        "S" => Some("string"),
        "T" => Some("timespan"),
        "U" => Some("user_defined"),
        "V" => Some("bit_string"),
        "X" => Some("unknown"),
        "Z" => Some("internal_use"),
        _ => None,
    };
    Ok(TypeInfo {
        schema_qualified_name: format!("{schema}.{name}"),
        name,
        schema,
        implicit: flag(row, "implicit")?,
        r#type: kind.map(str::to_owned),
        category: category.map(str::to_owned),
    })
}

fn view_info(row: &Row) -> DatabaseResult<ViewInfo> {
    let name = text(row, "name")?;
    let schema = text(row, "schema_name")?;
    Ok(ViewInfo {
        schema_qualified_name: format!("{schema}.{name}"),
        name,
        schema,
        definition: optional_text(row, "definition")?,
    })
}

fn optional_text(row: &Row, column: &str) -> DatabaseResult<Option<String>> {
    match row.get(column) {
        Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => Ok(Some(value.clone())),
        Some(Value::Bytes(value)) => String::from_utf8(value.clone())
            .map(Some)
            .map_err(|_| DatabaseError::InvalidSchema("catalog encoding")),
        _ => Err(DatabaseError::InvalidSchema("catalog value type")),
    }
}

fn text(row: &Row, column: &str) -> DatabaseResult<String> {
    optional_text(row, column)?.ok_or(DatabaseError::InvalidSchema("missing catalog value"))
}

fn flag(row: &Row, column: &str) -> DatabaseResult<bool> {
    match row.get(column) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::Integer(value)) => Ok(*value != 0),
        Some(Value::Unsigned(value)) => Ok(*value != 0),
        _ => Err(DatabaseError::InvalidSchema("catalog flag type")),
    }
}

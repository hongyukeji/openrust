use std::collections::BTreeMap;

use crate::database::{DatabaseError, DatabaseResult, Row, Value};

use super::Builder;
use crate::database::schema::{Column, ForeignKey, Index, TableInfo, grammars::Table};

#[allow(non_snake_case, reason = "Matches Laravel's Schema Builder API")]
impl Builder<'_> {
    fn catalog_parameters(&self, table: &Table) -> [Value; 2] {
        [
            self.grammar.catalog_schema(table).into(),
            table.name.as_str().into(),
        ]
    }

    pub async fn hasTable(&mut self, table: &str) -> DatabaseResult<bool> {
        let table = self.grammar.table(table)?;
        let parameters = self.catalog_parameters(&table);
        let sql = if self.grammar.mysql() {
            "select 1 from information_schema.tables where table_schema = coalesce(nullif(?, ''), database()) and table_name = ? and table_type = 'BASE TABLE' limit 1"
        } else {
            "select 1 from pragma_table_list where schema = ? and name = ? collate nocase and type in ('table', 'virtual') limit 1"
        };
        Ok(!self.select(sql, &parameters).await?.is_empty())
    }

    pub async fn getTables(&mut self, schema: Option<&str>) -> DatabaseResult<Vec<TableInfo>> {
        let (sql, schema) = if self.grammar.mysql() {
            (
                "select table_name as name, table_schema as schema_name from information_schema.tables where table_schema = coalesce(nullif(?, ''), database()) and table_type = 'BASE TABLE' order by table_name",
                schema.unwrap_or(""),
            )
        } else {
            (
                "select name, schema as schema_name from pragma_table_list where schema = ? and type in ('table', 'virtual') and substr(name,1,7) <> 'sqlite_' order by name",
                schema.unwrap_or("main"),
            )
        };
        self.select(sql, &[schema.into()])
            .await?
            .iter()
            .map(|row| {
                Ok(TableInfo {
                    name: text(row, "name")?,
                    schema: text(row, "schema_name")?,
                })
            })
            .collect()
    }

    pub async fn getColumns(&mut self, table: &str) -> DatabaseResult<Vec<Column>> {
        let table = self.grammar.table(table)?;
        let parameters = self.catalog_parameters(&table);
        let (sql, parameters) = if self.grammar.mysql() {
            (
                "select column_name as name, data_type as type_name, column_type as full_type, is_nullable = 'YES' as nullable, column_default as default_value, extra like '%auto_increment%' as auto_increment, column_key = 'PRI' as primary_key, column_comment as comment from information_schema.columns where table_schema = coalesce(nullif(?, ''), database()) and table_name = ? order by ordinal_position",
                parameters,
            )
        } else {
            let [schema, name] = parameters;
            (
                "select name, lower(type) as full_type, lower(type) as type_name, not \"notnull\" and pk = 0 as nullable, dflt_value as default_value, 0 as auto_increment, pk > 0 as primary_key, null as comment from pragma_table_xinfo(?, ?) order by cid",
                [name, schema],
            )
        };
        let mut columns = self
            .select(sql, &parameters)
            .await?
            .iter()
            .map(|row| {
                Ok(Column {
                    name: text(row, "name")?,
                    type_name: text(row, "type_name")?,
                    full_type: text(row, "full_type")?,
                    nullable: flag(row, "nullable")?,
                    default: optional_text(row, "default_value")?,
                    auto_increment: flag(row, "auto_increment")?,
                    primary: flag(row, "primary_key")?,
                    comment: optional_text(row, "comment")?,
                })
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        if !self.grammar.mysql()
            && columns
                .iter()
                .any(|column| column.primary && column.type_name == "integer")
        {
            let schema = self
                .grammar
                .identifier(self.grammar.catalog_schema(&table))?;
            let sql = format!(
                "select sql from {schema}.sqlite_schema where type = 'table' and name = ? collate nocase"
            );
            let rows = self.select(&sql, &[table.name.into()]).await?;
            if let Some(Value::Text(sql)) = rows.first().and_then(|row| row.get("sql"))
                && let Some(name) = super::sqlite::autoincrement_column(sql)?
            {
                for column in &mut columns {
                    column.auto_increment = column.name.eq_ignore_ascii_case(&name);
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

    pub async fn getColumnType(
        &mut self,
        table: &str,
        name: &str,
        full: bool,
    ) -> DatabaseResult<String> {
        let column = self
            .getColumns(table)
            .await?
            .into_iter()
            .find(|column| column.name.eq_ignore_ascii_case(name))
            .ok_or(DatabaseError::InvalidSchema("unknown column"))?;
        Ok(if full {
            column.full_type
        } else {
            column.type_name
        })
    }

    pub async fn getIndexes(&mut self, table: &str) -> DatabaseResult<Vec<Index>> {
        let identifier = self.grammar.table(table)?;
        let parameters = self.catalog_parameters(&identifier);
        let (sql, parameters) = if self.grammar.mysql() {
            (
                "select index_name as name, column_name as column_name, lower(index_type) as index_type, not non_unique as unique_key, index_name = 'PRIMARY' as primary_key from information_schema.statistics where table_schema = coalesce(nullif(?, ''), database()) and table_name = ? order by index_name, seq_in_index",
                parameters.to_vec(),
            )
        } else {
            let [schema, name] = parameters;
            (
                "select i.name, c.name as column_name, null as index_type, i.\"unique\" as unique_key, i.origin = 'pk' as primary_key from pragma_index_list(?, ?) i join pragma_index_info(i.name, ?) c order by i.name, c.seqno",
                vec![name, schema.clone(), schema],
            )
        };
        let mut indexes = BTreeMap::<String, Index>::new();
        for row in self.select(sql, &parameters).await? {
            let name = text(&row, "name")?;
            let primary = flag(&row, "primary_key")?;
            let key = if primary { "primary".to_owned() } else { name };
            let index = indexes.entry(key.clone()).or_insert(Index {
                name: key,
                columns: Vec::new(),
                index_type: optional_text(&row, "index_type")?,
                unique: flag(&row, "unique_key")?,
                primary,
            });
            index.columns.push(optional_text(&row, "column_name")?);
        }
        if !self.grammar.mysql() && !indexes.contains_key("primary") {
            let columns: Vec<_> = self
                .getColumns(table)
                .await?
                .into_iter()
                .filter(|column| column.primary)
                .map(|column| Some(column.name))
                .collect();
            if !columns.is_empty() {
                indexes.insert(
                    "primary".into(),
                    Index {
                        name: "primary".into(),
                        columns,
                        index_type: None,
                        unique: true,
                        primary: true,
                    },
                );
            }
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
        Ok(self
            .getIndexes(table)
            .await?
            .iter()
            .any(|index| index.name.eq_ignore_ascii_case(name)))
    }

    pub async fn getForeignKeys(&mut self, table: &str) -> DatabaseResult<Vec<ForeignKey>> {
        let table = self.grammar.table(table)?;
        let parameters = self.catalog_parameters(&table);
        let (sql, parameters) = if self.grammar.mysql() {
            (
                "select k.constraint_name as name, k.column_name as column_name, k.referenced_table_schema as foreign_schema, k.referenced_table_name as foreign_table, k.referenced_column_name as foreign_column, lower(r.delete_rule) as on_delete, lower(r.update_rule) as on_update from information_schema.key_column_usage k join information_schema.referential_constraints r on r.constraint_schema = k.constraint_schema and r.constraint_name = k.constraint_name and r.table_name = k.table_name where k.table_schema = coalesce(nullif(?, ''), database()) and k.table_name = ? and k.referenced_table_name is not null order by k.constraint_name, k.ordinal_position",
                parameters,
            )
        } else {
            let [schema, name] = parameters;
            (
                "select cast(id as text) as name, \"from\" as column_name, null as foreign_schema, \"table\" as foreign_table, \"to\" as foreign_column, lower(on_delete) as on_delete, lower(on_update) as on_update from pragma_foreign_key_list(?, ?) order by id, seq",
                [name, schema],
            )
        };
        let mut keys = BTreeMap::<String, ForeignKey>::new();
        for row in self.select(sql, &parameters).await? {
            let name = text(&row, "name")?;
            let key = keys.entry(name.clone()).or_insert(ForeignKey {
                name: self.grammar.mysql().then_some(name),
                columns: Vec::new(),
                foreign_schema: optional_text(&row, "foreign_schema")?,
                foreign_table: text(&row, "foreign_table")?,
                foreign_columns: Vec::new(),
                on_delete: text(&row, "on_delete")?,
                on_update: text(&row, "on_update")?,
            });
            key.columns.push(text(&row, "column_name")?);
            key.foreign_columns
                .push(optional_text(&row, "foreign_column")?);
        }
        Ok(keys.into_values().collect())
    }
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

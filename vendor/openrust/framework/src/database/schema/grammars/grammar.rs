use std::{fmt::Write as _, sync::Arc};

use crate::database::{
    DatabaseError, DatabaseResult, Value,
    query::grammars::Grammar as QueryGrammar,
    schema::{
        Blueprint, ColumnDefinition, ForeignKeyDefinition, IndexDefinition,
        blueprint::Command,
        column_definition::{ColumnType, DefaultValue, Position},
        index_definition::IndexType,
    },
};

#[derive(Clone)]
pub(in crate::database::schema) struct Grammar {
    grammar: QueryGrammar,
    prefix_indexes: bool,
    database: Arc<str>,
}

pub(in crate::database::schema) struct Table {
    pub schema: Option<String>,
    pub name: String,
}

impl Grammar {
    pub fn new(grammar: QueryGrammar, prefix_indexes: bool, database: &str) -> Self {
        Self {
            grammar,
            prefix_indexes,
            database: database.into(),
        }
    }

    pub fn mysql(&self) -> bool {
        self.grammar.is_mysql()
    }

    pub fn table(&self, name: &str) -> DatabaseResult<Table> {
        let (schema, name) = name
            .split_once('.')
            .map_or((None, name), |(schema, table)| {
                (Some(schema.to_owned()), table)
            });
        if name.is_empty() || name.contains('.') || schema.as_deref() == Some("") {
            return Err(DatabaseError::InvalidSchema("table name"));
        }
        let table = Table {
            schema,
            name: format!("{}{name}", self.grammar.table_prefix()),
        };
        self.qualified(&table)?;
        Ok(table)
    }

    pub fn catalog_schema<'a>(&'a self, table: &'a Table) -> &'a str {
        table
            .schema
            .as_deref()
            .unwrap_or_else(|| if self.mysql() { &self.database } else { "main" })
    }

    pub fn identifier(&self, name: &str) -> DatabaseResult<String> {
        self.grammar.quote(name)
    }

    pub fn qualified(&self, table: &Table) -> DatabaseResult<String> {
        let name = self.identifier(&table.name)?;
        match &table.schema {
            Some(schema) => Ok(format!("{}.{name}", self.identifier(schema)?)),
            None => Ok(name),
        }
    }

    pub fn qualified_index(&self, table: &Table, index: &str) -> DatabaseResult<String> {
        match &table.schema {
            Some(schema) => Ok(format!(
                "{}.{}",
                self.identifier(schema)?,
                self.identifier(index)?
            )),
            None => self.identifier(index),
        }
    }

    pub fn columns(&self, columns: &[String]) -> DatabaseResult<String> {
        if columns.is_empty() {
            return Err(DatabaseError::InvalidSchema("empty column list"));
        }
        columns
            .iter()
            .map(|column| self.identifier(column))
            .collect::<DatabaseResult<Vec<_>>>()
            .map(|columns| columns.join(", "))
    }

    fn conventional_name(&self, table: &Table, columns: &[String], suffix: &str) -> String {
        let name = if self.prefix_indexes {
            &table.name
        } else {
            table
                .name
                .strip_prefix(self.grammar.table_prefix())
                .unwrap_or(&table.name)
        };
        let name = match &table.schema {
            Some(schema) => format!("{schema}_{name}"),
            None => name.to_owned(),
        };
        format!("{name}_{}_{suffix}", columns.join("_"))
            .to_lowercase()
            .replace(['-', '.'], "_")
    }

    pub fn create(&self, table: &Table, blueprint: Blueprint) -> DatabaseResult<Vec<String>> {
        if blueprint.temporary {
            return Err(DatabaseError::UnsupportedSchema(
                "temporary table on a pooled connection",
            ));
        }
        let mut definitions = Vec::new();
        let mut constraints = Vec::new();
        let mut indexes = Vec::new();
        let mut columns = 0;
        let qualified = self.qualified(table)?;
        for command in blueprint.commands {
            match command {
                Command::Column(column) if !column.change => {
                    if column.position.is_some() {
                        return Err(DatabaseError::InvalidSchema(
                            "column position during create",
                        ));
                    }
                    columns += 1;
                    definitions.push(self.column(&column)?);
                    self.column_indexes(table, &column, &mut indexes)?;
                }
                Command::Index(index) if index.kind == IndexType::Primary => {
                    if index.inplace || index.lock.is_some() {
                        return Err(DatabaseError::UnsupportedSchema(
                            "primary key modifiers on create",
                        ));
                    }
                    constraints.push(format!("primary key ({})", self.columns(&index.columns)?));
                }
                Command::Index(index) => indexes.push(self.add_index(table, &index)?),
                Command::Foreign(foreign) if foreign.inplace || foreign.lock.is_some() => {
                    indexes.push(self.add_foreign(table, &foreign)?);
                }
                Command::Foreign(foreign) => constraints.push(self.foreign(table, &foreign)?),
                _ => return Err(DatabaseError::InvalidSchema("operation during create")),
            }
        }
        if columns == 0 {
            return Err(DatabaseError::InvalidSchema("no table columns"));
        }
        definitions.extend(constraints);
        let mut sql = vec![format!(
            "create table {qualified} ({})",
            definitions.join(", ")
        )];
        sql.extend(indexes);
        Ok(sql)
    }

    pub fn alter(&self, table: &Table, blueprint: Blueprint) -> DatabaseResult<Vec<String>> {
        if blueprint.temporary {
            return Err(DatabaseError::InvalidSchema(
                "temporary modifier during alter",
            ));
        }
        let qualified = self.qualified(table)?;
        let mut sql = Vec::new();
        for command in blueprint.commands {
            match command {
                Command::Column(column) => {
                    let verb = if column.change {
                        if !self.mysql() {
                            return Err(DatabaseError::UnsupportedSchema(
                                "SQLite column change requires a table rebuild",
                            ));
                        }
                        "modify column"
                    } else {
                        "add column"
                    };
                    sql.push(format!(
                        "alter table {qualified} {verb} {}",
                        self.column(&column)?
                    ));
                    self.column_indexes(table, &column, &mut sql)?;
                }
                Command::Index(index) => sql.push(self.add_index(table, &index)?),
                Command::Foreign(foreign) => sql.push(self.add_foreign(table, &foreign)?),
                Command::RenameColumn(from, to) => sql.push(format!(
                    "alter table {qualified} rename column {} to {}",
                    self.identifier(&from)?,
                    self.identifier(&to)?
                )),
                Command::DropColumn(column) => sql.push(format!(
                    "alter table {qualified} drop column {}",
                    self.identifier(&column)?
                )),
                Command::RenameIndex(from, to) if self.mysql() => sql.push(format!(
                    "alter table {qualified} rename index {} to {}",
                    self.identifier(&from)?,
                    self.identifier(&to)?
                )),
                Command::RenameIndex(_, _) => {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite index rename requires recreating the index",
                    ));
                }
                Command::DropIndex(IndexType::Primary, _) if self.mysql() => {
                    sql.push(format!("alter table {qualified} drop primary key"))
                }
                Command::DropIndex(IndexType::Primary, _) => {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite primary key drop requires a table rebuild",
                    ));
                }
                Command::DropIndex(_, name) if self.mysql() => sql.push(format!(
                    "alter table {qualified} drop index {}",
                    self.identifier(&name)?
                )),
                Command::DropIndex(_, name) => sql.push(format!(
                    "drop index {}",
                    self.qualified_index(table, &name)?
                )),
                Command::DropForeign(name) if self.mysql() => sql.push(format!(
                    "alter table {qualified} drop foreign key {}",
                    self.identifier(&name)?
                )),
                Command::DropForeignColumns(columns) if self.mysql() => {
                    let name = self.conventional_name(table, &columns, "foreign");
                    sql.push(format!(
                        "alter table {qualified} drop foreign key {}",
                        self.identifier(&name)?
                    ));
                }
                Command::DropForeign(_) | Command::DropForeignColumns(_) => {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite foreign key drop requires a table rebuild",
                    ));
                }
            }
        }
        Ok(sql)
    }

    fn column_indexes(
        &self,
        table: &Table,
        column: &ColumnDefinition,
        indexes: &mut Vec<String>,
    ) -> DatabaseResult<()> {
        for (enabled, kind) in [
            (column.unique, IndexType::Unique),
            (column.indexed, IndexType::Index),
        ] {
            if enabled {
                indexes.push(
                    self.add_index(table, &IndexDefinition::new(kind, &[&column.name], None))?,
                );
            }
        }
        Ok(())
    }

    fn add_index(&self, table: &Table, index: &IndexDefinition) -> DatabaseResult<String> {
        let columns = self.columns(&index.columns)?;
        let default;
        let name = match &index.name {
            Some(name) => name,
            None => {
                default = self.conventional_name(table, &index.columns, index.kind.suffix());
                &default
            }
        };
        if self.mysql() {
            let table = self.qualified(table)?;
            let kind = match index.kind {
                IndexType::Primary => format!("primary key ({columns})"),
                kind => {
                    let verb = match kind {
                        IndexType::Unique => "unique",
                        IndexType::FullText => "fulltext",
                        IndexType::Spatial => "spatial index",
                        _ => "index",
                    };
                    format!("{verb} {} ({columns})", self.identifier(name)?)
                }
            };
            Ok(format!(
                "alter table {table} add {kind}{}",
                self.ddl_options(index.inplace, index.lock.as_deref())?
            ))
        } else {
            self.ddl_options(index.inplace, index.lock.as_deref())?;
            let verb = match index.kind {
                IndexType::Unique => "create unique index",
                IndexType::Index => "create index",
                _ => return Err(DatabaseError::UnsupportedSchema("SQLite index kind")),
            };
            Ok(format!(
                "{verb} {} on {} ({columns})",
                self.qualified_index(table, name)?,
                self.identifier(&table.name)?
            ))
        }
    }

    fn add_foreign(&self, table: &Table, foreign: &ForeignKeyDefinition) -> DatabaseResult<String> {
        if !self.mysql() {
            return Err(DatabaseError::UnsupportedSchema(
                "SQLite foreign key addition requires a table rebuild",
            ));
        }
        Ok(format!(
            "alter table {} add {}{}",
            self.qualified(table)?,
            self.foreign(table, foreign)?,
            self.ddl_options(foreign.inplace, foreign.lock.as_deref())?
        ))
    }

    pub fn foreign(&self, table: &Table, foreign: &ForeignKeyDefinition) -> DatabaseResult<String> {
        if foreign.columns.len() != foreign.foreign_columns.len() {
            return Err(DatabaseError::InvalidSchema("foreign key column count"));
        }
        let target = self.table(
            foreign
                .foreign_table
                .as_deref()
                .ok_or(DatabaseError::InvalidSchema("foreign table"))?,
        )?;
        if !self.mysql() && target.schema.is_some() && target.schema != table.schema {
            return Err(DatabaseError::UnsupportedSchema(
                "SQLite cross-schema foreign key",
            ));
        }
        let reference = if self.mysql() {
            self.qualified(&target)?
        } else {
            self.identifier(&target.name)?
        };
        let default;
        let name = match &foreign.name {
            Some(name) => name,
            None => {
                default = self.conventional_name(table, &foreign.columns, "foreign");
                &default
            }
        };
        let mut sql = format!(
            "constraint {} foreign key ({}) references {reference} ({})",
            self.identifier(name)?,
            self.columns(&foreign.columns)?,
            self.columns(&foreign.foreign_columns)?
        );
        for (clause, action) in [
            ("on delete", &foreign.on_delete),
            ("on update", &foreign.on_update),
        ] {
            if let Some(action) = action {
                let action = action.to_ascii_lowercase();
                if !matches!(
                    action.as_str(),
                    "cascade" | "restrict" | "no action" | "set null" | "set default"
                ) {
                    return Err(DatabaseError::InvalidSchema("foreign key action"));
                }
                write!(sql, " {clause} {action}").expect("writing to a String");
            }
        }
        Ok(sql)
    }

    fn ddl_options(&self, inplace: bool, lock: Option<&str>) -> DatabaseResult<String> {
        if !self.mysql() && (inplace || lock.is_some()) {
            return Err(DatabaseError::UnsupportedSchema(
                "SQLite DDL algorithm or lock",
            ));
        }
        let mut result = String::new();
        if inplace {
            result.push_str(", algorithm=inplace");
        }
        if let Some(lock) = lock {
            let lock = lock.to_ascii_lowercase();
            if !matches!(lock.as_str(), "none" | "shared" | "exclusive" | "default") {
                return Err(DatabaseError::InvalidSchema("DDL lock mode"));
            }
            write!(result, ", lock={lock}").expect("writing to a String");
        }
        Ok(result)
    }

    pub fn column(&self, column: &ColumnDefinition) -> DatabaseResult<String> {
        if !self.mysql()
            && (column.invisible || column.current_on_update || column.position.is_some())
        {
            return Err(DatabaseError::UnsupportedSchema("MySQL column modifier"));
        }
        let kind = self.column_type(column.kind)?;
        let integer = matches!(
            column.kind,
            ColumnType::TinyInteger
                | ColumnType::SmallInteger
                | ColumnType::MediumInteger
                | ColumnType::Integer
                | ColumnType::BigInteger
        );
        if column.increment && (!integer || column.nullable || column.default.is_some()) {
            return Err(DatabaseError::InvalidSchema("auto increment column"));
        }
        let mut sql = format!("{} {kind}", self.identifier(&column.name)?);
        if self.mysql() && column.unsigned {
            sql.push_str(" unsigned");
        }
        sql.push_str(if column.nullable {
            " null"
        } else {
            " not null"
        });
        if let Some(default) = &column.default {
            write!(
                sql,
                " default {}",
                self.default_value(default, column.kind)?
            )
            .expect("writing to a String");
        }
        if column.increment {
            if self.mysql() {
                sql.push_str(" auto_increment primary key");
            } else {
                sql.push_str(" primary key autoincrement");
            }
        } else if column.primary {
            sql.push_str(" primary key");
        }
        if column.current_on_update {
            let precision = match column.kind {
                ColumnType::DateTime(precision) | ColumnType::Timestamp(precision) => precision,
                _ => return Err(DatabaseError::InvalidSchema("on-update timestamp column")),
            };
            write!(sql, " on update {}", current_timestamp(precision))
                .expect("writing to a String");
        }
        if column.invisible {
            sql.push_str(" invisible");
        }
        if self.mysql()
            && let Some(comment) = &column.comment
        {
            write!(sql, " comment {}", self.string(comment)?).expect("writing to a String");
        }
        match &column.position {
            Some(Position::First) => sql.push_str(" first"),
            Some(Position::After(name)) => {
                write!(sql, " after {}", self.identifier(name)?).expect("writing to a String");
            }
            None => {}
        }
        Ok(sql)
    }

    fn column_type(&self, kind: ColumnType) -> DatabaseResult<String> {
        let mysql = self.mysql();
        let scalar = match kind {
            ColumnType::TinyInteger if mysql => "tinyint",
            ColumnType::SmallInteger if mysql => "smallint",
            ColumnType::MediumInteger if mysql => "mediumint",
            ColumnType::BigInteger if mysql => "bigint",
            ColumnType::TinyInteger
            | ColumnType::SmallInteger
            | ColumnType::MediumInteger
            | ColumnType::Integer
            | ColumnType::BigInteger => "integer",
            ColumnType::String(length) | ColumnType::Char(length) => {
                if length == 0 || length > 65_535 {
                    return Err(DatabaseError::InvalidSchema("column length"));
                }
                let name = if matches!(kind, ColumnType::Char(_)) {
                    "char"
                } else {
                    "varchar"
                };
                return Ok(if mysql {
                    format!("{name}({length})")
                } else {
                    "varchar".into()
                });
            }
            ColumnType::MediumText if mysql => "mediumtext",
            ColumnType::LongText if mysql => "longtext",
            ColumnType::Text | ColumnType::MediumText | ColumnType::LongText => "text",
            ColumnType::Boolean if mysql => "tinyint(1)",
            ColumnType::Boolean => "tinyint(1)",
            ColumnType::Decimal(total, places) if total > 0 && total <= 65 && places <= total => {
                return Ok(if mysql {
                    format!("decimal({total}, {places})")
                } else {
                    "numeric".into()
                });
            }
            ColumnType::Decimal(_, _) => {
                return Err(DatabaseError::InvalidSchema("decimal precision"));
            }
            ColumnType::Float if mysql => "float",
            ColumnType::Double if mysql => "double",
            ColumnType::Float | ColumnType::Double => "float",
            ColumnType::Binary => "blob",
            ColumnType::Json if mysql => "json",
            ColumnType::Json => "text",
            ColumnType::Date => "date",
            ColumnType::DateTime(precision)
            | ColumnType::Timestamp(precision)
            | ColumnType::Time(precision) => {
                if precision > 6 {
                    return Err(DatabaseError::InvalidSchema("time precision"));
                }
                let name = match kind {
                    ColumnType::Time(_) => "time",
                    ColumnType::Timestamp(_) if mysql => "timestamp",
                    _ => "datetime",
                };
                return Ok(if mysql && precision > 0 {
                    format!("{name}({precision})")
                } else {
                    name.into()
                });
            }
        };
        Ok(scalar.into())
    }

    fn default_value(&self, value: &DefaultValue, kind: ColumnType) -> DatabaseResult<String> {
        let DefaultValue::Value(value) = value else {
            if matches!(kind, ColumnType::Date) {
                return Ok(if self.mysql() {
                    "(CURDATE())".into()
                } else {
                    "CURRENT_DATE".into()
                });
            }
            let precision = match kind {
                ColumnType::DateTime(p) | ColumnType::Timestamp(p) => p,
                _ => 0,
            };
            return Ok(current_timestamp(if self.mysql() { precision } else { 0 }));
        };
        match value {
            Value::Null => Ok("null".into()),
            Value::Bool(value) => Ok(u8::from(*value).to_string()),
            Value::Integer(value) => Ok(value.to_string()),
            Value::Unsigned(value) => Ok(value.to_string()),
            Value::Real(value) if value.is_finite() => Ok(value.to_string()),
            Value::Text(value) => self.string(value),
            Value::Json(value) if !self.mysql() => self.string(&value.to_string()),
            Value::Json(value) => Ok(format!(
                "(cast({} as json))",
                self.string(&value.to_string())?
            )),
            _ => Err(DatabaseError::InvalidSchema("default value")),
        }
    }

    fn string(&self, value: &str) -> DatabaseResult<String> {
        if value.contains('\0') {
            return Err(DatabaseError::InvalidSchema("string literal"));
        }
        let value = if self.mysql() {
            value.replace('\\', "\\\\")
        } else {
            value.to_owned()
        };
        Ok(format!("'{}'", value.replace('\'', "''")))
    }
}

fn current_timestamp(precision: u8) -> String {
    if precision > 0 {
        format!("CURRENT_TIMESTAMP({precision})")
    } else {
        "CURRENT_TIMESTAMP".into()
    }
}

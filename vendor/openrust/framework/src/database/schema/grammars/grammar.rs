use std::{fmt::Write as _, sync::Arc};

use crate::database::{
    Connection, DatabaseError, DatabaseResult, Value,
    query::grammars::Grammar as QueryGrammar,
    schema::{
        Blueprint, ColumnDefinition, ForeignKeyDefinition, IndexDefinition, SqliteJsonTypes,
        TableOptions,
        blueprint::Command,
        column_definition::{ColumnType, DefaultValue, GeneratedColumn, Position},
        index_definition::IndexType,
    },
};

#[derive(Clone)]
pub(in crate::database::schema) struct Grammar {
    grammar: QueryGrammar,
    prefix_indexes: bool,
    database: Arc<str>,
    table_options: Arc<TableOptions>,
    sqlite_json_types: SqliteJsonTypes,
}

#[derive(Clone)]
pub(in crate::database::schema) struct Table {
    pub schema: Option<String>,
    pub name: String,
}

impl Grammar {
    pub fn new(connection: &Connection) -> Self {
        Self {
            grammar: connection.grammar.clone(),
            prefix_indexes: connection.prefix_indexes,
            database: connection.getDatabaseName().into(),
            table_options: connection.table_options.clone(),
            sqlite_json_types: connection.sqlite_json_types,
        }
    }

    pub fn table_prefix(&self) -> &Arc<str> {
        self.grammar.table_prefix()
    }

    pub fn postgres(&self) -> bool {
        self.grammar.is_postgres()
    }

    pub fn mysql(&self) -> bool {
        self.grammar.is_mysql()
    }

    pub fn parse_schema_and_table(reference: &str) -> DatabaseResult<(Option<&str>, &str)> {
        let (schema, name) = reference
            .split_once('.')
            .map_or((None, reference), |(schema, name)| (Some(schema), name));
        if name.is_empty() || name.contains('.') || schema == Some("") {
            return Err(DatabaseError::InvalidSchema("table name"));
        }
        Ok((schema, name))
    }

    pub fn table(&self, reference: &str) -> DatabaseResult<Table> {
        let (schema, name) = Self::parse_schema_and_table(reference)?;
        let table = Table {
            schema: schema.map(str::to_owned),
            name: format!("{}{name}", self.grammar.table_prefix()),
        };
        self.qualified(&table)?;
        Ok(table)
    }

    pub fn default_schema(&self) -> &str {
        if self.mysql() {
            &self.database
        } else if self.postgres() {
            "public"
        } else {
            "main"
        }
    }

    pub fn catalog_schema<'a>(&'a self, table: &'a Table) -> &'a str {
        table
            .schema
            .as_deref()
            .unwrap_or_else(|| self.default_schema())
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
                .strip_prefix(self.grammar.table_prefix().as_ref())
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

    pub fn create_database(&self, name: &str) -> DatabaseResult<String> {
        let mut sql = format!("create database {}", self.identifier(name)?);
        for (value, clause) in [
            (self.table_options.charset.as_deref(), "character set"),
            (self.table_options.collation.as_deref(), "collate"),
        ] {
            if let Some(value) = value {
                let value = mysql_option(value, "database character set or collation")?;
                write!(sql, " default {clause} {}", self.identifier(value)?)
                    .expect("writing to a String");
            }
        }
        Ok(sql)
    }

    pub fn create(&self, table: &Table, blueprint: Blueprint) -> DatabaseResult<Vec<String>> {
        let temporary = if blueprint.temporary {
            " temporary"
        } else {
            ""
        };
        let mut definitions = Vec::new();
        let mut constraints = Vec::new();
        let mut indexes = Vec::new();
        let mut starting_values = Vec::new();
        let mut columns = 0;
        let explicit_primary = self.mysql() && blueprint.has_explicit_primary();
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
                    definitions.push(self.column(&column, explicit_primary)?);
                    self.column_indexes(table, &column, &mut indexes)?;
                    self.column_start(&qualified, &column, &mut starting_values);
                }
                Command::Index(index) if index.kind == IndexType::Primary => {
                    if index.inplace || index.lock.is_some() {
                        return Err(DatabaseError::UnsupportedSchema(
                            "primary key modifiers on create",
                        ));
                    }
                    constraints.push(format!(
                        "primary key{} ({})",
                        self.index_method(&index)?,
                        self.columns(&index.columns)?
                    ));
                }
                Command::Index(index) => indexes.push(self.add_index(table, &index)?),
                Command::Foreign(foreign) if foreign.inplace || foreign.lock.is_some() => {
                    indexes.push(self.add_foreign(table, &foreign)?);
                }
                Command::Foreign(foreign) => constraints.push(self.foreign(table, &foreign)?),
                Command::TableComment(comment) => {
                    if self.mysql() {
                        indexes.push(self.table_comment(&qualified, &comment)?);
                    }
                }
                _ => return Err(DatabaseError::InvalidSchema("operation during create")),
            }
        }
        if columns == 0 {
            return Err(DatabaseError::InvalidSchema("no table columns"));
        }
        definitions.extend(constraints);
        let mut create = format!(
            "create{temporary} table {qualified} ({})",
            definitions.join(", ")
        );
        if self.mysql() {
            self.append_table_options(&mut create, &blueprint.table_options)?;
        }
        let mut sql = vec![create];
        sql.extend(indexes);
        sql.extend(starting_values);
        Ok(sql)
    }

    fn append_table_options(&self, sql: &mut String, options: &TableOptions) -> DatabaseResult<()> {
        if let Some(charset) = options
            .charset
            .as_deref()
            .or(self.table_options.charset.as_deref())
        {
            write!(
                sql,
                " default character set {}",
                mysql_option(charset, "table character set")?
            )
            .expect("writing to a String");
        }
        if let Some(collation) = options
            .collation
            .as_deref()
            .or(self.table_options.collation.as_deref())
        {
            write!(
                sql,
                " collate {}",
                self.string(mysql_option(collation, "table collation")?)?
            )
            .expect("writing to a String");
        }
        if let Some(engine) = options
            .engine
            .as_deref()
            .or(self.table_options.engine.as_deref())
        {
            write!(
                sql,
                " engine = {}",
                mysql_option(engine, "table storage engine")?
            )
            .expect("writing to a String");
        }
        Ok(())
    }

    fn table_comment(&self, table: &str, comment: &str) -> DatabaseResult<String> {
        Ok(format!(
            "alter table {table} comment = {}",
            self.string(comment)?
        ))
    }

    pub fn alter(&self, table: &Table, blueprint: Blueprint) -> DatabaseResult<Vec<String>> {
        if blueprint.temporary {
            return Err(DatabaseError::InvalidSchema(
                "temporary modifier during alter",
            ));
        }
        let explicit_primary = self.mysql() && blueprint.has_explicit_primary();
        let qualified = self.qualified(table)?;
        let mut sql = Vec::new();
        let mut starting_values = Vec::new();
        for command in blueprint.commands {
            match command {
                Command::Column(mut column) => {
                    let rename;
                    let verb = if column.change {
                        if !self.mysql() {
                            return Err(DatabaseError::UnsupportedSchema(
                                "SQLite column change requires a table rebuild",
                            ));
                        }
                        if let Some(name) = column.rename_to.take() {
                            rename = format!("change column {}", self.identifier(&column.name)?);
                            column.name = name;
                            &rename
                        } else {
                            "modify column"
                        }
                    } else {
                        "add column"
                    };
                    sql.push(format!(
                        "alter table {qualified} {verb} {}{}",
                        self.column(&column, explicit_primary)?,
                        self.ddl_options(
                            column.instant.then_some("instant"),
                            column.lock.as_deref()
                        )?
                    ));
                    self.column_indexes(table, &column, &mut sql)?;
                    self.column_start(&qualified, &column, &mut starting_values);
                }
                Command::Index(index) => sql.push(self.add_index(table, &index)?),
                Command::TableComment(comment) => {
                    if self.mysql() {
                        sql.push(self.table_comment(&qualified, &comment)?);
                    }
                }
                Command::Foreign(foreign) => sql.push(self.add_foreign(table, &foreign)?),
                Command::RenameColumn(from, to) => sql.push(format!(
                    "alter table {qualified} rename column {} to {}",
                    self.identifier(&from)?,
                    self.identifier(&to)?
                )),
                Command::DropColumn(drop) => {
                    let options =
                        self.ddl_options(drop.instant.then_some("instant"), drop.lock.as_deref())?;
                    if drop.columns.is_empty() {
                        return Err(DatabaseError::InvalidSchema("empty column list"));
                    }
                    let clauses = drop
                        .columns
                        .iter()
                        .map(|name| Ok(format!("drop column {}", self.identifier(name)?)))
                        .collect::<DatabaseResult<Vec<_>>>()?;
                    if self.mysql() {
                        sql.push(format!(
                            "alter table {qualified} {}{options}",
                            clauses.join(", ")
                        ));
                    } else {
                        sql.extend(
                            clauses
                                .into_iter()
                                .map(|clause| format!("alter table {qualified} {clause}")),
                        );
                    }
                }

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
                Command::DropIndex(_, name) => sql.push(self.drop_index(table, &name)?),
                Command::DropIndexColumns(kind, columns) => {
                    if columns.is_empty() {
                        return Err(DatabaseError::InvalidSchema("empty column list"));
                    }
                    let name = self.conventional_name(table, &columns, kind.suffix());
                    sql.push(self.drop_index(table, &name)?);
                }
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
        sql.extend(starting_values);
        Ok(sql)
    }

    fn drop_index(&self, table: &Table, name: &str) -> DatabaseResult<String> {
        if self.mysql() {
            Ok(format!(
                "alter table {} drop index {}",
                self.qualified(table)?,
                self.identifier(name)?
            ))
        } else {
            Ok(format!("drop index {}", self.qualified_index(table, name)?))
        }
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
            (column.spatial_indexed, IndexType::Spatial),
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
        let quoted;
        let columns = match index.raw_expression.as_deref() {
            Some(expression) => {
                if expression.trim().is_empty() || expression.contains('\0') {
                    return Err(DatabaseError::InvalidSchema("raw index expression"));
                }
                expression
            }
            None => {
                quoted = self.columns(&index.columns)?;
                &quoted
            }
        };
        let method = self.index_method(index)?;
        let default;
        let name = match &index.name {
            Some(name) => name,
            None => {
                default = self.conventional_name(table, &index.columns, index.kind.suffix());
                &default
            }
        };
        if self.postgres() && index.kind == IndexType::FullText {
            self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
            let language = index
                .language
                .as_deref()
                .filter(|value| !value.is_empty())
                .unwrap_or("english");
            if language.contains('\0') {
                return Err(DatabaseError::InvalidSchema("full-text language"));
            }
            let vectors = index
                .columns
                .iter()
                .map(|column| self.grammar.postgres_text_vector(column, language))
                .collect::<DatabaseResult<Vec<_>>>()?
                .join(" || ");
            return Ok(format!(
                "create index {} on {} using gin (({vectors}))",
                self.identifier(name)?,
                self.qualified(table)?
            ));
        }
        if self.mysql() {
            let table = self.qualified(table)?;
            let kind = match index.kind {
                IndexType::Primary => format!("primary key{method} ({columns})"),
                kind => {
                    let verb = match kind {
                        IndexType::Unique => "unique",
                        IndexType::FullText => "fulltext",
                        IndexType::Spatial => "spatial index",
                        _ => "index",
                    };
                    format!("{verb} {}{method} ({columns})", self.identifier(name)?)
                }
            };
            Ok(format!(
                "alter table {table} add {kind}{}",
                self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?
            ))
        } else {
            self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
            let verb = match index.kind {
                IndexType::Unique => "create unique index",
                IndexType::Index => "create index",
                _ => return Err(DatabaseError::UnsupportedSchema("SQLite index kind")),
            };
            Ok(format!(
                "{verb} {} on {} ({columns})",
                if self.postgres() {
                    self.identifier(name)?
                } else {
                    self.qualified_index(table, name)?
                },
                if self.postgres() {
                    self.qualified(table)?
                } else {
                    self.identifier(&table.name)?
                }
            ))
        }
    }

    fn index_method(&self, index: &IndexDefinition) -> DatabaseResult<&'static str> {
        match index.algorithm.as_deref() {
            None => Ok(""),
            Some(_) if !self.mysql() => {
                Err(DatabaseError::UnsupportedSchema("SQLite index method"))
            }
            Some(method) if method.eq_ignore_ascii_case("btree") => Ok(" using btree"),
            Some(method) if method.eq_ignore_ascii_case("hash") => Ok(" using hash"),
            Some(_) => Err(DatabaseError::InvalidSchema("index method")),
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
            self.ddl_options(
                foreign.inplace.then_some("inplace"),
                foreign.lock.as_deref()
            )?
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

    fn column_start(&self, table: &str, column: &ColumnDefinition, sql: &mut Vec<String>) {
        if self.mysql()
            && column.increment
            && let Some(value) = column.start.filter(|value| *value != 0)
        {
            sql.push(format!("alter table {table} auto_increment = {value}"));
        }
    }

    fn ddl_options(&self, algorithm: Option<&str>, lock: Option<&str>) -> DatabaseResult<String> {
        if !self.mysql() && (algorithm.is_some() || lock.is_some()) {
            return Err(DatabaseError::UnsupportedSchema(
                "SQLite DDL algorithm or lock",
            ));
        }
        let mut result = String::new();
        if let Some(algorithm) = algorithm {
            write!(result, ", algorithm={algorithm}").expect("writing to a String");
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

    pub fn column(
        &self,
        column: &ColumnDefinition,
        explicit_primary: bool,
    ) -> DatabaseResult<String> {
        if column.rename_to.is_some() {
            return Err(DatabaseError::InvalidSchema("renameTo requires change"));
        }
        if !self.mysql()
            && (column.invisible
                || column.current_on_update
                || column.position.is_some()
                || column.instant
                || column.lock.is_some())
        {
            return Err(DatabaseError::UnsupportedSchema("MySQL column modifier"));
        }
        let kind = self.column_type(&column.kind, &column.name)?;
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
        if self.mysql()
            && let Some(charset) = &column.charset
        {
            write!(
                sql,
                " character set {}",
                mysql_option(charset, "column character set")?
            )
            .expect("writing to a String");
        }
        if let Some(collation) = &column.collation {
            let collation = if self.mysql() {
                self.string(mysql_option(collation, "column collation")?)?
            } else {
                self.identifier(collation)?
            };
            write!(sql, " collate {collation}").expect("writing to a String");
        }
        if let Some(generated) = &column.generated {
            if column.default.is_some()
                || column.increment
                || column.primary
                || column.current_on_update
            {
                return Err(DatabaseError::InvalidSchema("generated column modifiers"));
            }
            let expression = self.generated_expression(generated)?;
            let mode = if generated.stored { " stored" } else { "" };
            write!(sql, " as ({expression}){mode}").expect("writing to a String");
            if !column.nullable {
                sql.push_str(" not null");
            }
        } else {
            sql.push_str(if column.nullable {
                " null"
            } else {
                " not null"
            });
        }
        if let Some(default) = &column.default {
            write!(
                sql,
                " default {}",
                self.default_value(default, &column.kind)?
            )
            .expect("writing to a String");
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
        if column.increment {
            if self.mysql() {
                sql.push_str(" auto_increment");
                if !explicit_primary && (!column.change || column.primary) {
                    sql.push_str(" primary key");
                }
            } else if self.postgres() {
                sql.push_str(" generated by default as identity primary key");
            } else {
                sql.push_str(" primary key autoincrement");
            }
        } else if column.primary {
            sql.push_str(" primary key");
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

    fn generated_expression(&self, generated: &GeneratedColumn) -> DatabaseResult<String> {
        let expression = generated.expression.trim();
        if expression.is_empty() || expression.contains('\0') {
            return Err(DatabaseError::InvalidSchema("generated column expression"));
        }
        if generated.json && expression.contains("->") {
            let (column, path) = self.grammar.json_selector(expression)?;
            let path = path.ok_or(DatabaseError::InvalidSchema("generated JSON column path"))?;
            let expression = format!("json_extract({column}, {})", self.string(&path)?);
            return Ok(if self.mysql() {
                format!("json_unquote({expression})")
            } else {
                expression
            });
        }
        Ok(expression.to_owned())
    }

    fn column_type(&self, kind: &ColumnType, column: &str) -> DatabaseResult<String> {
        if self.postgres() {
            let native = match kind {
                ColumnType::SmallInteger | ColumnType::TinyInteger => Some("smallint"),
                ColumnType::BigInteger => Some("bigint"),
                ColumnType::Boolean => Some("boolean"),
                ColumnType::Binary | ColumnType::SizedBinary(_, _) => Some("bytea"),
                ColumnType::Json => Some("json"),
                ColumnType::Jsonb => Some("jsonb"),
                ColumnType::Double => Some("double precision"),
                ColumnType::Timestamp(precision) | ColumnType::DateTime(precision) => {
                    return Ok(format!("timestamp({precision}) without time zone"));
                }
                _ => None,
            };
            if let Some(native) = native {
                return Ok(native.into());
            }
        }
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
                if *length == 0 || *length > 65_535 {
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
            ColumnType::TinyText if mysql => "tinytext",
            ColumnType::MediumText if mysql => "mediumtext",
            ColumnType::LongText if mysql => "longtext",
            ColumnType::TinyText
            | ColumnType::Text
            | ColumnType::MediumText
            | ColumnType::LongText => "text",
            ColumnType::Boolean if mysql => "tinyint(1)",
            ColumnType::Boolean => "tinyint(1)",
            ColumnType::Decimal(total, places) if *total > 0 && *total <= 65 && places <= total => {
                return Ok(if mysql {
                    format!("decimal({total}, {places})")
                } else {
                    "numeric".into()
                });
            }
            ColumnType::Decimal(_, _) => {
                return Err(DatabaseError::InvalidSchema("decimal precision"));
            }
            ColumnType::Float(0) => "float",
            ColumnType::Float(precision) if mysql => {
                if *precision > 53 {
                    return Err(DatabaseError::InvalidSchema("floating-point precision"));
                }
                return Ok(format!("float({precision})"));
            }
            ColumnType::Float(_) => "float",
            ColumnType::Double => "double",
            ColumnType::Raw(definition) => {
                if definition.trim().is_empty() || definition.contains('\0') {
                    return Err(DatabaseError::InvalidSchema("raw column definition"));
                }
                return Ok(definition.clone());
            }
            ColumnType::Binary => "blob",
            ColumnType::SizedBinary(length, fixed) => {
                if mysql {
                    let max = if *fixed { 255 } else { 65_535 };
                    if *length == 0 || *length > max {
                        return Err(DatabaseError::InvalidSchema("binary length"));
                    }
                    let kind = if *fixed { "binary" } else { "varbinary" };
                    return Ok(format!("{kind}({length})"));
                }
                "blob"
            }
            ColumnType::Enum(values) | ColumnType::Set(values) => {
                let is_set = matches!(kind, ColumnType::Set(_));
                if is_set && !mysql {
                    return Err(DatabaseError::UnsupportedSchema("SQLite set column"));
                }
                if values.is_empty() || (is_set && values.len() > 64) {
                    return Err(DatabaseError::InvalidSchema("allowed column values"));
                }
                let values = values
                    .iter()
                    .map(|value| self.string(value))
                    .collect::<DatabaseResult<Vec<_>>>()?
                    .join(", ");
                if mysql {
                    let kind = if is_set { "set" } else { "enum" };
                    return Ok(format!("{kind}({values})"));
                }
                return Ok(format!(
                    "varchar check ({} in ({values}))",
                    self.identifier(column)?
                ));
            }
            ColumnType::IpAddress if mysql => "varchar(45)",
            ColumnType::MacAddress if mysql => "varchar(17)",
            ColumnType::IpAddress | ColumnType::MacAddress => "varchar",
            ColumnType::Spatial(subtype, srid) if mysql => {
                return Ok(if *srid == 0 {
                    (*subtype).into()
                } else {
                    format!("{subtype} srid {srid}")
                });
            }
            ColumnType::Spatial(_, _) => "geometry",
            ColumnType::Year if mysql => "year",
            ColumnType::Year => "integer",
            ColumnType::Json | ColumnType::Jsonb if mysql => "json",
            ColumnType::Json if self.sqlite_json_types.json => "json",
            ColumnType::Jsonb if self.sqlite_json_types.jsonb => "jsonb",
            ColumnType::Json | ColumnType::Jsonb => "text",
            ColumnType::Date => "date",
            ColumnType::DateTime(precision)
            | ColumnType::Timestamp(precision)
            | ColumnType::Time(precision) => {
                if *precision > 6 {
                    return Err(DatabaseError::InvalidSchema("time precision"));
                }
                let name = match kind {
                    ColumnType::Time(_) => "time",
                    ColumnType::Timestamp(_) if mysql => "timestamp",
                    _ => "datetime",
                };
                return Ok(if mysql && *precision > 0 {
                    format!("{name}({precision})")
                } else {
                    name.into()
                });
            }
        };
        Ok(scalar.into())
    }

    fn default_value(&self, value: &DefaultValue, kind: &ColumnType) -> DatabaseResult<String> {
        let value = match value {
            DefaultValue::Value(value) => value,
            DefaultValue::Expression(expression) => {
                let sql = expression
                    .unbound_sql()
                    .filter(|sql| !sql.trim().is_empty() && !sql.contains('\0'))
                    .ok_or(DatabaseError::InvalidSchema("column default expression"))?;
                return Ok(sql.to_owned());
            }
            DefaultValue::CurrentTimestamp => return Ok(self.current_default(kind)),
        };
        match value {
            Value::Null => Ok("null".into()),
            Value::Bool(value) if self.postgres() => Ok(value.to_string()),
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

    fn current_default(&self, kind: &ColumnType) -> String {
        match kind {
            ColumnType::Year if self.mysql() => "(YEAR(CURDATE()))".into(),
            ColumnType::Year => "(CAST(strftime('%Y', 'now') AS INTEGER))".into(),
            ColumnType::Date if self.mysql() => "(CURDATE())".into(),
            ColumnType::Date => "CURRENT_DATE".into(),
            ColumnType::DateTime(precision) | ColumnType::Timestamp(precision) if self.mysql() => {
                current_timestamp(*precision)
            }
            _ => current_timestamp(0),
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

fn mysql_option<'a>(value: &'a str, option: &'static str) -> DatabaseResult<&'a str> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(DatabaseError::InvalidSchema(option));
    }
    Ok(value)
}

fn current_timestamp(precision: u8) -> String {
    if precision > 0 {
        format!("CURRENT_TIMESTAMP({precision})")
    } else {
        "CURRENT_TIMESTAMP".into()
    }
}

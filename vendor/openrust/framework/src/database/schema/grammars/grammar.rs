use std::{borrow::Cow, fmt::Write as _, sync::Arc};

use crate::database::{
    Connection, DatabaseError, DatabaseResult, Value,
    query::{Expression, grammars::Grammar as QueryGrammar},
    schema::{
        Blueprint, ColumnDefinition, ForeignKeyDefinition, IndexDefinition, SqliteJsonTypes,
        TableOptions,
        blueprint::Command,
        column_definition::{
            ColumnType, CurrentDefault, DefaultValue, GeneratedColumn, Identity, Position,
        },
        index_definition::IndexType,
    },
};

#[derive(Clone)]
pub(in crate::database::schema) struct Grammar {
    grammar: QueryGrammar,
    prefix_indexes: bool,
    database: Arc<str>,
    postgres_schemas: Arc<[String]>,
    table_options: Arc<TableOptions>,
    sqlite_json_types: SqliteJsonTypes,
    mariadb_native_uuid: bool,
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
            postgres_schemas: Arc::clone(&connection.postgres_schemas),
            table_options: connection.table_options.clone(),
            sqlite_json_types: connection.sqlite_json_types,
            mariadb_native_uuid: false,
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

    pub fn mariadb(&self) -> bool {
        self.grammar.is_mariadb()
    }

    pub fn sql_server(&self) -> bool {
        self.grammar.is_sql_server()
    }

    pub fn use_native_mariadb_uuid(&mut self, enabled: bool) {
        self.mariadb_native_uuid = enabled;
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
            self.postgres_schemas
                .first()
                .map_or("public", String::as_str)
        } else if self.sql_server() {
            "dbo"
        } else {
            "main"
        }
    }

    pub fn postgres_schemas(&self) -> &[String] {
        &self.postgres_schemas
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

    pub fn sql_server_rename_table(&self, from: &Table, to: &Table) -> DatabaseResult<String> {
        if to.schema.is_some() && to.schema != from.schema {
            return Err(DatabaseError::UnsupportedSchema(
                "SQL Server cross-schema table rename",
            ));
        }
        self.sql_server_rename(&self.qualified(from)?, &to.name, "OBJECT")
    }

    fn sql_server_rename_member(
        &self,
        table: &str,
        from: &str,
        to: &str,
        kind: &str,
    ) -> DatabaseResult<String> {
        self.sql_server_rename(&format!("{table}.{}", self.identifier(from)?), to, kind)
    }

    fn sql_server_rename(&self, from: &str, to: &str, kind: &str) -> DatabaseResult<String> {
        if to.is_empty() || to.encode_utf16().count() > 128 {
            return Err(DatabaseError::InvalidSchema("SQL Server rename target"));
        }
        Ok(format!(
            "exec sys.sp_rename {}, {}, {}",
            self.string(from)?,
            self.string(to)?,
            self.string(kind)?
        ))
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
        if self.postgres() {
            if let Some(charset) = &self.table_options.charset {
                write!(sql, " encoding {}", self.identifier(charset)?)
                    .expect("writing to a String");
            }
            return Ok(sql);
        }
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
        if blueprint.temporary && self.sql_server() {
            return Err(DatabaseError::UnsupportedSchema(
                "SQL Server temporary table",
            ));
        }
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
        let explicit_primary = blueprint.has_explicit_primary();
        let qualified = self.qualified(table)?;
        for command in blueprint.commands {
            match command {
                Command::Column(column) if !column.change => {
                    if self.mysql() && column.position.is_some() {
                        return Err(DatabaseError::InvalidSchema(
                            "column position during create",
                        ));
                    }
                    columns += 1;
                    definitions.push(self.column(&column, explicit_primary)?);
                    self.column_indexes(table, &column, &column.name, &mut indexes)?;
                    self.column_start(&qualified, &column, &mut starting_values);
                    if self.postgres()
                        && let Some(comment) = &column.comment
                    {
                        indexes.push(self.column_comment(
                            &qualified,
                            &column.name,
                            Some(comment),
                        )?);
                    }
                }
                Command::Index(index) if index.kind == IndexType::Primary => {
                    if index.inplace || index.lock.is_some() {
                        return Err(DatabaseError::UnsupportedSchema(
                            "primary key modifiers on create",
                        ));
                    }
                    constraints.push(if self.sql_server() {
                        self.sql_server_primary_key(table, &index)?
                    } else {
                        self.primary_key(&index)?
                    });
                }
                Command::Index(index) => self.add_index(table, &index, &mut indexes)?,
                Command::Foreign(foreign)
                    if foreign.inplace || foreign.lock.is_some() || foreign.not_valid =>
                {
                    indexes.push(self.add_foreign(table, &foreign)?);
                }
                Command::Foreign(foreign) => constraints.push(self.foreign(table, &foreign)?),
                Command::TableComment(comment) => {
                    if self.mysql() || self.postgres() {
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
        let comment = self.string(comment)?;
        Ok(if self.postgres() {
            format!("comment on table {table} is {comment}")
        } else {
            format!("alter table {table} comment = {comment}")
        })
    }

    fn column_comment(
        &self,
        table: &str,
        column: &str,
        comment: Option<&str>,
    ) -> DatabaseResult<String> {
        let name = self.identifier(column)?;
        let comment = match comment {
            Some(comment) => self.string(comment)?,
            None => "null".into(),
        };
        Ok(format!("comment on column {table}.{name} is {comment}"))
    }

    pub fn alter(&self, table: &Table, blueprint: Blueprint) -> DatabaseResult<Vec<String>> {
        if blueprint.temporary {
            return Err(DatabaseError::InvalidSchema(
                "temporary modifier during alter",
            ));
        }
        let explicit_primary =
            (self.mysql() || self.postgres()) && blueprint.has_explicit_primary();
        let qualified = self.qualified(table)?;
        let mut sql = Vec::new();
        let mut starting_values = Vec::new();
        let mut defaults = Vec::new();
        let mut indexes = Vec::new();
        let mut commands = blueprint.commands.into_iter();
        while let Some(mut command) = commands.next() {
            let mut drop_primary = "";
            if self.mysql() && matches!(command, Command::DropIndex(IndexType::Primary, _)) {
                match commands.as_slice() {
                    [Command::Index(index), ..] if index.kind == IndexType::Primary => {
                        drop_primary = "drop primary key, ";
                        command = commands.next().expect("primary index command");
                    }
                    [Command::Column(column), primary, ..]
                        if primary_for_new_auto_column(column, primary) =>
                    {
                        drop_primary = "drop primary key, ";
                        command = commands.next().expect("auto-increment column command");
                    }
                    _ => {}
                }
            }
            match command {
                Command::Column(column) if self.postgres() && column.change => {
                    self.postgres_change(&qualified, &column, &mut sql)?;
                    self.column_indexes(table, &column, &column.name, &mut indexes)?;
                }
                Command::Column(column) if self.sql_server() && column.change => {
                    let definition = self.column(&column, false)?;
                    sql.push(self.sql_server_drop_column_constraints(
                        &qualified,
                        std::slice::from_ref(&column.name),
                        false,
                    )?);
                    sql.push(format!("alter table {qualified} alter column {definition}"));
                    if let Some(value) = self.column_default(&column)
                        && !matches!(value, DefaultValue::Value(Value::Null))
                    {
                        defaults.push(format!(
                            "alter table {qualified} add default {} for {}",
                            self.default_value(value, &column.kind)?,
                            self.identifier(&column.name)?
                        ));
                    }
                    self.column_indexes(table, &column, &column.name, &mut indexes)?;
                }
                Command::Column(mut column) => {
                    let mut previous_name = None;
                    let rename;
                    let verb = if column.change {
                        if !self.mysql() {
                            return Err(DatabaseError::UnsupportedSchema(
                                "SQLite column change requires a table rebuild",
                            ));
                        }
                        if let Some(name) = column.rename_to.take() {
                            rename = format!("change column {}", self.identifier(&column.name)?);
                            previous_name = Some(std::mem::replace(&mut column.name, name));
                            &rename
                        } else {
                            "modify column"
                        }
                    } else {
                        if self.sql_server() {
                            "add"
                        } else {
                            "add column"
                        }
                    };
                    let definition = self.column(&column, explicit_primary)?;
                    let compound_primary = if self.mysql()
                        && matches!(commands.as_slice(), [primary, ..] if primary_for_new_auto_column(&column, primary))
                    {
                        commands.next()
                    } else {
                        None
                    };
                    let algorithm = column.instant.then_some("instant");
                    let lock = column.lock.as_deref();
                    if let Some(Command::Index(index)) = compound_primary {
                        let primary = self.primary_key(&index)?;
                        let options = self.column_primary_options(algorithm, lock, &index)?;
                        sql.push(format!(
                            "alter table {qualified} {drop_primary}{verb} {definition}, add {primary}{options}"
                        ));
                    } else {
                        let options = self.ddl_options(algorithm, lock)?;
                        sql.push(format!(
                            "alter table {qualified} {verb} {definition}{options}"
                        ));
                    }
                    self.column_indexes(
                        table,
                        &column,
                        previous_name.as_deref().unwrap_or(&column.name),
                        &mut indexes,
                    )?;
                    self.column_start(&qualified, &column, &mut starting_values);
                    if self.postgres()
                        && let Some(comment) = &column.comment
                    {
                        sql.push(self.column_comment(&qualified, &column.name, Some(comment))?);
                    }
                }
                Command::Index(index) if !drop_primary.is_empty() => {
                    let primary = self.primary_key(&index)?;
                    let options = self
                        .ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
                    sql.push(format!(
                        "alter table {qualified} {drop_primary}add {primary}{options}"
                    ));
                }
                Command::Index(index) => self.add_index(table, &index, &mut sql)?,
                Command::TableComment(comment) => {
                    if self.mysql() || self.postgres() {
                        sql.push(self.table_comment(&qualified, &comment)?);
                    }
                }
                Command::Foreign(foreign) => sql.push(self.add_foreign(table, &foreign)?),
                Command::RenameColumn(from, to) if self.sql_server() => {
                    sql.push(self.sql_server_rename_member(&qualified, &from, &to, "COLUMN")?)
                }
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
                    if self.sql_server() {
                        sql.push(self.sql_server_drop_column_constraints(
                            &qualified,
                            &drop.columns,
                            true,
                        )?);
                        sql.push(format!(
                            "alter table {qualified} drop column {}",
                            self.columns(&drop.columns)?
                        ));
                        continue;
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

                Command::RenameIndex(from, to) if self.sql_server() => {
                    sql.push(self.sql_server_rename_member(&qualified, &from, &to, "INDEX")?)
                }
                Command::RenameIndex(from, to) if self.mysql() => sql.push(format!(
                    "alter table {qualified} rename index {} to {}",
                    self.identifier(&from)?,
                    self.identifier(&to)?
                )),
                Command::RenameIndex(from, to) if self.postgres() => {
                    let (schema, name) = Self::parse_schema_and_table(&from)?;
                    let source = match schema {
                        Some(schema) => {
                            format!("{}.{}", self.identifier(schema)?, self.identifier(name)?)
                        }
                        None => self.qualified_index(table, name)?,
                    };
                    sql.push(format!(
                        "alter index {source} rename to {}",
                        self.identifier(&to)?
                    ));
                }
                Command::RenameIndex(_, _) => {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite index rename requires recreating the index",
                    ));
                }
                Command::DropIndex(IndexType::Primary, _) if self.mysql() => {
                    sql.push(format!("alter table {qualified} drop primary key"));
                }
                Command::DropIndex(IndexType::Primary, name) if self.sql_server() => {
                    sql.push(self.sql_server_drop_primary(&qualified, &name)?);
                }
                Command::DropIndex(IndexType::Primary, _) => {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite primary key drop requires a table rebuild",
                    ));
                }
                Command::DropIndex(kind, name) => sql.push(self.drop_index(table, kind, &name)?),
                Command::DropIndexColumns(kind, columns) => {
                    if columns.is_empty() {
                        return Err(DatabaseError::InvalidSchema("empty column list"));
                    }
                    let name = self.conventional_name(table, &columns, kind.suffix());
                    sql.push(self.drop_index(table, kind, &name)?);
                }
                Command::DropForeign(name) => sql.push(self.drop_foreign(&qualified, &name)?),
                Command::DropForeignColumns(columns) => {
                    let name = self.conventional_name(table, &columns, "foreign");
                    sql.push(self.drop_foreign(&qualified, &name)?);
                }
            }
        }
        sql.extend(indexes);
        sql.extend(starting_values);
        sql.extend(defaults);
        Ok(sql)
    }

    fn drop_index(&self, table: &Table, kind: IndexType, name: &str) -> DatabaseResult<String> {
        if kind == IndexType::Vector && !self.postgres() && !self.mariadb() {
            return Err(DatabaseError::UnsupportedSchema("vector index"));
        }
        if kind == IndexType::Primary {
            if self.sql_server() {
                return self.sql_server_drop_primary(&self.qualified(table)?, name);
            }
            return Err(DatabaseError::UnsupportedSchema(
                "primary key drop by columns",
            ));
        }
        if self.mysql() {
            Ok(format!(
                "alter table {} drop index {}",
                self.qualified(table)?,
                self.identifier(name)?
            ))
        } else if self.sql_server() {
            Ok(format!(
                "drop index {} on {}",
                self.identifier(name)?,
                self.qualified(table)?
            ))
        } else if self.postgres() && kind == IndexType::Unique {
            self.drop_postgres_unique(table, name)
        } else {
            Ok(format!("drop index {}", self.qualified_index(table, name)?))
        }
    }

    fn column_indexes(
        &self,
        table: &Table,
        column: &ColumnDefinition,
        previous_name: &str,
        indexes: &mut Vec<String>,
    ) -> DatabaseResult<()> {
        for command in column.index_commands(previous_name, false) {
            match command {
                Command::Index(index) => self.add_index(table, &index, indexes)?,
                Command::DropIndexColumns(kind, columns) => indexes.push(self.drop_index(
                    table,
                    kind,
                    &self.conventional_name(table, &columns, kind.suffix()),
                )?),
                _ => unreachable!("column index command"),
            }
        }
        Ok(())
    }

    fn column_primary_options(
        &self,
        column_algorithm: Option<&str>,
        column_lock: Option<&str>,
        index: &IndexDefinition,
    ) -> DatabaseResult<String> {
        if column_algorithm.is_some() && index.inplace {
            return Err(DatabaseError::InvalidSchema(
                "conflicting MySQL ALTER algorithms",
            ));
        }
        let index_lock = index.lock.as_deref();
        if let (Some(column), Some(index)) = (column_lock, index_lock)
            && !column.eq_ignore_ascii_case(index)
        {
            return Err(DatabaseError::InvalidSchema(
                "conflicting MySQL ALTER lock modes",
            ));
        }
        self.ddl_options(
            column_algorithm.or(index.inplace.then_some("inplace")),
            column_lock.or(index_lock),
        )
    }

    fn primary_key(&self, index: &IndexDefinition) -> DatabaseResult<String> {
        if index.has_unique_modifiers() {
            return Err(DatabaseError::UnsupportedSchema(
                "PostgreSQL unique constraint modifiers",
            ));
        }
        if index.online {
            return Err(DatabaseError::UnsupportedSchema("online primary key"));
        }
        Ok(format!(
            "primary key{} ({})",
            self.index_method(index)?,
            self.columns(&index.columns)?
        ))
    }

    fn add_index(
        &self,
        table: &Table,
        index: &IndexDefinition,
        statements: &mut Vec<String>,
    ) -> DatabaseResult<()> {
        if self.sql_server() && index.kind == IndexType::Primary {
            self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
            statements.push(format!(
                "alter table {} add {}",
                self.qualified(table)?,
                self.sql_server_primary_key(table, index)?
            ));
            return Ok(());
        }
        if self.postgres() && index.kind == IndexType::Unique {
            return self.add_postgres_unique(table, index, statements);
        }
        if index.has_unique_modifiers() {
            return Err(DatabaseError::UnsupportedSchema(
                "PostgreSQL unique constraint modifiers",
            ));
        }
        statements.push(self.index_sql(table, index)?);
        Ok(())
    }

    fn sql_server_primary_key(
        &self,
        table: &Table,
        index: &IndexDefinition,
    ) -> DatabaseResult<String> {
        let default = self.conventional_name(table, &index.columns, "primary");
        let name = self.identifier(index.name.as_deref().unwrap_or(&default))?;
        Ok(format!("constraint {name} {}", self.primary_key(index)?))
    }

    fn sql_server_drop_primary(&self, table: &str, name: &str) -> DatabaseResult<String> {
        if !name.is_empty() {
            return Ok(format!(
                "alter table {table} drop constraint {}",
                self.identifier(name)?
            ));
        }
        let object = self.string(table)?;
        let drop = self.string(&format!("alter table {table} drop constraint "))?;
        Ok(format!(
            "declare @name sysname = (select name from sys.key_constraints \
                 where parent_object_id = object_id({object}) and type = 'PK'); \
             if @name is null throw 50000, N'Primary key constraint not found', 1; \
             declare @sql nvarchar(max) = {drop} + quotename(@name); \
             exec sys.sp_executesql @sql"
        ))
    }

    fn add_postgres_unique(
        &self,
        table: &Table,
        index: &IndexDefinition,
        statements: &mut Vec<String>,
    ) -> DatabaseResult<()> {
        self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
        if index.operator_class.is_some() || index.raw_expression.is_some() {
            return Err(DatabaseError::UnsupportedSchema(
                "PostgreSQL unique constraint expression or operator class",
            ));
        }
        let method = match index.algorithm.as_deref() {
            None => "",
            Some(method) if method.eq_ignore_ascii_case("btree") => " using btree",
            Some(_) => {
                return Err(DatabaseError::InvalidSchema(
                    "PostgreSQL unique index method",
                ));
            }
        };
        let default_name = self.conventional_name(table, &index.columns, "unique");
        let name = self.identifier(index.name.as_deref().unwrap_or(&default_name))?;
        let table = self.qualified(table)?;
        let columns = self.columns(&index.columns)?;
        let nulls = match index.nulls_not_distinct {
            Some(true) => " nulls not distinct",
            Some(false) => " nulls distinct",
            None => "",
        };
        let mut statement = format!("alter table {table} add constraint {name} unique");
        if index.online || !method.is_empty() {
            let concurrently = if index.online { "concurrently " } else { "" };
            statements.push(format!(
                "create unique index {concurrently}{name} on {table}{method} ({columns}){nulls}"
            ));
            write!(statement, " using index {name}").expect("writing to a String");
        } else {
            write!(statement, "{nulls} ({columns})").expect("writing to a String");
        }
        constraint_timing(&mut statement, index.deferrable, index.initially_immediate);
        statements.push(statement);
        Ok(())
    }

    fn drop_postgres_unique(&self, table: &Table, name: &str) -> DatabaseResult<String> {
        let qualified = self.qualified(table)?;
        let constraint = self.identifier(name)?;
        let target = postgres_string(&qualified);
        let index = postgres_string(name);
        // 早期迁移会为唯一约束创建独立索引，此处兼容这些索引。
        let body = format!(
            "declare index_schema text;
             begin
                 alter table {qualified} drop constraint {constraint};
             exception when undefined_object then
                 lock table {qualified} in access exclusive mode;
                 select ns.nspname into index_schema
                 from pg_catalog.pg_index as i
                 join pg_catalog.pg_class as c on c.oid = i.indexrelid
                 join pg_catalog.pg_namespace as ns on ns.oid = c.relnamespace
                 where i.indrelid = pg_catalog.to_regclass({target})
                       and i.indisunique and c.relname = cast({index} as pg_catalog.name);
                 if not found then raise; end if;
                 execute pg_catalog.format('drop index %I.%I', index_schema, {index});
             end;"
        );
        Ok(format!("do {}", postgres_string(&body)))
    }

    fn index_sql(&self, table: &Table, index: &IndexDefinition) -> DatabaseResult<String> {
        let (concurrently, online) = match (index.online, index.kind) {
            (false, _) => ("", ""),
            (true, IndexType::Index | IndexType::Unique) if self.sql_server() => {
                ("", " with (online = on)")
            }
            (true, kind)
                if self.postgres() && !matches!(kind, IndexType::Primary | IndexType::Spatial) =>
            {
                ("concurrently ", "")
            }
            (true, _) => return Err(DatabaseError::UnsupportedSchema("online index")),
        };
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
        let default;
        let name = match &index.name {
            Some(name) => name,
            None => {
                default = self.conventional_name(table, &index.columns, index.kind.suffix());
                &default
            }
        };
        if index.kind == IndexType::Vector {
            return self.vector_index(table, name, columns, index);
        }
        if index.operator_class.is_some() {
            return Err(DatabaseError::UnsupportedSchema(
                "operator class on non-vector index",
            ));
        }
        if self.postgres() && index.kind == IndexType::Index {
            self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
            let method = match index.algorithm.as_deref() {
                Some(method) => format!(" using {}", postgres_index_method(method)?),
                None => String::new(),
            };
            return Ok(format!(
                "create index {concurrently}{} on {}{method} ({columns})",
                self.identifier(name)?,
                self.qualified(table)?
            ));
        }
        let method = self.index_method(index)?;
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
                "create index {concurrently}{} on {} using gin (({vectors}))",
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
                "{verb} {concurrently}{} on {} ({columns}){online}",
                if self.postgres() || self.sql_server() {
                    self.identifier(name)?
                } else {
                    self.qualified_index(table, name)?
                },
                if self.postgres() || self.sql_server() {
                    self.qualified(table)?
                } else {
                    self.identifier(&table.name)?
                }
            ))
        }
    }

    fn vector_index(
        &self,
        table: &Table,
        name: &str,
        column: &str,
        index: &IndexDefinition,
    ) -> DatabaseResult<String> {
        let name = self.identifier(name)?;
        let table = self.qualified(table)?;
        if self.postgres() {
            self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
            let method = match index.algorithm.as_deref() {
                None | Some("hnsw") => "hnsw",
                Some("ivfflat") => "ivfflat",
                _ => {
                    return Err(DatabaseError::InvalidSchema(
                        "PostgreSQL vector index method",
                    ));
                }
            };
            let (schema, class) = Self::parse_schema_and_table(
                index
                    .operator_class
                    .as_deref()
                    .unwrap_or("vector_cosine_ops"),
            )?;
            let class = match schema {
                Some(schema) => format!("{}.{}", self.identifier(schema)?, self.identifier(class)?),
                None => self.identifier(class)?,
            };
            let concurrently = if index.online { "concurrently " } else { "" };
            Ok(format!(
                "create index {concurrently}{name} on {table} using {method} ({column} {class})"
            ))
        } else if self.mariadb() {
            if index.algorithm.is_some() {
                return Err(DatabaseError::UnsupportedSchema(
                    "MariaDB vector index method",
                ));
            }
            let (neighbors, distance) = maria_vector_options(index.operator_class.as_deref())?;
            let options =
                self.ddl_options(index.inplace.then_some("inplace"), index.lock.as_deref())?;
            Ok(format!(
                "alter table {table} add vector index {name} ({column}) M={neighbors} DISTANCE={distance}{options}"
            ))
        } else {
            Err(DatabaseError::UnsupportedSchema("vector index"))
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
        if !self.mysql() && !self.postgres() && !self.sql_server() {
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

    fn drop_foreign(&self, table: &str, name: &str) -> DatabaseResult<String> {
        let kind = if self.mysql() {
            "foreign key"
        } else if self.postgres() || self.sql_server() {
            "constraint"
        } else {
            return Err(DatabaseError::UnsupportedSchema("foreign key drop"));
        };
        Ok(format!(
            "alter table {table} drop {kind} {}",
            self.identifier(name)?
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
        if !self.mysql()
            && !self.postgres()
            && !self.sql_server()
            && target.schema.is_some()
            && target.schema != table.schema
        {
            return Err(DatabaseError::UnsupportedSchema(
                "SQLite cross-schema foreign key",
            ));
        }
        let reference = if self.mysql() || self.postgres() || self.sql_server() {
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
        if foreign.deferrable.is_some()
            || foreign.initially_immediate.is_some()
            || foreign.not_valid
        {
            if !self.postgres() {
                return Err(DatabaseError::UnsupportedSchema(
                    "PostgreSQL foreign key modifiers",
                ));
            }
            constraint_timing(&mut sql, foreign.deferrable, foreign.initially_immediate);
            if foreign.not_valid {
                sql.push_str(" not valid");
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
        let postgres = column.postgres.as_deref();
        let identity = postgres.and_then(|modifiers| modifiers.identity.as_ref());
        if let Some(modifiers) = postgres {
            if modifiers.using.is_some() {
                return Err(DatabaseError::UnsupportedSchema(
                    "USING requires a PostgreSQL column change",
                ));
            }
            if modifiers.drop_expression || (identity.is_some() && !self.postgres()) {
                return Err(DatabaseError::UnsupportedSchema(
                    "PostgreSQL column modifier",
                ));
            }
        }
        if column.rename_to.is_some() {
            return Err(DatabaseError::InvalidSchema("renameTo requires change"));
        }
        if !self.mysql()
            && (column.invisible
                || column.current_on_update
                || column.on_update.is_some()
                || column.instant
                || column.lock.is_some())
        {
            return Err(DatabaseError::UnsupportedSchema("MySQL column modifier"));
        }
        let computed = matches!(column.kind, ColumnType::Computed);
        let kind = if computed && self.sql_server() {
            String::new()
        } else {
            self.column_type(&column.kind, &column.name)?
        };
        let integer = matches!(
            column.kind,
            ColumnType::TinyInteger
                | ColumnType::SmallInteger
                | ColumnType::MediumInteger
                | ColumnType::Integer
                | ColumnType::BigInteger
        );
        let sql_server_change = self.sql_server() && column.change;
        if sql_server_change && column.generated.is_some() && !computed {
            return Err(DatabaseError::UnsupportedSchema(
                "SQL Server computed column change",
            ));
        }
        let default = if sql_server_change {
            None
        } else {
            self.column_default(column)
        };
        let nullable = column.nullable.unwrap_or(false);
        let increment = column.increment && !sql_server_change;
        if increment && (!integer || nullable || default.is_some()) {
            return Err(DatabaseError::InvalidSchema("auto increment column"));
        }
        let mut sql = format!("{} {kind}", self.identifier(&column.name)?);
        if self.sql_server() && increment {
            sql.push_str(" identity(1,1)");
        }
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
            if default.is_some()
                || column.increment
                || identity.is_some()
                || column.primary
                || column.current_on_update
                || column.on_update.is_some()
            {
                return Err(DatabaseError::InvalidSchema("generated column modifiers"));
            }
            if sql_server_change {
                let operation = if generated.stored { "add" } else { "drop" };
                return Ok(format!(
                    "{} {operation} persisted",
                    self.identifier(&column.name)?
                ));
            }
            let expression = self.generated_expression(generated)?;
            let mode = match (self.postgres(), generated.stored) {
                (true, true) => " stored",
                (true, false) => " virtual",
                (false, true) => " stored",
                (false, false) => "",
            };
            if self.sql_server() {
                let mode = if generated.stored { " persisted" } else { "" };
                return Ok(format!(
                    "{} as ({expression}){mode}",
                    self.identifier(&column.name)?
                ));
            }
            let generated = if self.postgres() {
                " generated always"
            } else {
                ""
            };
            write!(sql, "{generated} as ({expression}){mode}").expect("writing to a String");
            if !column.nullable.unwrap_or_else(|| self.mysql()) {
                sql.push_str(" not null");
            }
        } else {
            sql.push_str(if nullable { " null" } else { " not null" });
        }
        if let Some(default) = default {
            write!(
                sql,
                " default {}",
                self.default_value(default, &column.kind)?
            )
            .expect("writing to a String");
        }
        let on_update = match (
            &column.kind,
            column.current_on_update,
            column.on_update.as_deref(),
        ) {
            (_, false, None) => None,
            (ColumnType::DateTime(precision) | ColumnType::Timestamp(precision), true, _) => {
                Some(Cow::Owned(current_timestamp(*precision)))
            }
            (
                ColumnType::DateTime(_) | ColumnType::Timestamp(_) | ColumnType::Raw(_),
                false,
                Some(expression),
            ) => Some(Cow::Borrowed(schema_expression(
                expression,
                "column on-update expression",
            )?)),
            _ => return Err(DatabaseError::InvalidSchema("on-update timestamp column")),
        };
        if let Some(expression) = on_update {
            write!(sql, " on update {expression}").expect("writing to a String");
        }
        if column.invisible {
            sql.push_str(" invisible");
        }
        if self.postgres() && (identity.is_some() || column.increment) {
            sql.push(' ');
            sql.push_str(&self.postgres_identity(column)?);
            if !explicit_primary && (column.increment || column.primary) {
                sql.push_str(" primary key");
            }
        } else if increment {
            if self.mysql() {
                sql.push_str(" auto_increment");
                if !explicit_primary && (!column.change || column.primary) {
                    sql.push_str(" primary key");
                }
            } else if self.sql_server() {
                if !explicit_primary {
                    sql.push_str(" primary key");
                }
            } else {
                sql.push_str(" primary key autoincrement");
            }
        } else if column.primary && !sql_server_change {
            sql.push_str(" primary key");
        }
        if self.mysql()
            && let Some(comment) = &column.comment
        {
            write!(sql, " comment {}", self.string(comment)?).expect("writing to a String");
        }
        if self.mysql() {
            match &column.position {
                Some(Position::First) => sql.push_str(" first"),
                Some(Position::After(name)) => {
                    write!(sql, " after {}", self.identifier(name)?).expect("writing to a String");
                }
                None => {}
            }
        }
        Ok(sql)
    }

    fn sql_server_drop_column_constraints(
        &self,
        table: &str,
        columns: &[String],
        dropping_columns: bool,
    ) -> DatabaseResult<String> {
        let table_literal = self.string(table)?;
        let drop = self.string(&format!("alter table {table} drop constraint "))?;
        let columns = columns
            .iter()
            .map(|name| self.string(name))
            .collect::<DatabaseResult<Vec<_>>>()?
            .join(", ");
        let mut sql = format!(
            "declare @sql nvarchar(max) = N''; \
             select @sql += {drop} + quotename(d.name) + N';' \
             from sys.default_constraints d join sys.columns c \
                 on c.object_id = d.parent_object_id and c.column_id = d.parent_column_id \
             where d.parent_object_id = object_id({table_literal}) and c.name in ({columns}); "
        );
        if dropping_columns {
            write!(
                sql,
                "select @sql += {drop} + quotename(k.name) + N';' \
                 from sys.check_constraints k join sys.columns c \
                     on c.object_id = k.parent_object_id and c.column_id = k.parent_column_id \
                 where k.parent_object_id = object_id({table_literal}) and c.name in ({columns}); "
            )
            .expect("writing to a String");
        }
        sql.push_str("exec sys.sp_executesql @sql");
        Ok(sql)
    }

    fn postgres_identity(&self, column: &ColumnDefinition) -> DatabaseResult<String> {
        let integer = matches!(
            column.kind,
            ColumnType::TinyInteger
                | ColumnType::SmallInteger
                | ColumnType::MediumInteger
                | ColumnType::Integer
                | ColumnType::BigInteger
        );
        if !integer || column.nullable.unwrap_or(false) || self.column_default(column).is_some() {
            return Err(DatabaseError::InvalidSchema("identity column"));
        }
        let modifiers = column.postgres.as_deref();
        let identity = modifiers.and_then(|modifiers| modifiers.identity.as_ref());
        let mode = if identity.is_some() && modifiers.is_some_and(|modifiers| modifiers.always) {
            "always"
        } else {
            "by default"
        };
        let mut sql = format!("generated {mode} as identity");
        if let Some(Identity::Options(options)) = identity {
            let options = schema_expression(options, "identity sequence options")?;
            write!(sql, " ({options})").expect("writing to a String");
        }
        Ok(sql)
    }

    fn postgres_change(
        &self,
        table: &str,
        column: &ColumnDefinition,
        statements: &mut Vec<String>,
    ) -> DatabaseResult<()> {
        if column.rename_to.is_some()
            || column.generated.is_some()
            || column.charset.is_some()
            || column.invisible
            || column.current_on_update
            || column.on_update.is_some()
            || column.instant
            || column.lock.is_some()
            || column.start.is_some()
            || matches!(column.kind, ColumnType::Enum(_) | ColumnType::Set(_))
        {
            return Err(DatabaseError::UnsupportedSchema(
                "PostgreSQL column change modifier",
            ));
        }
        let name = self.identifier(&column.name)?;
        let kind = self.column_type(&column.kind, &column.name)?;
        let mut actions = vec![format!("alter column {name} type {kind}")];
        if let Some(collation) = &column.collation {
            actions[0].push_str(&format!(" collate {}", self.identifier(collation)?));
        }
        let modifiers = column.postgres.as_deref();
        let identity = modifiers.is_some_and(|modifiers| modifiers.identity.is_some());
        if let Some(using) = modifiers.and_then(|modifiers| modifiers.using.as_ref()) {
            actions[0].push_str(" using ");
            actions[0].push_str(schema_expression(using, "column conversion expression")?);
        }
        let nullability = if column.nullable.unwrap_or(false) {
            "drop not null"
        } else {
            "set not null"
        };
        actions.push(format!("alter column {name} {nullability}"));
        if modifiers.is_some_and(|modifiers| modifiers.drop_expression) {
            statements.push(format!(
                "alter table {table} alter column {name} drop expression if exists"
            ));
        }
        if !column.increment || identity {
            actions.push(format!("alter column {name} drop identity if exists"));
            let default = match self.column_default(column) {
                Some(DefaultValue::Value(Value::Null)) | None => "drop default".into(),
                Some(value) => format!("set default {}", self.default_value(value, &column.kind)?),
            };
            actions.push(format!("alter column {name} {default}"));
        }
        let comment = match column.comment.as_deref() {
            Some(comment) => self.string(comment)?,
            None => "null".into(),
        };
        statements.push(format!("alter table {table} {}", actions.join(", ")));
        if identity {
            let identity = self.postgres_identity(column)?;
            statements.push(format!(
                "alter table {table} alter column {name} add {identity}"
            ));
        }
        statements.push(format!("comment on column {table}.{name} is {comment}"));
        Ok(())
    }

    fn generated_expression(&self, generated: &GeneratedColumn) -> DatabaseResult<String> {
        let expression = generated.expression.trim();
        if expression.is_empty() || expression.contains('\0') {
            return Err(DatabaseError::InvalidSchema("generated column expression"));
        }
        if generated.json && expression.contains("->") {
            if self.postgres() {
                return self.grammar.postgres_json_selector(expression);
            }
            let (column, path) = self.grammar.json_selector(expression)?;
            let path = path.ok_or(DatabaseError::InvalidSchema("generated JSON column path"))?;
            if self.grammar.is_mariadb() || self.sql_server() {
                return Ok(format!("json_value({column}, {})", self.string(&path)?));
            }
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
        if matches!(kind, ColumnType::TsVector) {
            return if self.postgres() {
                Ok("tsvector".into())
            } else {
                Err(DatabaseError::UnsupportedSchema("tsvector column"))
            };
        }
        if let ColumnType::Vector(dimensions) = kind {
            if !self.postgres() && !self.mariadb() {
                return Err(DatabaseError::UnsupportedSchema("vector column"));
            }
            return match dimensions {
                Some(0) | None if self.mariadb() => {
                    Err(DatabaseError::InvalidSchema("MariaDB vector dimensions"))
                }
                Some(0) => Err(DatabaseError::InvalidSchema("vector dimensions")),
                Some(dimensions) => Ok(format!("vector({dimensions})")),
                None => Ok("vector".into()),
            };
        }
        if self.sql_server() {
            return sql_server_type(kind);
        }
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
                let fixed = matches!(kind, ColumnType::Char(_));
                let max = if mysql && fixed { 255 } else { 65_535 };
                if *length > max || (*length == 0 && !(mysql && fixed)) {
                    return Err(DatabaseError::InvalidSchema("column length"));
                }
                let name = if fixed { "char" } else { "varchar" };
                return Ok(if mysql || self.postgres() {
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
                return Ok(if mysql || self.postgres() {
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
            ColumnType::Computed => {
                return Err(DatabaseError::UnsupportedSchema(
                    "SQL Server computed column",
                ));
            }
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
            ColumnType::Uuid if self.mariadb_native_uuid => "uuid",
            ColumnType::Uuid if mysql => "char(36)",
            ColumnType::Uuid => "varchar",
            ColumnType::Spatial(subtype, srid) if mysql => {
                return Ok(if *srid == 0 {
                    (*subtype).into()
                } else {
                    if self.grammar.is_mariadb() {
                        format!("{subtype} ref_system_id={srid}")
                    } else {
                        format!("{subtype} srid {srid}")
                    }
                });
            }
            ColumnType::Spatial(_, _) => "geometry",
            ColumnType::Vector(_) => unreachable!("vector types handled above"),
            ColumnType::TsVector => unreachable!("tsvector handled above"),
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

    fn column_default<'a>(&self, column: &'a ColumnDefinition) -> Option<&'a DefaultValue> {
        let current = if self.mysql() {
            column.current_default != CurrentDefault::Unset
                && matches!(
                    column.kind,
                    ColumnType::Date
                        | ColumnType::Year
                        | ColumnType::DateTime(_)
                        | ColumnType::Timestamp(_)
                )
        } else {
            column.current_default == CurrentDefault::AfterExplicit
        };
        if current {
            Some(&DefaultValue::CurrentTimestamp)
        } else {
            column.default.as_ref()
        }
    }

    fn default_value(&self, value: &DefaultValue, kind: &ColumnType) -> DatabaseResult<String> {
        let value = match value {
            DefaultValue::Value(value) => value,
            DefaultValue::Expression(expression) => {
                return schema_expression(expression, "column default expression")
                    .map(str::to_owned);
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
            Value::Json(value) if !self.mysql() || self.grammar.is_mariadb() => {
                self.string(&value.to_string())
            }
            Value::Json(value) => Ok(format!(
                "(cast({} as json))",
                self.string(&value.to_string())?
            )),
            _ => Err(DatabaseError::InvalidSchema("default value")),
        }
    }

    fn current_default(&self, kind: &ColumnType) -> String {
        if self.sql_server() {
            return "sysutcdatetime()".into();
        }
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
        let national = if self.sql_server() { "N" } else { "" };
        Ok(format!("{national}'{}'", value.replace('\'', "''")))
    }
}

fn sql_server_type(kind: &ColumnType) -> DatabaseResult<String> {
    Ok(match kind {
        ColumnType::TinyInteger => "tinyint".into(),
        ColumnType::SmallInteger => "smallint".into(),
        ColumnType::Integer | ColumnType::MediumInteger | ColumnType::Year => "int".into(),
        ColumnType::BigInteger => "bigint".into(),
        ColumnType::String(1..=4000) | ColumnType::Char(1..=4000) => match kind {
            ColumnType::String(length) => format!("nvarchar({length})"),
            ColumnType::Char(length) => format!("nchar({length})"),
            _ => unreachable!(),
        },
        ColumnType::TinyText
        | ColumnType::Text
        | ColumnType::MediumText
        | ColumnType::LongText
        | ColumnType::Json
        | ColumnType::Jsonb => "nvarchar(max)".into(),
        ColumnType::Boolean => "bit".into(),
        ColumnType::Decimal(total, places) if *total > 0 && *total <= 38 && places <= total => {
            format!("decimal({total},{places})")
        }
        ColumnType::Float(0) | ColumnType::Double => "float".into(),
        ColumnType::Float(precision @ 1..=53) => format!("float({precision})"),
        ColumnType::Binary => "varbinary(max)".into(),
        ColumnType::SizedBinary(length @ 1..=8000, fixed) => {
            let name = if *fixed { "binary" } else { "varbinary" };
            format!("{name}({length})")
        }
        ColumnType::IpAddress => "nvarchar(45)".into(),
        ColumnType::MacAddress => "nvarchar(17)".into(),
        ColumnType::Uuid => "uniqueidentifier".into(),
        ColumnType::Date => "date".into(),
        ColumnType::DateTime(precision @ 0..=6) | ColumnType::Timestamp(precision @ 0..=6) => {
            format!("datetime2({precision})")
        }
        ColumnType::Time(precision @ 0..=6) => format!("time({precision})"),
        ColumnType::Raw(definition)
            if !definition.trim().is_empty() && !definition.contains('\0') =>
        {
            definition.clone()
        }
        _ => return Err(DatabaseError::UnsupportedSchema("SQL Server column type")),
    })
}

fn maria_vector_options(options: Option<&str>) -> DatabaseResult<(u16, &'static str)> {
    let invalid = || DatabaseError::InvalidSchema("MariaDB vector index options");
    let mut neighbors = None;
    let mut distance = None;
    for option in options.unwrap_or_default().split_ascii_whitespace() {
        let (name, value) = option.split_once('=').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("M") && neighbors.is_none() {
            neighbors = Some(
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| (3..=200).contains(value))
                    .ok_or_else(invalid)?,
            );
        } else if name.eq_ignore_ascii_case("DISTANCE") && distance.is_none() {
            distance = Some(if value.eq_ignore_ascii_case("cosine") {
                "cosine"
            } else if value.eq_ignore_ascii_case("euclidean") {
                "euclidean"
            } else {
                return Err(invalid());
            });
        } else {
            return Err(invalid());
        }
    }
    Ok((neighbors.unwrap_or(6), distance.unwrap_or("cosine")))
}

fn primary_for_new_auto_column(column: &ColumnDefinition, command: &Command) -> bool {
    !column.change
        && column.increment
        && matches!(command, Command::Index(index)
            if index.kind == IndexType::Primary && index.columns.contains(&column.name))
}

fn schema_expression<'a>(
    expression: &'a Expression,
    label: &'static str,
) -> DatabaseResult<&'a str> {
    expression
        .unbound_sql()
        .filter(|sql| !sql.trim().is_empty() && !sql.contains('\0'))
        .ok_or(DatabaseError::InvalidSchema(label))
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

fn postgres_string(value: &str) -> String {
    format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

fn postgres_index_method(method: &str) -> DatabaseResult<&str> {
    let identifier_start =
        |byte: u8| byte.is_ascii_alphabetic() || byte == b'_' || !byte.is_ascii();
    let mut bytes = method.bytes();
    if !bytes.next().is_some_and(identifier_start)
        || !bytes.all(|byte| identifier_start(byte) || byte.is_ascii_digit() || byte == b'$')
    {
        return Err(DatabaseError::InvalidSchema("PostgreSQL index method"));
    }
    Ok(method)
}

fn constraint_timing(
    sql: &mut String,
    deferrable: Option<bool>,
    initially_immediate: Option<bool>,
) {
    if let Some(deferrable) = deferrable {
        sql.push_str(if deferrable {
            " deferrable"
        } else {
            " not deferrable"
        });
        if deferrable && let Some(immediate) = initially_immediate {
            sql.push_str(if immediate {
                " initially immediate"
            } else {
                " initially deferred"
            });
        }
    }
}

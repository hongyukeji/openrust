use crate::database::{DatabaseError, DatabaseResult, Row, Transaction, Value};

use crate::database::schema::{
    Blueprint,
    blueprint::Command,
    grammars::{Grammar, Table},
    index_definition::IndexType,
};

mod sql;
pub(super) fn generated_expressions(
    create_sql: &str,
) -> DatabaseResult<impl Iterator<Item = (String, String)>> {
    let mut expressions = Vec::new();
    for column in sql::parse_table(create_sql)?.columns {
        if let Some(expression) = sql::generation_expression(column)? {
            expressions.push((sql::column_name(column)?, expression.into()));
        }
    }
    Ok(expressions.into_iter())
}

pub(super) fn column_collations(create_sql: &str) -> DatabaseResult<Vec<(String, String)>> {
    if !create_sql
        .as_bytes()
        .windows(7)
        .any(|part| part.eq_ignore_ascii_case(b"collate"))
    {
        return Ok(Vec::new());
    }
    let definition = match sql::parse_table(create_sql) {
        Ok(definition) => definition,
        Err(DatabaseError::UnsupportedSchema("SQLite virtual table rebuild")) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let mut collations = Vec::new();
    for column in definition.columns {
        if let Some(collation) = sql::column_collation(column)? {
            collations.push((sql::column_name(column)?, collation));
        }
    }
    Ok(collations)
}

pub(super) fn requires_catalog(command: &Command) -> bool {
    match command {
        Command::Column(column) => column.change,
        Command::Index(index) => index.kind == IndexType::Primary,
        Command::Foreign(_)
        | Command::DropForeign(_)
        | Command::DropForeignColumns(_)
        | Command::DropIndex(IndexType::Primary, _)
        | Command::RenameIndex(_, _) => true,
        _ => false,
    }
}

pub(super) async fn alter(
    transaction: &mut Transaction,
    grammar: Grammar,
    table: Table,
    blueprint: Blueprint,
    foreign_keys: bool,
) -> DatabaseResult<()> {
    if blueprint.temporary {
        return Err(DatabaseError::InvalidSchema(
            "temporary modifier during alter",
        ));
    }
    let mut rebuild = Vec::new();
    for command in blueprint.commands {
        if requires_catalog(&command) && !matches!(command, Command::RenameIndex(_, _)) {
            rebuild.push(command);
            continue;
        }
        if !rebuild.is_empty() {
            rebuild_table(transaction, &grammar, &table, std::mem::take(&mut rebuild)).await?;
        }
        match command {
            Command::RenameIndex(from, to) => {
                rename_index(transaction, &grammar, &table, &from, &to).await?;
            }
            command => {
                for sql in grammar.alter(
                    &table,
                    Blueprint {
                        commands: vec![command],
                        ..Blueprint::default()
                    },
                )? {
                    transaction.statement(&sql, &[]).await?;
                }
            }
        }
    }
    if !rebuild.is_empty() {
        rebuild_table(transaction, &grammar, &table, rebuild).await?;
    }
    if foreign_keys {
        let schema = grammar.identifier(grammar.catalog_schema(&table))?;
        if transaction
            .selectOne(&format!("pragma {schema}.foreign_key_check"), &[])
            .await?
            .is_some()
        {
            return Err(DatabaseError::InvalidSchema("SQLite foreign key check"));
        }
    }
    Ok(())
}

async fn rename_index(
    transaction: &mut Transaction,
    grammar: &Grammar,
    table: &Table,
    from: &str,
    to: &str,
) -> DatabaseResult<()> {
    let schema = grammar.identifier(grammar.catalog_schema(table))?;
    let row = transaction.selectOne(
        &format!("select sql from {schema}.sqlite_schema where type = 'index' and tbl_name = ? collate nocase and name = ? collate nocase"),
        &[table.name.as_str().into(), from.into()],
    ).await?.ok_or(DatabaseError::InvalidSchema("unknown SQLite index"))?;
    let sql = text(&row, "sql")
        .map_err(|_| DatabaseError::UnsupportedSchema("SQLite implicit index rename"))?;
    let new = format!("{schema}.{}", grammar.identifier(to)?);
    let recreate = sql::qualify_object(sql, "index", &new)?;
    transaction
        .statement(
            &format!("drop index {schema}.{}", grammar.identifier(from)?),
            &[],
        )
        .await?;
    transaction.statement(&recreate, &[]).await?;
    Ok(())
}

async fn rebuild_table(
    transaction: &mut Transaction,
    grammar: &Grammar,
    table: &Table,
    commands: Vec<Command>,
) -> DatabaseResult<()> {
    let schema_name = grammar.catalog_schema(table);
    let schema = grammar.identifier(schema_name)?;
    let name = grammar.identifier(&table.name)?;
    let original = format!("{schema}.{name}");
    let table_sql = transaction.selectOne(
        &format!("select sql from {schema}.sqlite_schema where type = 'table' and name = ? collate nocase"),
        &[table.name.as_str().into()],
    ).await?.ok_or(DatabaseError::InvalidSchema("unknown SQLite table"))?;
    let definition = sql::parse_table(text(&table_sql, "sql")?)?;
    let mut columns: Vec<_> = definition.columns.into_iter().map(str::to_owned).collect();
    let mut constraints: Vec<_> = definition
        .constraints
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut new_indexes = Vec::new();
    for command in commands {
        match command {
            Command::Column(mut column) => {
                if column.rename_to.is_some() {
                    return Err(DatabaseError::UnsupportedSchema("SQLite renameTo modifier"));
                }
                let mut found = None;
                for (index, definition) in columns.iter().enumerate() {
                    if sql::column_name(definition)?.eq_ignore_ascii_case(&column.name) {
                        found = Some(index);
                        break;
                    }
                }
                let index = found.ok_or(DatabaseError::InvalidSchema("unknown SQLite column"))?;
                let constraints = sql::inline_constraints(&columns[index])?;
                let requested_increment = column.increment;
                if constraints
                    .iter()
                    .any(|constraint| constraint.kind == sql::InlineKind::Primary)
                {
                    if requested_increment {
                        grammar.column(&column)?;
                    }
                    column.primary = false;
                    column.increment = false;
                }
                if constraints
                    .iter()
                    .any(|constraint| constraint.kind == sql::InlineKind::Unique)
                {
                    column.unique = false;
                }
                let mut definition = grammar.column(&column)?;
                for constraint in constraints {
                    definition.push(' ');
                    definition.push_str(&columns[index][constraint.range]);
                    if constraint.kind == sql::InlineKind::Primary
                        && requested_increment
                        && !constraint.autoincrement
                    {
                        definition.push_str(" autoincrement");
                    }
                }
                columns[index] = definition;
                for (enabled, kind) in [
                    (column.unique, IndexType::Unique),
                    (column.indexed, IndexType::Index),
                    (column.spatial_indexed, IndexType::Spatial),
                ] {
                    if enabled {
                        new_indexes.push(Command::Index(
                            crate::database::schema::IndexDefinition::new(
                                kind,
                                &[&column.name],
                                None,
                            ),
                        ));
                    }
                }
            }
            Command::Foreign(foreign) => {
                if foreign.inplace || foreign.lock.is_some() {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite foreign key modifiers",
                    ));
                }
                constraints.push(grammar.foreign(table, &foreign)?);
            }
            Command::DropForeign(name) => {
                remove_foreign(&mut columns, &mut constraints, Some(&name), &[])?;
            }
            Command::DropForeignColumns(names) => {
                remove_foreign(&mut columns, &mut constraints, None, &names)?;
            }
            Command::Index(index) => {
                if index.algorithm.is_some() || index.inplace || index.lock.is_some() {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite primary key modifiers",
                    ));
                }
                constraints.push(format!(
                    "primary key ({})",
                    grammar.columns(&index.columns)?
                ));
            }
            Command::DropIndex(IndexType::Primary, _) => {
                let mut removed = 0;
                for definition in &mut columns {
                    removed +=
                        remove_inline(definition, |key| key.kind == sql::InlineKind::Primary)?;
                }
                if removed == 0 {
                    remove_constraint(&mut constraints, sql::primary)?;
                }
            }
            _ => unreachable!("only table rebuild operations are queued"),
        }
    }

    let objects = transaction.select(
        &format!("select type, name, sql from {schema}.sqlite_schema where tbl_name = ? collate nocase and type in ('index', 'trigger') and sql is not null order by type, name"),
        &[table.name.as_str().into()],
    ).await?;
    let sequence = read_sequence(transaction, &schema, &table.name).await?;
    let temporary_name = format!("__temp__{:016x}", fastrand::u64(..));
    let temporary = format!("{schema}.{}", grammar.identifier(&temporary_name)?);
    columns.extend(constraints);
    let create = format!(
        "create table {temporary} ({}){}",
        columns.join("\n, "),
        definition.suffix
    );
    let restore_sequence = sequence.is_some() && sql::autoincrement_column(&create)?.is_some();
    transaction.statement(&create, &[]).await?;
    let copy = transaction
        .select(
            "select name from pragma_table_xinfo(?, ?) where hidden = 0 order by cid",
            &[temporary_name.into(), schema_name.into()],
        )
        .await?
        .iter()
        .map(|row| grammar.identifier(text(row, "name")?))
        .collect::<DatabaseResult<Vec<_>>>()?
        .join(", ");
    transaction
        .statement(
            &format!("insert into {temporary} ({copy}) select {copy} from {original}"),
            &[],
        )
        .await?;
    transaction
        .statement(&format!("drop table {original}"), &[])
        .await?;
    transaction
        .statement(&format!("alter table {temporary} rename to {name}"), &[])
        .await?;

    for object in &objects {
        let object_name = format!("{schema}.{}", grammar.identifier(text(object, "name")?)?);
        let sql = sql::qualify_object(text(object, "sql")?, text(object, "type")?, &object_name)?;
        transaction.statement(&sql, &[]).await?;
    }
    if restore_sequence && let Some(sequence) = sequence {
        let name = Value::from(table.name.as_str());
        let updated = transaction.update(
            &format!("update {schema}.sqlite_sequence set seq = max(seq, ?) where name = ? collate nocase"),
            &[sequence.clone(), name.clone()],
        ).await?;
        if updated == 0 {
            transaction
                .insert(
                    &format!("insert into {schema}.sqlite_sequence (name, seq) values (?, ?)"),
                    &[name, sequence],
                )
                .await?;
        }
    }
    for sql in grammar.alter(
        table,
        Blueprint {
            commands: new_indexes,
            ..Blueprint::default()
        },
    )? {
        transaction.statement(&sql, &[]).await?;
    }
    Ok(())
}

async fn read_sequence(
    transaction: &mut Transaction,
    schema: &str,
    table: &str,
) -> DatabaseResult<Option<Value>> {
    let exists = transaction.selectOne(&format!("select 1 from {schema}.sqlite_schema where type = 'table' and name = 'sqlite_sequence'"), &[]).await?.is_some();
    if !exists {
        return Ok(None);
    }
    let sequence = transaction
        .scalar(
            &format!("select seq from {schema}.sqlite_sequence where name = ? collate nocase"),
            &[table.into()],
        )
        .await?;
    Ok((sequence != Value::Null).then_some(sequence))
}

fn remove_foreign(
    definitions: &mut [String],
    constraints: &mut Vec<String>,
    name: Option<&str>,
    columns: &[String],
) -> DatabaseResult<()> {
    let mut removed = 0;
    let mut index = 0;
    while index < constraints.len() {
        if sql::foreign_matches(&constraints[index], name, columns)? {
            constraints.remove(index);
            removed += 1;
        } else {
            index += 1;
        }
    }
    for definition in definitions {
        let column = sql::column_name(definition)?;
        removed += remove_inline(definition, |constraint| {
            constraint.kind == sql::InlineKind::Foreign
                && match name {
                    Some(name) => constraint
                        .name
                        .as_deref()
                        .is_some_and(|actual| actual.eq_ignore_ascii_case(name)),
                    None => matches!(columns, [wanted] if column.eq_ignore_ascii_case(wanted)),
                }
        })?;
    }
    if removed == 0 {
        return Err(DatabaseError::InvalidSchema("unknown SQLite foreign key"));
    }
    Ok(())
}

fn remove_inline(
    column: &mut String,
    mut matches: impl FnMut(&sql::InlineConstraint) -> bool,
) -> DatabaseResult<usize> {
    let mut removed = 0;
    for constraint in sql::inline_constraints(column)?.into_iter().rev() {
        if matches(&constraint) {
            column.replace_range(constraint.range, "");
            removed += 1;
        }
    }
    Ok(removed)
}

fn remove_constraint(
    constraints: &mut Vec<String>,
    mut matches: impl FnMut(&str) -> DatabaseResult<bool>,
) -> DatabaseResult<()> {
    for (index, sql) in constraints.iter().enumerate() {
        if matches(sql)? {
            constraints.remove(index);
            return Ok(());
        }
    }
    Err(DatabaseError::UnsupportedSchema(
        "SQLite constraint absent or defined inline",
    ))
}

fn text<'a>(row: &'a Row, name: &str) -> DatabaseResult<&'a str> {
    match row.get(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(DatabaseError::InvalidSchema("SQLite catalog text")),
    }
}

use crate::database::{DatabaseError, DatabaseResult, Row, Transaction, Value};

use crate::database::schema::{
    Blueprint,
    blueprint::Command,
    grammars::{Grammar, Table},
    index_definition::IndexType,
};

mod sql;
pub(super) fn autoincrement_column(create_sql: &str) -> DatabaseResult<Option<String>> {
    sql::autoincrement_column(create_sql)
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
                        temporary: false,
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
            Command::Column(column) => {
                let mut found = None;
                for (index, definition) in columns.iter().enumerate() {
                    if sql::column_name(definition)?.eq_ignore_ascii_case(&column.name) {
                        found = Some(index);
                        break;
                    }
                }
                let index = found.ok_or(DatabaseError::InvalidSchema("unknown SQLite column"))?;
                if sql::constrained_column(&columns[index])? {
                    return Err(DatabaseError::UnsupportedSchema(
                        "SQLite change of an inline constrained column",
                    ));
                }
                columns[index] = grammar.column(&column)?;
                for (enabled, kind) in [
                    (column.unique, IndexType::Unique),
                    (column.indexed, IndexType::Index),
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
                remove_constraint(&mut constraints, |sql| {
                    sql::foreign_matches(sql, Some(&name), &[])
                })?;
            }
            Command::DropForeignColumns(columns) => {
                remove_constraint(&mut constraints, |sql| {
                    sql::foreign_matches(sql, None, &columns)
                })?;
            }
            Command::Index(index) => {
                if index.inplace || index.lock.is_some() {
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
                remove_constraint(&mut constraints, sql::primary)?;
            }
            _ => unreachable!("only table rebuild operations are queued"),
        }
    }

    let objects = transaction.select(
        &format!("select type, name, sql from {schema}.sqlite_schema where tbl_name = ? collate nocase and type in ('index', 'trigger') and sql is not null order by type, name"),
        &[table.name.as_str().into()],
    ).await?;
    let copy = transaction
        .select(
            "select name from pragma_table_xinfo(?, ?) where hidden = 0 order by cid",
            &[table.name.as_str().into(), schema_name.into()],
        )
        .await?
        .iter()
        .map(|row| grammar.identifier(text(row, "name")?))
        .collect::<DatabaseResult<Vec<_>>>()?
        .join(", ");
    let sequence = read_sequence(transaction, &schema, &table.name).await?;
    let temporary = format!(
        "{schema}.{}",
        grammar.identifier(&format!("__temp__{:016x}", fastrand::u64(..)))?
    );
    columns.extend(constraints);
    transaction
        .statement(
            &format!(
                "create table {temporary} ({}){}",
                columns.join("\n, "),
                definition.suffix
            ),
            &[],
        )
        .await?;
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
    if let Some(sequence) = sequence {
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
            temporary: false,
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

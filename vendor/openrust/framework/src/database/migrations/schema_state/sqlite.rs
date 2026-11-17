use std::{
    fs::{self, File},
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use crate::database::{Connection, DatabaseError, DatabaseResult, Value};

const OBJECTS: &str = "select sql from main.sqlite_schema as objects
    where sql is not null and substr(name, 1, 7) <> 'sqlite_'
      and not exists (select 1 from pragma_table_list
          where schema = 'main' and type = 'shadow' and name = objects.name)
    order by case type when 'table' then 0 when 'index' then 1
        when 'view' then 2 else 3 end, rowid";

pub(super) async fn dump(
    connection: &Connection,
    path: &Path,
    migration_table: Option<&str>,
    output: &mut File,
) -> DatabaseResult<()> {
    check_output_path(connection, path)?;
    let ledger = migration_table
        .map(|table| connection.grammar.table(table))
        .transpose()?;
    let statements = connection
        .transaction(
            move |transaction| {
                let ledger = ledger.clone();
                Box::pin(async move {
                    let mut statements = transaction.select(OBJECTS, &[]).await?;
                    if let Some(table) = ledger {
                        let insert = format!("INSERT INTO {table} (id, migration, batch) VALUES (");
                        let query = format!(
                            "select ? || quote(id) || ', ' || quote(migration) || ', ' ||
                             quote(batch) || ')' from {table} order by id"
                        );
                        statements.extend(transaction.select(&query, &[insert.into()]).await?);
                    }
                    Ok(statements)
                })
            },
            1,
        )
        .await?;
    for row in statements {
        let Some(Value::Text(sql)) = row.get_at(0) else {
            return Err(DatabaseError::InvalidSchema("SQLite snapshot statement"));
        };
        writeln!(output, "{};", sql.trim_end_matches(';')).map_err(DatabaseError::SchemaIo)?;
    }
    Ok(())
}

pub(super) async fn load(
    connection: &Connection,
    path: &Path,
    empty_ledger: Option<&str>,
) -> DatabaseResult<()> {
    let sql = fs::read_to_string(path).map_err(DatabaseError::SchemaIo)?;
    let ledger = empty_ledger
        .map(|table| connection.grammar.table(table))
        .transpose()?;
    connection
        .sqlite_schema_transaction(|transaction, _| {
            Box::pin(async move {
                if let Some(table) = ledger {
                    transaction
                        .statement(&format!("DROP TABLE {table}"), &[])
                        .await?;
                }
                transaction.unprepared(&sql).await?;
                Ok(())
            })
        })
        .await
}

fn check_output_path(connection: &Connection, path: &Path) -> DatabaseResult<()> {
    if connection.getDatabaseName() == ":memory:" {
        return Ok(());
    }
    let database = comparison_path(Path::new(connection.getDatabaseName()))?;
    let output = comparison_path(path)?;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut reserved = database.as_os_str().to_os_string();
        reserved.push(suffix);
        if output.as_path() == Path::new(&reserved) {
            return Err(DatabaseError::InvalidSchema(
                "snapshot path is a SQLite database file",
            ));
        }
    }
    Ok(())
}

fn comparison_path(path: &Path) -> DatabaseResult<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let file = path
                .file_name()
                .ok_or(DatabaseError::InvalidSchema("SQLite snapshot path"))?;
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()
                .map(|parent| parent.join(file))
                .map_err(DatabaseError::SchemaIo)
        }
        Err(error) => Err(DatabaseError::SchemaIo(error)),
    }
}

use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{Blueprint, Builder, Executor, Table};
use crate::database::schema::{blueprint::Command, column_definition::ColumnType};

pub(super) fn has_rename(blueprint: &Blueprint) -> bool {
    blueprint
        .commands
        .iter()
        .any(|command| matches!(command, Command::RenameColumn(..)))
}

impl Builder<'_> {
    pub(super) async fn mariadb_version_for(
        &mut self,
        blueprint: &Blueprint,
    ) -> DatabaseResult<Option<(u32, u32, u32)>> {
        if !self.grammar.mariadb() || matches!(self.executor, Executor::Pretend(_)) {
            return Ok(None);
        }
        let uses_uuid = blueprint.commands.iter().any(
            |command| matches!(command, Command::Column(column) if matches!(column.kind, ColumnType::Uuid)),
        );
        if !uses_uuid && !has_rename(blueprint) {
            return Ok(None);
        }
        let rows = self.select("select version()", &[]).await?;
        let Some(Value::Text(version)) = rows.first().and_then(|row| row.get_at(0)) else {
            return Err(DatabaseError::InvalidSchema("MariaDB server version"));
        };
        let version = version.strip_prefix("5.5.5-").unwrap_or(version);
        let mut parts = version.split(['.', '-']);
        let mut component = || {
            parts
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or(DatabaseError::InvalidSchema("MariaDB server version"))
        };
        let version = (component()?, component()?, component()?);
        self.grammar.use_native_mariadb_uuid(version >= (10, 7, 0));
        Ok(Some(version))
    }

    pub(super) async fn mariadb_legacy_alter(
        &mut self,
        table: &Table,
        blueprint: Blueprint,
    ) -> DatabaseResult<()> {
        let mut pending = Blueprint::default();
        let qualified = self.grammar.qualified(table)?;
        for command in blueprint.commands {
            let Command::RenameColumn(from, to) = command else {
                pending.commands.push(command);
                continue;
            };
            self.execute(self.grammar.alter(table, std::mem::take(&mut pending))?)
                .await?;
            let from = self.grammar.identifier(&from)?;
            let to = self.grammar.identifier(&to)?;
            let rows = self
                .select(&format!("show create table {qualified}"), &[])
                .await?;
            let Some(Value::Text(create)) = rows.first().and_then(|row| row.get_at(1)) else {
                return Err(DatabaseError::InvalidSchema("MariaDB table definition"));
            };
            let definition = column_definition(create, &from)
                .ok_or(DatabaseError::InvalidSchema("MariaDB column definition"))?;
            self.execute(vec![format!(
                "alter table {qualified} change column {from} {to} {definition}"
            )])
            .await?;
        }
        self.execute(self.grammar.alter(table, pending)?).await
    }
}

fn column_definition<'a>(create: &'a str, name: &str) -> Option<&'a str> {
    let mut depth = 0;
    let mut start = 0;
    let mut quoted = None;
    let mut bytes = create.bytes().enumerate();
    while let Some((index, byte)) = bytes.next() {
        if let Some(delimiter) = quoted {
            if byte == b'\\' && delimiter != b'`' {
                bytes.next();
            } else if byte == delimiter {
                quoted = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' | b'`' => quoted = Some(byte),
            b'(' => {
                if depth == 0 {
                    start = index + 1;
                }
                depth += 1;
            }
            b',' | b')' if depth == 1 => {
                if let Some(definition) = create[start..index].trim().strip_prefix(name)
                    && definition.starts_with(char::is_whitespace)
                {
                    return Some(definition.trim());
                }
                if byte == b')' {
                    return None;
                }
                start = index + 1;
            }
            b')' => depth -= 1,
            _ => {}
        }
    }
    None
}

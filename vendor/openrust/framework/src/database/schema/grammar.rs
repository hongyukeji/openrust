//! 仅name投影；保固定catalog关联及列序，不冒充完整getColumns。
use super::{DbBackend, parameter_error};
use crate::{Result, database::DatabaseTableNames};
use sea_orm::{Statement, Value};

pub(super) struct ColumnQuery {
    pub(super) columns: Statement,
    pub(super) definition: Option<Statement>,
}
pub(super) fn parse(reference: &str) -> Result<(Option<&str>, &str)> {
    let mut parts = reference.split('.');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some() {
        return Err(parameter_error(
            "schema_reference_invalid",
            "Schema引用最多包含schema和table两段",
        ));
    }
    Ok(second.map_or((None, first), |table| (Some(first), table)))
}
fn sqlite_schema(schema: Option<&str>) -> Result<String> {
    let schema = schema.unwrap_or("main");
    if schema == "*" || schema.contains('\0') {
        return Err(parameter_error(
            "schema_identifier_invalid",
            "SQLite schema不可为星号或包含NUL",
        ));
    }
    Ok(format!("\"{}\"", schema.replace('"', "\"\"")))
}
pub(super) fn compile(
    backend: DbBackend,
    names: &DatabaseTableNames,
    reference: &str,
) -> Result<ColumnQuery> {
    let (schema, logical) = parse(reference)?;
    let table = names.catalog_name(logical);
    let explicit = schema.filter(|name| !name.is_empty() && *name != "0");
    let (sql, values, definition): (String, Vec<Value>, Option<Statement>) = match backend {
        DbBackend::MySql => {
            let (schema_sql, mut values) = explicit.map_or_else(
                || ("schema()", Vec::new()),
                |schema| ("?", vec![schema.into()]),
            );
            values.push(table.into());
            (
                format!(
                    "select column_name as name from information_schema.columns where table_schema = {schema_sql} and table_name = ? order by ordinal_position asc"
                ),
                values,
                None,
            )
        }
        DbBackend::Postgres => {
            let (schema_sql, values) = explicit.map_or_else(
                || ("current_schema()", vec![table.clone().into()]),
                |schema| ("$2", vec![table.clone().into(), schema.into()]),
            );
            (
                format!(
                    "select a.attname as name from pg_attribute a, pg_class c, pg_type t, pg_namespace n where c.relname = $1 and n.nspname = {schema_sql} and a.attnum > 0 and a.attrelid = c.oid and a.atttypid = t.oid and n.oid = c.relnamespace order by a.attnum"
                ),
                values,
                None,
            )
        }
        DbBackend::Sqlite => {
            if table.contains('\0') {
                return Err(parameter_error(
                    "schema_identifier_invalid",
                    "SQLite table不可包含NUL",
                ));
            }
            let quoted = sqlite_schema(schema)?;
            let definition = Statement::from_sql_and_values(
                backend,
                format!("select \"sql\" from {quoted}.sqlite_master where type = ? and name = ?"),
                ["table".into(), table.clone().into()],
            );
            (
                "select name from pragma_table_xinfo(?, ?) order by cid asc".into(),
                vec![table.into(), schema.unwrap_or("main").into()],
                Some(definition),
            )
        }
        _ => {
            return Err(parameter_error(
                "schema_backend_unsupported",
                "尚未实现所选Schema后端列清单",
            ));
        }
    };
    Ok(ColumnQuery {
        columns: Statement::from_sql_and_values(backend, sql, values),
        definition,
    })
}

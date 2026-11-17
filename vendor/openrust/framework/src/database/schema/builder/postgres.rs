use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseResult, Row, Transaction, Value, query::grammars::Grammar,
};

const RELATIONS: &str = "
    select n.nspname::text as schema, c.relname::text as name
    from pg_catalog.pg_class c join pg_catalog.pg_namespace n on n.oid = c.relnamespace
    where n.nspname = any(pg_catalog.current_schemas(false))
      and n.nspname !~ '^pg_' and n.nspname <> 'information_schema'
      and c.relkind::text in (?, ?)
    order by n.nspname, c.relname";

const TYPES: &str = "
    select n.nspname::text as schema, t.typname::text as name, (t.typtype = 'd') as domain
    from pg_catalog.pg_type t join pg_catalog.pg_namespace n on n.oid = t.typnamespace
    left join pg_catalog.pg_class c on c.oid = t.typrelid
    left join pg_catalog.pg_type el on el.oid = t.typelem
    left join pg_catalog.pg_class ce on ce.oid = el.typrelid
    where n.nspname = any(pg_catalog.current_schemas(false))
      and n.nspname !~ '^pg_' and n.nspname <> 'information_schema'
      and ((t.typrelid = 0 and (ce.relkind = 'c' or ce.relkind is null)) or c.relkind = 'c')
      and not (t.typinput = 'pg_catalog.array_in'::pg_catalog.regproc
           and t.typoutput = 'pg_catalog.array_out'::pg_catalog.regproc)
      and t.typtype <> 'm'
      and not exists (
          select 1 from pg_catalog.pg_depend d
          where d.classid = 'pg_catalog.pg_type'::pg_catalog.regclass
            and d.objid in (t.oid, t.typelem) and d.deptype = 'e'
      )
    order by n.nspname, t.typname";

pub(super) async fn drop_schema_objects(
    connection: &Connection,
    drop_tables: bool,
    drop_views: bool,
    drop_types: bool,
) -> DatabaseResult<()> {
    let grammar = connection.grammar.clone();
    let excluded = Arc::clone(&connection.postgres_dont_drop);
    connection
        .transaction(
            move |transaction| {
                let grammar = grammar.clone();
                let excluded = Arc::clone(&excluded);
                Box::pin(async move {
                    if drop_views {
                        let views = transaction
                            .select(RELATIONS, &["v".into(), "v".into()])
                            .await?;
                        drop_group(transaction, &grammar, "view", &views, &[]).await?;
                    }
                    if drop_tables {
                        let tables = transaction
                            .select(RELATIONS, &["r".into(), "p".into()])
                            .await?;
                        drop_group(transaction, &grammar, "table", &tables, &excluded).await?;
                    }
                    if drop_types {
                        let (domains, types): (Vec<_>, Vec<_>) = transaction
                            .select(TYPES, &[])
                            .await?
                            .into_iter()
                            .partition(|row| row.get("domain") == Some(&Value::Bool(true)));
                        drop_group(transaction, &grammar, "type", &types, &[]).await?;
                        drop_group(transaction, &grammar, "domain", &domains, &[]).await?;
                    }
                    Ok(())
                })
            },
            1,
        )
        .await
}

async fn drop_group(
    transaction: &mut Transaction,
    grammar: &Grammar,
    kind: &str,
    rows: &[Row],
    excluded: &[String],
) -> DatabaseResult<()> {
    if let Some(sql) = drop_statement(grammar, kind, rows, excluded)? {
        transaction.statement(&sql, &[]).await?;
    }
    Ok(())
}

fn drop_statement(
    grammar: &Grammar,
    kind: &str,
    rows: &[Row],
    excluded: &[String],
) -> DatabaseResult<Option<String>> {
    let mut targets = Vec::new();
    for row in rows {
        let (Some(Value::Text(schema)), Some(Value::Text(name))) =
            (row.get("schema"), row.get("name"))
        else {
            return Err(DatabaseError::InvalidSchema(
                "PostgreSQL catalog object name",
            ));
        };
        if excluded
            .iter()
            .any(|excluded| excluded == name || excluded == &format!("{schema}.{name}"))
        {
            continue;
        }
        targets.push(format!(
            "{}.{}",
            grammar.quote(schema)?,
            grammar.quote(name)?
        ));
    }
    Ok((!targets.is_empty())
        .then(|| format!("drop {kind} if exists {} cascade", targets.join(", "))))
}

#[cfg(test)]
#[path = "../../../../tests/unit/postgres_schema_wipe.rs"]
mod tests;
